use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use crate::extract::{Chunk, Extracted, document_id};
use crate::{
    ContentUse, GameStatus, KnowledgeError, KnowledgeLibrary, KnowledgeSettings, KnowledgeStatus,
    ModelStatus, Result, SourceStatus, SyncReport, embedding, sources, unix_seconds,
};
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};

pub(crate) async fn open(root: &Path) -> Result<SqlitePool> {
    tokio::fs::create_dir_all(root).await?;
    let options = SqliteConnectOptions::new()
        .filename(root.join("library.sqlite3"))
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(10));
    let pool = SqlitePoolOptions::new()
        .max_connections(4)
        .connect_with(options)
        .await?;
    sqlx::raw_sql("CREATE TABLE IF NOT EXISTS metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS sources (
            module_id TEXT NOT NULL, source_id TEXT NOT NULL, state TEXT NOT NULL,
            checked_at INTEGER, success_at INTEGER, error TEXT, catalog_hash TEXT,
            PRIMARY KEY(module_id, source_id));
        CREATE TABLE IF NOT EXISTS documents (
            module_id TEXT NOT NULL, id TEXT NOT NULL, source_id TEXT NOT NULL,
            url TEXT NOT NULL, title TEXT NOT NULL, body TEXT NOT NULL, links TEXT NOT NULL,
            content_hash TEXT NOT NULL, etag TEXT, modified TEXT, retrieved_at INTEGER NOT NULL,
            model_revision TEXT NOT NULL, content_use TEXT NOT NULL DEFAULT 'reference', PRIMARY KEY(module_id,id));
        CREATE INDEX IF NOT EXISTS docs_source ON documents(module_id,source_id);
        CREATE TABLE IF NOT EXISTS chunks (
            id INTEGER PRIMARY KEY, module_id TEXT NOT NULL, document_id TEXT NOT NULL,
            heading TEXT NOT NULL, body TEXT NOT NULL, offset_bytes INTEGER NOT NULL, vector BLOB NOT NULL,
            FOREIGN KEY(module_id,document_id) REFERENCES documents(module_id,id) ON DELETE CASCADE);
        CREATE INDEX IF NOT EXISTS chunks_game ON chunks(module_id);
        CREATE VIRTUAL TABLE IF NOT EXISTS chunk_fts USING fts5(title,heading,body,module_id UNINDEXED,tokenize='unicode61');
        CREATE TRIGGER IF NOT EXISTS chunks_delete AFTER DELETE ON chunks BEGIN DELETE FROM chunk_fts WHERE rowid=old.id; END;")
        .execute(&pool).await?;
    // Preserve pre-policy caches. Their delivery is limited to excerpts until
    // a successful refresh records the publisher's current use ceiling.
    let columns = sqlx::query("PRAGMA table_info(documents)")
        .fetch_all(&pool)
        .await?;
    if !columns
        .iter()
        .any(|row| row.get::<String, _>("name") == "content_use")
    {
        sqlx::query(
            "ALTER TABLE documents ADD COLUMN content_use TEXT NOT NULL DEFAULT 'reference'",
        )
        .execute(&pool)
        .await?;
    }
    Ok(pool)
}

#[derive(Clone)]
pub(crate) struct StoredDocument {
    pub content_use: ContentUse,
    pub url: String,
    pub content: Extracted,
    pub content_hash: String,
    pub etag: Option<String>,
    pub modified: Option<String>,
    pub retrieved_at: u64,
    pub model_revision: String,
}

pub(crate) struct PreparedDocument {
    pub stored: StoredDocument,
    /// None reuses the existing chunks; Some replaces all chunks atomically.
    pub chunks: Option<Vec<Chunk>>,
}

pub(crate) async fn cached(
    pool: &SqlitePool,
    module: &str,
    source: &str,
) -> Result<HashMap<String, StoredDocument>> {
    let rows = sqlx::query("SELECT * FROM documents WHERE module_id=? AND source_id=?")
        .bind(module)
        .bind(source)
        .fetch_all(pool)
        .await?;
    let mut docs = HashMap::new();
    for row in rows {
        let url: String = row.try_get("url")?;
        docs.insert(
            url.clone(),
            StoredDocument {
                content_use: ContentUse::from_stored(row.try_get("content_use")?)?,
                url,
                content: Extracted {
                    title: row.try_get("title")?,
                    body: row.try_get("body")?,
                    links: serde_json::from_str(row.try_get("links")?).map_err(|e| {
                        KnowledgeError::Invalid(format!("Stored document links are invalid: {e}"))
                    })?,
                },
                content_hash: row.try_get("content_hash")?,
                etag: row.try_get("etag")?,
                modified: row.try_get("modified")?,
                retrieved_at: row.try_get::<i64, _>("retrieved_at")? as u64,
                model_revision: row.try_get("model_revision")?,
            },
        );
    }
    Ok(docs)
}

pub(crate) async fn restrict_source_use(
    pool: &SqlitePool,
    module: &str,
    source: &str,
    content_use: ContentUse,
) -> Result<()> {
    if content_use == ContentUse::Reference {
        // A newly observed restriction applies even if a later page fails and
        // the old source snapshot must otherwise be preserved.
        sqlx::query(
            "UPDATE documents SET content_use='reference' WHERE module_id=? AND source_id=?",
        )
        .bind(module)
        .bind(source)
        .execute(pool)
        .await?;
    }
    Ok(())
}

pub(crate) async fn source_state(
    pool: &SqlitePool,
    module: &str,
    source: &str,
    state: &str,
    error: Option<&str>,
) -> Result<()> {
    // A transient network failure after policy withdrawal must not reactivate
    // old evidence. Only publish() after a permitted complete refresh clears it.
    sqlx::query("INSERT INTO sources(module_id,source_id,state,checked_at,error) VALUES(?,?,?,?,?) ON CONFLICT(module_id,source_id) DO UPDATE SET state=CASE WHEN sources.state='restricted' THEN 'restricted' ELSE excluded.state END,checked_at=excluded.checked_at,error=CASE WHEN sources.state='restricted' AND excluded.state!='restricted' THEN sources.error ELSE excluded.error END")
        .bind(module).bind(source).bind(state).bind(unix_seconds() as i64).bind(error).execute(pool).await?;
    Ok(())
}

/// A source is published as a whole. Failed/cancelled crawls never remove old
/// pages or expose half-updated vectors. Successful complete crawls retire pages
/// no longer present in the publisher's current navigable documentation scope.
pub(crate) async fn publish(
    pool: &SqlitePool,
    module: &str,
    source: &sources::Source,
    documents: Vec<PreparedDocument>,
    catalog_hash: &str,
) -> Result<usize> {
    if documents.is_empty() {
        return Err(KnowledgeError::Unavailable(
            "Source yielded no documents".into(),
        ));
    }
    let mut tx = pool.begin().await?;
    let previous: Vec<String> =
        sqlx::query_scalar("SELECT id FROM documents WHERE module_id=? AND source_id=?")
            .bind(module)
            .bind(&source.id)
            .fetch_all(&mut *tx)
            .await?;
    let mut keep = std::collections::HashSet::new();
    let mut changed = 0;
    for prepared in documents {
        let doc = prepared.stored;
        let id = document_id(&source.id, &doc.url);
        keep.insert(id.clone());
        sqlx::query("INSERT INTO documents(module_id,id,source_id,url,title,body,links,content_hash,etag,modified,retrieved_at,model_revision,content_use) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(module_id,id) DO UPDATE SET title=excluded.title,body=excluded.body,links=excluded.links,content_hash=excluded.content_hash,etag=excluded.etag,modified=excluded.modified,retrieved_at=excluded.retrieved_at,model_revision=excluded.model_revision,content_use=excluded.content_use")
            .bind(module).bind(&id).bind(&source.id).bind(&doc.url).bind(&doc.content.title).bind(&doc.content.body)
            .bind(serde_json::to_string(&doc.content.links).map_err(|e| KnowledgeError::Invalid(e.to_string()))?)
            .bind(&doc.content_hash).bind(&doc.etag).bind(&doc.modified).bind(doc.retrieved_at as i64).bind(&doc.model_revision).bind(doc.content_use.as_str()).execute(&mut *tx).await?;
        if let Some(chunks) = prepared.chunks {
            changed += 1;
            sqlx::query("DELETE FROM chunks WHERE module_id=? AND document_id=?")
                .bind(module)
                .bind(&id)
                .execute(&mut *tx)
                .await?;
            for chunk in chunks {
                if chunk.vector.len() != embedding::DIMENSIONS
                    || chunk.vector.iter().any(|v| !v.is_finite())
                {
                    return Err(KnowledgeError::Model("Invalid document vector".into()));
                }
                let bytes: Vec<u8> = chunk.vector.iter().flat_map(|v| v.to_le_bytes()).collect();
                let inserted = sqlx::query("INSERT INTO chunks(module_id,document_id,heading,body,offset_bytes,vector) VALUES(?,?,?,?,?,?)")
                    .bind(module).bind(&id).bind(&chunk.heading).bind(&chunk.body).bind(chunk.offset as i64).bind(bytes).execute(&mut *tx).await?;
                sqlx::query(
                    "INSERT INTO chunk_fts(rowid,title,heading,body,module_id) VALUES(?,?,?,?,?)",
                )
                .bind(inserted.last_insert_rowid())
                .bind(&doc.content.title)
                .bind(&chunk.heading)
                .bind(&chunk.body)
                .bind(module)
                .execute(&mut *tx)
                .await?;
            }
        }
    }
    for id in previous {
        if !keep.contains(&id) {
            sqlx::query("DELETE FROM documents WHERE module_id=? AND id=?")
                .bind(module)
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
    }
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chunks WHERE module_id=?")
        .bind(module)
        .fetch_one(&mut *tx)
        .await?;
    if count > 20_000 {
        return Err(KnowledgeError::Unavailable(format!(
            "{module} exceeds the 20,000-chunk index budget; previous snapshot retained"
        )));
    }
    sqlx::query("INSERT INTO sources(module_id,source_id,state,checked_at,success_at,error,catalog_hash) VALUES(?,?,'ready',?,?,NULL,?) ON CONFLICT(module_id,source_id) DO UPDATE SET state='ready',checked_at=excluded.checked_at,success_at=excluded.success_at,error=NULL,catalog_hash=excluded.catalog_hash")
        .bind(module).bind(&source.id).bind(unix_seconds() as i64).bind(unix_seconds() as i64).bind(catalog_hash).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(changed)
}

impl KnowledgeLibrary {
    pub async fn settings(&self) -> Result<KnowledgeSettings> {
        match metadata(&self.pool, "settings").await? {
            Some(value) => serde_json::from_str(&value).map_err(|e| {
                KnowledgeError::Invalid(format!("Invalid saved knowledge settings: {e}"))
            }),
            None => Ok(KnowledgeSettings::default()),
        }
    }

    pub async fn update_settings(&self, settings: KnowledgeSettings) -> Result<()> {
        if !(6..=168).contains(&settings.interval_hours) {
            return Err(KnowledgeError::Invalid(
                "Knowledge update interval must be between 6 and 168 hours".into(),
            ));
        }
        save_metadata(&self.pool, "settings", &settings).await
    }

    pub async fn status(&self) -> Result<KnowledgeStatus> {
        let root = self.modules_root.clone();
        let catalogs = tokio::task::spawn_blocking(move || sources::load_all(&root))
            .await
            .map_err(|e| KnowledgeError::Unavailable(e.to_string()))??;
        let mut games = Vec::new();
        for catalog in catalogs {
            let mut statuses = Vec::new();
            for source in catalog.sources {
                let row = sqlx::query("SELECT state,checked_at,success_at,error FROM sources WHERE module_id=? AND source_id=?").bind(&catalog.module_id).bind(&source.id).fetch_optional(&self.pool).await?;
                let (documents, chunks): (i64, i64) = sqlx::query_as("SELECT COUNT(DISTINCT d.id),COUNT(c.id) FROM documents d LEFT JOIN chunks c ON c.module_id=d.module_id AND c.document_id=d.id WHERE d.module_id=? AND d.source_id=?").bind(&catalog.module_id).bind(&source.id).fetch_one(&self.pool).await?;
                let source_url = source.citation_url(&source.seeds[0]);
                statuses.push(SourceStatus {
                    id: source.id,
                    title: source.title,
                    authority: source.authority,
                    kind: source.kind,
                    url: source_url,
                    document_count: documents as u64,
                    chunk_count: chunks as u64,
                    state: if source.reference_only { "restricted".into() } else { row
                        .as_ref()
                        .map(|r| r.try_get("state"))
                        .transpose()?
                        .unwrap_or_else(|| "pending".into()) },
                    last_checked_at: row
                        .as_ref()
                        .map(|r| r.try_get::<Option<i64>, _>("checked_at"))
                        .transpose()?
                        .flatten()
                        .map(|n| n as u64),
                    last_success_at: row
                        .as_ref()
                        .map(|r| r.try_get::<Option<i64>, _>("success_at"))
                        .transpose()?
                        .flatten()
                        .map(|n| n as u64),
                    last_error: if source.reference_only { Some("This source permits reference links only; background AI ingestion is disabled.".into()) } else { row
                        .as_ref()
                        .map(|r| r.try_get("error"))
                        .transpose()?
                        .flatten() },
                });
            }
            games.push(GameStatus {
                module_id: catalog.module_id,
                scope: catalog.scope,
                gaps: catalog.gaps,
                sources: statuses,
            });
        }
        let model_path = self.root.join("model").join(embedding::MODEL_SLUG);
        let ready = tokio::task::spawn_blocking(move || embedding::installed(&model_path))
            .await
            .map_err(|e| KnowledgeError::Model(e.to_string()))?;
        let last_run = metadata(&self.pool, "last_run")
            .await?
            .map(|text| serde_json::from_str::<SyncReport>(&text))
            .transpose()
            .map_err(|e| KnowledgeError::Invalid(e.to_string()))?;
        Ok(KnowledgeStatus {
            settings: self.settings().await?,
            model: ModelStatus {
                id: embedding::MODEL_ID.into(),
                revision: embedding::REVISION.into(),
                ready,
                download_bytes: embedding::DOWNLOAD_BYTES,
            },
            games,
            last_run,
        })
    }
}

pub(crate) async fn metadata(pool: &SqlitePool, key: &str) -> Result<Option<String>> {
    Ok(sqlx::query_scalar("SELECT value FROM metadata WHERE key=?")
        .bind(key)
        .fetch_optional(pool)
        .await?)
}

pub(crate) async fn save_metadata(
    pool: &SqlitePool,
    key: &str,
    value: &impl serde::Serialize,
) -> Result<()> {
    let encoded =
        serde_json::to_string(value).map_err(|e| KnowledgeError::Invalid(e.to_string()))?;
    sqlx::query("INSERT INTO metadata(key,value) VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value").bind(key).bind(encoded).execute(pool).await?;
    Ok(())
}
