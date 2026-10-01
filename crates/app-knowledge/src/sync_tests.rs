use std::collections::HashSet;

use reqwest::Url;
use sqlx::Row;

use crate::{embedding, extract, search, sources, store, sync};

#[test]
fn sitemaps_and_documents_share_transfer_and_discovery_budgets() {
    let mut received = 0;
    for _ in 0..8 {
        sync::record_transfer(&mut received, 8 * 1024 * 1024).unwrap();
    }
    assert!(sync::record_transfer(&mut received, 1).is_err());
    let mut known: HashSet<String> = (0..50_000)
        .map(|i| format!("https://example.com/{i}"))
        .collect();
    let mut queue = std::collections::VecDeque::new();
    sync::enqueue(&mut queue, &mut known, "https://example.com/0".into()).unwrap();
    assert!(queue.is_empty());
    assert!(sync::enqueue(&mut queue, &mut known, "https://example.com/50000".into()).is_err());
    assert_eq!(known.len(), 50_000);
}

fn source() -> sources::Source {
    sources::Source {
        id: "current".into(),
        title: "Official manual".into(),
        authority: "Publisher".into(),
        kind: "official".into(),
        seeds: vec!["https://example.com/current/start".into()],
        allowed_prefixes: vec!["https://example.com/current/".into()],
        discover_links: true,
        reference_only: false,
        max_pages: 4,
        content_selector: None,
        discovery_selector: None,
        sitemaps: vec![],
        authority_evidence: "https://example.com/about".into(),
        license_note: "Public reference".into(),
        license_url: None,
        reviewed_on: "2026-09-28".into(),
    }
}

fn cached_document() -> store::StoredDocument {
    store::StoredDocument {
        content_use: crate::ContentUse::Full,
        url: "https://example.com/current/canonical".into(),
        content: extract::Extracted {
            title: "Server setup".into(),
            body: "Previously extracted body".into(),
            links: vec![],
        },
        content_hash: "original".into(),
        etag: Some("resource-version".into()),
        modified: None,
        retrieved_at: 100,
        model_revision: embedding::REVISION.into(),
    }
}

#[tokio::test]
async fn obsolete_sources_and_narrowed_urls_cannot_consume_search_page_positions() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    let catalog = sources::Catalog {
        schema_version: 1,
        module_id: "minecraft".into(),
        scope: "Server manual".into(),
        gaps: vec![],
        sources: vec![source()],
    };
    let mut rows = Vec::new();
    // Removed sources and no-longer-reviewed URLs precede all usable rows.
    // Filtering after take(5) would make the entire result undiscoverable.
    for index in 0..12_i64 {
        rows.push(
            sqlx::query("SELECT ? AS id, ? AS source_id, ? AS url")
                .bind(index)
                .bind(if index < 6 { "removed" } else { "current" })
                .bind(format!("https://example.com/obsolete/{index}"))
                .fetch_one(&pool)
                .await
                .unwrap(),
        );
    }
    for index in 12..24_i64 {
        rows.push(
            sqlx::query("SELECT ? AS id, 'current' AS source_id, ? AS url")
                .bind(index)
                .bind(format!("https://example.com/current/{index}"))
                .fetch_one(&pool)
                .await
                .unwrap(),
        );
    }
    let eligible = search::eligible_rows(rows, &catalog).unwrap();
    let ids: Vec<i64> = eligible
        .iter()
        .map(|row| row.try_get("id").unwrap())
        .collect();
    assert_eq!(ids, (12..24).collect::<Vec<_>>());
    assert_eq!(
        ids.chunks(5).map(<[i64]>::len).collect::<Vec<_>>(),
        vec![5, 5, 2]
    );
    pool.close().await;
}

#[tokio::test]
async fn changed_or_absent_source_policy_disables_http_cache_validators() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::query("CREATE TABLE sources(module_id TEXT,source_id TEXT,catalog_hash TEXT)")
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        !sync::source_policy_matches(&pool, "minecraft", "current", "new-policy")
            .await
            .unwrap()
    );
    sqlx::query("INSERT INTO sources VALUES('minecraft','current','old-selector')")
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        !sync::source_policy_matches(&pool, "minecraft", "current", "new-selector")
            .await
            .unwrap()
    );
    assert!(
        sync::source_policy_matches(&pool, "minecraft", "current", "old-selector")
            .await
            .unwrap()
    );
    assert!(
        !sync::source_policy_matches(&pool, "palworld", "current", "old-selector")
            .await
            .unwrap()
    );
    pool.close().await;
}

#[test]
fn unchanged_documents_rebuild_passages_after_index_policy_changes() {
    let cached = cached_document();
    let current = cached.clone();
    assert!(!sync::needs_index(Some(&cached), &current, true));
    assert!(sync::needs_index(Some(&cached), &current, false));
    assert!(sync::needs_index(None, &current, true));

    let mut old_model = cached.clone();
    old_model.model_revision = "older-model-revision".into();
    assert!(sync::needs_index(Some(&old_model), &current, true));

    let mut updated = current;
    updated.content_hash = "new-body-or-title".into();
    assert!(sync::needs_index(Some(&cached), &updated, true));
}

#[test]
fn redirected_aliases_share_one_canonical_document_without_consuming_page_budget() {
    let final_url = Url::parse("https://example.com/current/canonical").unwrap();
    let other = Url::parse("https://example.com/current/other").unwrap();
    let mut canonical = HashSet::new();
    assert!(sync::record_canonical(&mut canonical, &final_url, 1).unwrap());
    // A second seed redirecting to the same final URL must be a no-op, even
    // after reaching the distinct-document budget.
    assert!(!sync::record_canonical(&mut canonical, &final_url, 1).unwrap());
    assert_eq!(canonical.len(), 1);
    assert!(sync::record_canonical(&mut canonical, &other, 1).is_err());
}

#[test]
fn not_modified_response_requires_the_exact_cached_resource_and_sent_validators() {
    let mut cached = cached_document();
    let canonical = Url::parse(&cached.url).unwrap();
    let alias = Url::parse("https://example.com/current/alias").unwrap();
    let reused = sync::cached_not_modified(
        Some(&cached),
        &canonical,
        &canonical,
        true,
        crate::ContentUse::Full,
    )
    .unwrap();
    assert_eq!(reused.content_hash, "original");
    assert_eq!(reused.url, canonical.as_str());
    assert!(
        sync::cached_not_modified(None, &canonical, &canonical, true, crate::ContentUse::Full)
            .is_err()
    );
    assert!(
        sync::cached_not_modified(
            Some(&cached),
            &canonical,
            &canonical,
            false,
            crate::ContentUse::Full
        )
        .is_err()
    );
    assert!(
        sync::cached_not_modified(
            Some(&cached),
            &alias,
            &canonical,
            true,
            crate::ContentUse::Full
        )
        .is_err()
    );
    assert!(
        sync::cached_not_modified(Some(&cached), &alias, &alias, true, crate::ContentUse::Full)
            .is_err()
    );
    cached.etag = None;
    assert!(
        sync::cached_not_modified(
            Some(&cached),
            &canonical,
            &canonical,
            true,
            crate::ContentUse::Full
        )
        .is_err()
    );
}

#[test]
fn not_modified_response_applies_new_policy_without_restoring_old_full_delivery() {
    let cached = cached_document();
    let url = Url::parse(&cached.url).unwrap();
    let restricted = sync::cached_not_modified(
        Some(&cached),
        &url,
        &url,
        true,
        crate::ContentUse::Reference,
    )
    .unwrap();
    assert_eq!(restricted.content_use, crate::ContentUse::Reference);
    assert_eq!(restricted.content.body, cached.content.body);
    let still_restricted =
        sync::cached_not_modified(Some(&restricted), &url, &url, true, crate::ContentUse::Full)
            .unwrap();
    assert_eq!(still_restricted.content_use, crate::ContentUse::Reference);
}

#[tokio::test]
async fn reference_results_keep_only_the_best_chunk_across_search_pages() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    let mut rows = Vec::new();
    for (id, policy) in [
        ("reference-a", "reference"),
        ("full-a", "full"),
        ("reference-a", "reference"),
        ("reference-b", "reference"),
        ("full-a", "full"),
        ("reference-a", "reference"),
    ] {
        rows.push(
            sqlx::query("SELECT ? AS document_id, ? AS content_use")
                .bind(id)
                .bind(policy)
                .fetch_one(&pool)
                .await
                .unwrap(),
        );
    }
    // Rank 2 is more relevant than rank 0 for the same reference document.
    let mut ranked = vec![
        (2, 0.9, 0.9),
        (1, 0.8, 0.8),
        (0, 0.7, 0.7),
        (3, 0.6, 0.6),
        (4, 0.5, 0.5),
        (5, 0.4, 0.4),
    ];
    search::restrict_reference_ranks(&mut ranked, &rows).unwrap();
    assert_eq!(
        ranked.iter().map(|entry| entry.0).collect::<Vec<_>>(),
        vec![2, 1, 3, 4]
    );
    assert_eq!(ranked.chunks(2).count(), 2);
    let body = "设置服务器密码🔒".repeat(100);
    let excerpt = search::excerpt(body.clone(), crate::ContentUse::Reference);
    assert_eq!(excerpt.chars().count(), 400);
    assert!(body.starts_with(&excerpt));
    assert_eq!(search::excerpt(body.clone(), crate::ContentUse::Full), body);
    pool.close().await;
}
