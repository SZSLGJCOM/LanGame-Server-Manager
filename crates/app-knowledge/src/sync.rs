use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, atomic::AtomicBool};
use std::time::{Duration, Instant};

use crate::store::{PreparedDocument, StoredDocument};
use crate::{
    KnowledgeError, KnowledgeLibrary, Result, SyncProgress, SyncReport, check_cancel, embedding,
    extract, fetch::Fetcher, sources, store, unix_seconds,
};
use reqwest::{StatusCode, Url};
use sqlx::Row;

pub type ProgressCallback = Arc<dyn Fn(SyncProgress) + Send + Sync>;

impl KnowledgeLibrary {
    pub async fn sync(
        &self,
        module_id: Option<&str>,
        force: bool,
        cancel: Arc<AtomicBool>,
        progress: ProgressCallback,
    ) -> Result<SyncReport> {
        let _guard = self.sync_gate.try_lock().map_err(|_| {
            KnowledgeError::Unavailable("Knowledge synchronization is already running".into())
        })?;
        let root = self.modules_root.clone();
        let module = module_id.map(str::to_owned);
        let catalogs = tokio::task::spawn_blocking(move || match module {
            Some(module) => sources::load(&root, &module).map(|catalog| vec![catalog]),
            None => sources::load_all(&root),
        })
        .await
        .map_err(|e| KnowledgeError::Unavailable(e.to_string()))??;
        let mut report = SyncReport {
            started_at: unix_seconds(),
            ..Default::default()
        };
        let download_progress = progress.clone();
        let downloaded = embedding::ensure_model(
            &self.root.join("model").join(embedding::MODEL_SLUG),
            &cancel,
            &move |received, total| {
                download_progress(SyncProgress {
                    phase: "model_download".into(),
                    downloaded_bytes: received,
                    total_download_bytes: total,
                    ..Default::default()
                });
            },
        )
        .await;
        if let Err(error) = downloaded {
            report.cancelled = matches!(error, KnowledgeError::Cancelled);
            report.errors.push(error.to_string());
            report.finished_at = unix_seconds();
            store::save_metadata(&self.pool, "last_run", &report).await?;
            return Err(error);
        }
        progress(SyncProgress {
            phase: "model_loading".into(),
            ..Default::default()
        });
        self.encoder().await?;
        let settings = self.settings().await?;
        let total = catalogs.iter().map(|catalog| catalog.sources.len()).sum();
        let deadline = Instant::now() + Duration::from_secs(30 * 60);
        let mut completed = 0;
        let mut fetcher = Fetcher::new();
        for catalog in catalogs {
            for source in catalog.sources {
                if check_cancel(&cancel).is_err() {
                    report.cancelled = true;
                    break;
                }
                if Instant::now() >= deadline {
                    report.errors.push("Documentation synchronization reached its 30-minute budget; remaining sources will resume on the next sync".into());
                    break;
                }
                let catalog_hash = extract::digest(
                    format!(
                        "public-docs-extraction-5-discovery\n{}\n{}",
                        embedding::REVISION,
                        serde_json::to_string(&source)
                            .map_err(|e| KnowledgeError::Invalid(e.to_string()))?
                    )
                    .as_bytes(),
                );
                if !force
                    && !source_due(
                        &self.pool,
                        &catalog.module_id,
                        &source.id,
                        &catalog_hash,
                        settings.interval_hours,
                    )
                    .await?
                {
                    completed += 1;
                    continue;
                }
                progress(SyncProgress {
                    phase: "documents".into(),
                    module_id: Some(catalog.module_id.clone()),
                    source_id: Some(source.id.clone()),
                    completed,
                    total,
                    ..Default::default()
                });
                let result = self
                    .sync_source(
                        &catalog.module_id,
                        &source,
                        &catalog_hash,
                        &mut fetcher,
                        cancel.clone(),
                        deadline,
                    )
                    .await;
                match result {
                    Ok(changed) => {
                        report.sources_succeeded += 1;
                        report.changed_documents += changed;
                    }
                    Err(KnowledgeError::Cancelled) => {
                        store::source_state(
                            &self.pool,
                            &catalog.module_id,
                            &source.id,
                            "interrupted",
                            Some("Synchronization cancelled; previous source snapshot retained"),
                        )
                        .await?;
                        report.cancelled = true;
                        break;
                    }
                    Err(error) => {
                        let message = format!("{}/{}: {}", catalog.module_id, source.id, error);
                        store::source_state(
                            &self.pool,
                            &catalog.module_id,
                            &source.id,
                            if matches!(error, KnowledgeError::Policy(_)) {
                                "restricted"
                            } else {
                                "error"
                            },
                            Some(&message),
                        )
                        .await?;
                        report.sources_failed += 1;
                        report.errors.push(message);
                    }
                }
                completed += 1;
            }
            if report.cancelled || Instant::now() >= deadline {
                break;
            }
        }
        report.finished_at = unix_seconds();
        store::save_metadata(&self.pool, "last_run", &report).await?;
        progress(SyncProgress {
            phase: if report.cancelled {
                "cancelled"
            } else {
                "finished"
            }
            .into(),
            completed,
            total,
            ..Default::default()
        });
        Ok(report)
    }

    async fn sync_source(
        &self,
        module: &str,
        source: &sources::Source,
        catalog_hash: &str,
        fetcher: &mut Fetcher,
        cancel: Arc<AtomicBool>,
        round_deadline: Instant,
    ) -> Result<usize> {
        if source.reference_only {
            return Err(KnowledgeError::Policy("Reviewed publisher terms allow reference links only; background AI ingestion is disabled".into()));
        }
        let deadline = round_deadline.min(Instant::now() + Duration::from_secs(10 * 60));
        let encoder = self.encoder().await?;
        let cached = store::cached(&self.pool, module, &source.id).await?;
        // A new extraction selector or source policy invalidates cached parsed
        // content even when the publisher's original HTTP body has not changed.
        let conditional_allowed =
            source_policy_matches(&self.pool, module, &source.id, catalog_hash).await?;
        let mut queue: VecDeque<String> = source.seeds.iter().cloned().collect();
        let mut queued: HashSet<String> = source.seeds.iter().cloned().collect();
        let mut received = 0;
        for sitemap in &source.sitemaps {
            check_cancel(&cancel)?;
            crate::indexing::check_deadline(deadline)?;
            let page = fetcher
                .page(source, &sources::public_url(sitemap)?, None, None, &cancel)
                .await?;
            store::restrict_source_use(&self.pool, module, &source.id, page.content_use).await?;
            record_transfer(&mut received, page.body.len())?;
            crate::indexing::check_deadline(deadline)?;
            for link in extract::sitemap_links(&page.body)? {
                if Url::parse(&link).is_ok_and(|url| source.allows(&url)) {
                    enqueue(&mut queue, &mut queued, link)?;
                }
            }
        }
        let mut seen = HashSet::new();
        let mut canonical_urls = HashSet::new();
        let mut prepared = Vec::new();
        let mut prepared_text_bytes = 0_usize;
        let mut prepared_vectors = 0_usize;
        while let Some(url) = queue.pop_front() {
            check_cancel(&cancel)?;
            if !seen.insert(url.clone()) {
                continue;
            }
            if Instant::now() >= deadline {
                return Err(KnowledgeError::Network(
                    "Source exceeded its synchronization deadline; previous snapshot retained"
                        .into(),
                ));
            }
            let parsed = sources::public_url(&url)?;
            if !source.allows(&parsed) {
                continue;
            }
            let validator = cached.get(parsed.as_str()).filter(|_| conditional_allowed);
            let response = fetcher
                .page(
                    source,
                    &parsed,
                    validator.and_then(|d| d.etag.as_deref()),
                    validator.and_then(|d| d.modified.as_deref()),
                    &cancel,
                )
                .await?;
            store::restrict_source_use(&self.pool, module, &source.id, response.content_use)
                .await?;
            record_transfer(&mut received, response.body.len())?;
            crate::indexing::check_deadline(deadline)?;
            if !record_canonical(&mut canonical_urls, &response.url, source.max_pages)? {
                continue;
            }
            seen.insert(response.url.to_string());
            let previous = cached.get(response.url.as_str());
            let mut stored = if response.status == StatusCode::NOT_MODIFIED {
                let mut stored = cached_not_modified(
                    previous,
                    &parsed,
                    &response.url,
                    conditional_allowed,
                    response.content_use,
                )?;
                stored.retrieved_at = unix_seconds();
                stored
            } else {
                let source = source.clone();
                let url = response.url.clone();
                let parser_cancel = cancel.clone();
                let extracted = tokio::task::spawn_blocking(move || {
                    extract::extract_with_context(
                        &source,
                        &url,
                        &response.content_type,
                        &response.body,
                        &parser_cancel,
                        deadline,
                    )
                })
                .await
                .map_err(|e| {
                    KnowledgeError::Invalid(format!("Documentation parser failed: {e}"))
                })??;
                // Include title as well as body: title changes affect chunk vectors.
                let content_hash =
                    extract::digest(format!("{}\n{}", extracted.title, extracted.body).as_bytes());
                StoredDocument {
                    content_use: response.content_use,
                    url: response.url.to_string(),
                    content: extracted,
                    content_hash,
                    etag: response.etag,
                    modified: response.modified,
                    retrieved_at: unix_seconds(),
                    model_revision: embedding::REVISION.into(),
                }
            };
            if source.discover_links {
                for link in &stored.content.links {
                    if !seen.contains(link) {
                        enqueue(&mut queue, &mut queued, link.clone())?;
                    }
                }
            }
            // A directory landing page may contain only its heading and links.
            // Follow its reviewed children but do not index navigation as a
            // substantive manual. The source still needs real document bodies.
            if !extract::substantive(source, &stored.content.body) {
                continue;
            }
            prepared_text_bytes = prepared_text_bytes.saturating_add(stored.content.body.len());
            if prepared_text_bytes > 64 * 1024 * 1024 {
                return Err(KnowledgeError::Unavailable("Source exceeds the 64 MiB extracted-text preparation budget; previous snapshot retained".into()));
            }
            let changed = needs_index(previous, &stored, conditional_allowed);
            let chunks = if changed {
                let title = stored.content.title.clone();
                let body = stored.content.body.clone();
                let encoder = encoder.clone();
                let cancellation = cancel.clone();
                Some(
                    tokio::task::spawn_blocking(move || {
                        crate::indexing::prepare(&title, &body, &encoder, &cancellation, deadline)
                    })
                    .await
                    .map_err(|e| KnowledgeError::Model(e.to_string()))??,
                )
            } else {
                None
            };
            prepared_vectors += chunks.as_ref().map_or(0, Vec::len);
            if prepared_vectors > 20_000 {
                return Err(KnowledgeError::Unavailable("Source exceeds the 20,000-vector preparation budget; previous snapshot retained".into()));
            }
            stored.model_revision = embedding::REVISION.into();
            prepared.push(PreparedDocument { stored, chunks });
        }
        check_cancel(&cancel)?;
        crate::indexing::check_deadline(deadline)?;
        store::publish(&self.pool, module, source, prepared, catalog_hash).await
    }
}

pub(crate) fn needs_index(
    previous: Option<&StoredDocument>,
    current: &StoredDocument,
    source_policy_matches: bool,
) -> bool {
    // Chunking and extraction revisions are part of the source policy. An
    // unchanged body can still require different passages and vectors.
    !source_policy_matches
        || previous.is_none_or(|previous| {
            previous.content_hash != current.content_hash
                || previous.model_revision != embedding::REVISION
        })
}

pub(crate) fn record_transfer(received: &mut usize, bytes: usize) -> Result<()> {
    *received = received.saturating_add(bytes);
    if *received > 64 * 1024 * 1024 {
        return Err(KnowledgeError::Unavailable(
            "Source exceeded the 64 MiB transfer budget; previous snapshot retained".into(),
        ));
    }
    Ok(())
}

pub(crate) fn enqueue(
    queue: &mut VecDeque<String>,
    known: &mut HashSet<String>,
    url: String,
) -> Result<()> {
    if known.contains(&url) {
        return Ok(());
    }
    if known.len() >= 50_000 {
        return Err(KnowledgeError::Unavailable(
            "Source discovery exceeded 50,000 unique links".into(),
        ));
    }
    known.insert(url.clone());
    queue.push_back(url);
    Ok(())
}

pub(crate) async fn source_policy_matches(
    pool: &sqlx::SqlitePool,
    module: &str,
    source: &str,
    hash: &str,
) -> Result<bool> {
    let stored: Option<Option<String>> =
        sqlx::query_scalar("SELECT catalog_hash FROM sources WHERE module_id=? AND source_id=?")
            .bind(module)
            .bind(source)
            .fetch_optional(pool)
            .await?;
    Ok(stored.flatten().as_deref() == Some(hash))
}

pub(crate) fn record_canonical(
    seen: &mut HashSet<String>,
    url: &Url,
    limit: usize,
) -> Result<bool> {
    if seen.contains(url.as_str()) {
        return Ok(false);
    }
    if seen.len() >= limit {
        return Err(KnowledgeError::Unavailable(format!(
            "Source exceeded its {limit} page scope; previous snapshot retained"
        )));
    }
    seen.insert(url.to_string());
    Ok(true)
}

pub(crate) fn cached_not_modified(
    previous: Option<&StoredDocument>,
    requested: &Url,
    final_url: &Url,
    conditional_allowed: bool,
    content_use: crate::ContentUse,
) -> Result<StoredDocument> {
    previous
        .filter(|document| {
            conditional_allowed
                && requested == final_url
                && document.url == final_url.as_str()
                && (document.etag.is_some() || document.modified.is_some())
        })
        .cloned()
        .map(|mut document| {
            // A 304 is not a fresh grant of broader use. Apply new restrictions
            // and retain old header restrictions omitted from a 304 response.
            document.content_use = document.content_use.restrict(content_use);
            document
        })
        .ok_or_else(|| {
            KnowledgeError::Network(
                "Publisher returned 304 without matching cached content and request validators"
                    .into(),
            )
        })
}

async fn source_due(
    pool: &sqlx::SqlitePool,
    module: &str,
    source: &str,
    hash: &str,
    interval_hours: u32,
) -> Result<bool> {
    let row = sqlx::query(
        "SELECT checked_at,catalog_hash FROM sources WHERE module_id=? AND source_id=?",
    )
    .bind(module)
    .bind(source)
    .fetch_optional(pool)
    .await?;
    let Some(row) = row else {
        return Ok(true);
    };
    let checked: Option<i64> = row.try_get("checked_at")?;
    let previous_hash: Option<String> = row.try_get("catalog_hash")?;
    Ok(previous_hash.as_deref() != Some(hash)
        || checked.is_none_or(|checked| {
            unix_seconds().saturating_sub(checked as u64) >= u64::from(interval_hours) * 3600
        }))
}
