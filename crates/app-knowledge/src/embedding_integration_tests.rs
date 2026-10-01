//! Opt-in, network-free acceptance against explicitly supplied public snapshots.
//! Output is retained for inspection; neither the source DB nor model cache is opened for writing.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, atomic::AtomicBool};
use std::time::{Duration, Instant};

use sqlx::{Row, SqlitePool, sqlite::SqliteConnectOptions};

use crate::{
    ContentUse, KnowledgeError, KnowledgeLibrary, embedding, extract, indexing, sources, store,
};

type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
const SELECTED: &[(&str, &str)] = &[
    ("palworld", "b8b48b8dff86b06d347293808f7e992d"),
    ("palworld", "24783debfb20aa3b43d16f5fe292d6d3"),
    ("barotrauma", "fdb82537294865330335072da691e8e0"),
    ("barotrauma", "3ea258610efa27aea56ed5aa589d36b5"),
    ("necesse", "8419671fa6c84df0168f5e2b0ef7cafd"),
    ("necesse", "9b9f1ff2296eb49867eb873401030d34"),
    ("terraria", "3ff7804d9c0a2ed37b2d50390670b350"),
];

struct Snapshot {
    module: String,
    id: String,
    source: sources::Source,
    stored: store::StoredDocument,
    chunks: Vec<(String, String, i64, Vec<u8>)>,
}

fn require(condition: bool, message: impl Into<String>) -> TestResult {
    if !condition {
        return Err(std::io::Error::other(message.into()).into());
    }
    Ok(())
}

fn explicit_path(name: &str) -> TestResult<PathBuf> {
    let path = PathBuf::from(std::env::var_os(name).ok_or_else(|| {
        std::io::Error::other(format!(
            "Set {name} explicitly; no path discovery is performed"
        ))
    })?);
    require(path.is_absolute(), format!("{name} must be absolute"))?;
    require(
        !path
            .components()
            .any(|part| part == std::path::Component::ParentDir),
        format!("{name} must not contain parent traversal"),
    )?;
    Ok(path)
}

fn external_existing(name: &str, directory: bool, repository: &Path) -> TestResult<PathBuf> {
    let path = explicit_path(name)?;
    let metadata = std::fs::symlink_metadata(&path)?;
    require(
        !metadata.file_type().is_symlink(),
        format!("{name} must not be a symlink"),
    )?;
    require(
        metadata.is_dir() == directory,
        format!("{name} has the wrong file type"),
    )?;
    let path = std::fs::canonicalize(path)?;
    require(
        !path.starts_with(repository),
        format!("{name} must be outside the repository"),
    )?;
    Ok(path)
}

fn prepare_paths(repository: &Path) -> TestResult<(PathBuf, PathBuf)> {
    let database = external_existing("LANGAME_KNOWLEDGE_SNAPSHOT_DB", false, repository)?;
    require(
        std::fs::metadata(&database)?.len() <= 128 * 1024 * 1024,
        "Snapshot exceeds 128 MiB",
    )?;
    let mut wal = database.as_os_str().to_owned();
    wal.push("-wal");
    match std::fs::metadata(PathBuf::from(wal)) {
        Ok(metadata) => require(
            metadata.len() == 0,
            "Checkpoint and freeze the input DB first; a nonempty WAL is not accepted",
        )?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let model = external_existing("LANGAME_EMBEDDING_MODEL_DIR", true, repository)?;
    let requested = explicit_path("LANGAME_KNOWLEDGE_REINDEX_ROOT")?;
    let parent = std::fs::canonicalize(
        requested
            .parent()
            .ok_or_else(|| std::io::Error::other("Missing output parent"))?,
    )?;
    let root = parent.join(
        requested
            .file_name()
            .ok_or_else(|| std::io::Error::other("Missing output name"))?,
    );
    require(
        !root.starts_with(repository),
        "Output must be outside the repository",
    )?;
    require(
        !root.starts_with(&model) && !root.starts_with(database.parent().unwrap()),
        "Output must be separate from the source DB and model cache",
    )?;
    // create_dir is deliberately not create_dir_all and never accepts an existing destination.
    std::fs::create_dir(&root)?;
    let target = root.join("model").join(embedding::MODEL_SLUG);
    std::fs::create_dir_all(&target)?;
    for name in ["model.onnx", "tokenizer.json", "config.json", "LICENSE.txt"] {
        let source = model.join(name);
        let metadata = std::fs::symlink_metadata(&source)?;
        require(
            metadata.is_file()
                && !metadata.file_type().is_symlink()
                && metadata.len() <= 256 * 1024 * 1024,
            format!("Invalid model input: {name}"),
        )?;
        std::fs::copy(source, target.join(name))?;
    }
    // Embedder::load verifies the copied bytes. Native runtime extraction occurs
    // only beneath this new root, never beneath the supplied model cache.
    Ok((database, root))
}

async fn read_snapshot(database: &Path, modules: &Path, all: bool) -> TestResult<Vec<Snapshot>> {
    let input = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(database)
                .read_only(true)
                .immutable(true),
        )
        .await?;
    let outcome = async {
        let selection: Vec<(String, String)> = if all {
            let rows = sqlx::query_as("SELECT module_id,id FROM documents ORDER BY module_id,id LIMIT 513")
                .fetch_all(&input).await?;
            require(!rows.is_empty() && rows.len() <= 512, "Full reindex accepts 1–512 documents")?;
            rows
        } else {
            SELECTED.iter().map(|(module, id)| ((*module).into(), (*id).into())).collect()
        };
        let mut snapshots = Vec::new();
        for (module, id) in &selection {
            let row = sqlx::query("SELECT d.*,s.state FROM documents d JOIN sources s ON s.module_id=d.module_id AND s.source_id=d.source_id WHERE d.module_id=? AND d.id=?")
                .bind(module).bind(id).fetch_one(&input).await?;
            require(row.try_get::<&str, _>("state")? == "ready", "Only ready public snapshot sources are accepted")?;
            let source_id: String = row.try_get("source_id")?;
            let catalog = sources::load(modules, module)?;
            let source = catalog.sources.into_iter().find(|source| source.id == source_id)
                .ok_or_else(|| std::io::Error::other("Snapshot source is absent from the reviewed catalog"))?;
            let title: String = row.try_get("title")?;
            let body: String = row.try_get("body")?;
            require(body.len() <= extract::MAX_TEXT && title.len() <= 8192, "Selected document exceeds the production text budget")?;
            let stored = store::StoredDocument {
                content_use: ContentUse::from_stored(row.try_get("content_use")?)?,
                url: row.try_get("url")?,
                content: extract::Extracted { title, body, links: serde_json::from_str(row.try_get("links")?)? },
                content_hash: row.try_get("content_hash")?,
                etag: row.try_get("etag")?,
                modified: row.try_get("modified")?,
                retrieved_at: row.try_get::<i64, _>("retrieved_at")?.try_into()?,
                model_revision: row.try_get("model_revision")?,
            };
            require(!source.reference_only && source.allows(&reqwest::Url::parse(&stored.url)?), "Snapshot no longer satisfies the shipped source policy")?;
            require(stored.model_revision != embedding::REVISION, "Supply the frozen pre-migration snapshot, not an already reindexed DB")?;
            require(extract::document_id(&source.id, &stored.url) == *id, "Snapshot document identity differs")?;
            require(extract::digest(format!("{}\n{}", stored.content.title, stored.content.body).as_bytes()) == stored.content_hash, "Snapshot body/hash differs")?;
            let rows = sqlx::query("SELECT heading,body,offset_bytes,vector FROM chunks WHERE module_id=? AND document_id=? ORDER BY offset_bytes LIMIT 4097")
                .bind(module).bind(id).fetch_all(&input).await?;
            require(!rows.is_empty() && rows.len() <= 4096, "Invalid snapshot chunk count")?;
            let mut chunks = Vec::new();
            for row in rows {
                let body: String = row.try_get("body")?;
                let offset: i64 = row.try_get("offset_bytes")?;
                let start = usize::try_from(offset)?;
                let vector: Vec<u8> = row.try_get("vector")?;
                require(vector.len() <= 16 * 1024 && !vector.is_empty(), "Invalid snapshot vector size")?;
                require(stored.content.body.as_bytes().get(start..start + body.len()) == Some(body.as_bytes()), "Chunk byte offsets do not match the source body")?;
                chunks.push((row.try_get("heading")?, body, offset, vector));
            }
            snapshots.push(Snapshot { module: module.clone(), id: id.clone(), source, stored, chunks });
        }
        Ok(snapshots)
    }.await;
    input.close().await;
    outcome
}

async fn seed_old_vectors(pool: &SqlitePool, snapshots: &[Snapshot]) -> TestResult {
    let mut tx = pool.begin().await?;
    for snapshot in snapshots {
        let doc = &snapshot.stored;
        sqlx::query("INSERT OR IGNORE INTO sources(module_id,source_id,state) VALUES(?,?,'ready')")
            .bind(&snapshot.module)
            .bind(&snapshot.source.id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO documents(module_id,id,source_id,url,title,body,links,content_hash,etag,modified,retrieved_at,model_revision,content_use) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?)")
            .bind(&snapshot.module).bind(&snapshot.id).bind(&snapshot.source.id).bind(&doc.url)
            .bind(&doc.content.title).bind(&doc.content.body).bind(serde_json::to_string(&doc.content.links)?)
            .bind(&doc.content_hash).bind(&doc.etag).bind(&doc.modified).bind(doc.retrieved_at as i64)
            .bind(&doc.model_revision).bind(doc.content_use.as_str()).execute(&mut *tx).await?;
        for (heading, body, offset, vector) in &snapshot.chunks {
            let result = sqlx::query("INSERT INTO chunks(module_id,document_id,heading,body,offset_bytes,vector) VALUES(?,?,?,?,?,?)")
                .bind(&snapshot.module).bind(&snapshot.id).bind(heading).bind(body).bind(offset).bind(vector)
                .execute(&mut *tx).await?;
            sqlx::query(
                "INSERT INTO chunk_fts(rowid,title,heading,body,module_id) VALUES(?,?,?,?,?)",
            )
            .bind(result.last_insert_rowid())
            .bind(&doc.content.title)
            .bind(heading)
            .bind(body)
            .bind(&snapshot.module)
            .execute(&mut *tx)
            .await?;
        }
    }
    tx.commit().await?;
    Ok(())
}

async fn stored_fingerprint(pool: &SqlitePool) -> TestResult<String> {
    let rows = sqlx::query("SELECT d.module_id,d.id,d.body,d.model_revision,c.heading,c.body AS chunk_body,c.offset_bytes,c.vector FROM documents d JOIN chunks c ON c.module_id=d.module_id AND c.document_id=d.id ORDER BY d.module_id,d.id,c.offset_bytes")
        .fetch_all(pool).await?;
    let mut values = Vec::new();
    for row in rows {
        values.push(serde_json::json!([
            row.try_get::<String, _>("module_id")?,
            row.try_get::<String, _>("id")?,
            extract::digest(row.try_get::<String, _>("body")?.as_bytes()),
            row.try_get::<String, _>("model_revision")?,
            row.try_get::<String, _>("heading")?,
            row.try_get::<String, _>("chunk_body")?,
            row.try_get::<i64, _>("offset_bytes")?,
            row.try_get::<Vec<u8>, _>("vector")?
        ]));
    }
    Ok(extract::digest(&serde_json::to_vec(&values)?))
}

async fn rebuild(library: &KnowledgeLibrary, snapshots: &[Snapshot]) -> TestResult {
    seed_old_vectors(&library.pool, snapshots).await?;
    for module in ["palworld", "barotrauma", "necesse", "terraria"] {
        require(
            matches!(
                library.search(module, "server configuration", 0).await,
                Err(KnowledgeError::Unavailable(_))
            ),
            "Old-revision vectors were admitted by search",
        )?;
    }
    let before = stored_fingerprint(&library.pool).await?;
    let encoder = library.encoder().await?;
    let cancelled = Arc::clone(&encoder);
    let doc = snapshots[0].stored.clone();
    let result = tokio::task::spawn_blocking(move || {
        indexing::prepare(
            &doc.content.title,
            &doc.content.body,
            &cancelled,
            &AtomicBool::new(true),
            Instant::now() + Duration::from_secs(180),
        )
    })
    .await?;
    require(
        matches!(result, Err(KnowledgeError::Cancelled)),
        "Cancelled preparation was not rejected",
    )?;
    require(
        stored_fingerprint(&library.pool).await? == before,
        "Cancelled preparation changed the retained snapshot",
    )?;

    let mut prepared =
        BTreeMap::<(String, String), (sources::Source, Vec<store::PreparedDocument>)>::new();
    for snapshot in snapshots {
        let encoder = Arc::clone(&encoder);
        let mut stored = snapshot.stored.clone();
        let document = tokio::task::spawn_blocking(move || {
            let chunks = indexing::prepare(
                &stored.content.title,
                &stored.content.body,
                &encoder,
                &AtomicBool::new(false),
                Instant::now() + Duration::from_secs(180),
            )?;
            let mut covered = 0;
            for chunk in &chunks {
                let end = chunk.offset.saturating_add(chunk.body.len());
                if chunk.offset > covered
                    || stored.content.body.as_bytes().get(chunk.offset..end)
                        != Some(chunk.body.as_bytes())
                    || chunk.vector.len() != embedding::DIMENSIONS
                    || chunk.vector.iter().any(|value| !value.is_finite())
                    || (chunk.vector.iter().map(|value| value * value).sum::<f32>() - 1.0).abs()
                        > 0.001
                {
                    return Err(KnowledgeError::Model(
                        "Rebuilt passage offsets/vector are invalid".into(),
                    ));
                }
                // Production chunks overlap for context. Check exact source
                // slices and the union of covered ranges instead of concatenating.
                covered = covered.max(end);
            }
            if covered != stored.content.body.len() {
                return Err(KnowledgeError::Model("Reindexing lost source bytes".into()));
            }
            stored.model_revision = embedding::REVISION.into();
            Ok(store::PreparedDocument {
                stored,
                chunks: Some(chunks),
            })
        })
        .await??;
        prepared
            .entry((snapshot.module.clone(), snapshot.source.id.clone()))
            .or_insert_with(|| (snapshot.source.clone(), Vec::new()))
            .1
            .push(document);
    }
    for ((module, _), (source, documents)) in prepared {
        let count = documents.len();
        require(
            store::publish(
                &library.pool,
                &module,
                &source,
                documents,
                "offline-public-snapshot-reindex",
            )
            .await?
                == count,
            "Rebuilt documents were not all published",
        )?;
    }
    let current: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM documents WHERE model_revision=?")
        .bind(embedding::REVISION)
        .fetch_one(&library.pool)
        .await?;
    require(
        current as usize == snapshots.len(),
        "Reindex left mixed model revisions",
    )?;
    for snapshot in snapshots {
        let vectors: Vec<Vec<u8>> = sqlx::query_scalar(
            "SELECT vector FROM chunks WHERE module_id=? AND document_id=? ORDER BY offset_bytes",
        )
        .bind(&snapshot.module)
        .bind(&snapshot.id)
        .fetch_all(&library.pool)
        .await?;
        let old: Vec<_> = snapshot
            .chunks
            .iter()
            .map(|chunk| chunk.3.clone())
            .collect();
        require(vectors != old, "Reindex merely relabeled the old vectors")?;
    }
    Ok(())
}

async fn check_reopened(
    library: &KnowledgeLibrary,
    snapshots: &[Snapshot],
) -> TestResult<Vec<serde_json::Value>> {
    let mut reports = Vec::new();
    for (module, query, id, terms) in [
        (
            "palworld",
            "我只想让朋友进来玩，怎么给服务器设置密码和人数上限？",
            SELECTED[0].1,
            &["ServerPassword", "ServerPlayerMaxNum"][..],
        ),
        (
            "palworld",
            "set server password maximum players",
            SELECTED[0].1,
            &["ServerPassword", "ServerPlayerMaxNum"][..],
        ),
        (
            "barotrauma",
            "按中文开服教程，游戏通信和 Steam 查询分别使用什么默认端口和协议？",
            SELECTED[2].1,
            &["27015", "27016", "UDP"][..],
        ),
        (
            "necesse",
            "按中文教程，Windows 防火墙和路由器需要放行哪个端口和协议？",
            SELECTED[4].1,
            &["14159", "UDP"][..],
        ),
    ] {
        let page = library.search(module, query, 0).await?;
        require(
            page.model == embedding::MODEL_ID,
            "Search reported a different model",
        )?;
        let entry = page
            .entries
            .iter()
            .take(3)
            .find(|entry| entry.id == id)
            .ok_or_else(|| {
                std::io::Error::other(format!(
                    "{module}: relevant source missing from top three; {page:?}"
                ))
            })?;
        let expected = snapshots
            .iter()
            .find(|snapshot| snapshot.module == module && snapshot.id == id)
            .unwrap();
        let read = library.read(module, id, entry.offset_bytes).await?;
        let end = read.next_offset_bytes.unwrap_or(read.total_bytes);
        require(
            expected
                .stored
                .content
                .body
                .as_bytes()
                .get(entry.offset_bytes..end)
                == Some(read.body.as_bytes()),
            "Reopened body differs from the exact source bytes",
        )?;
        require(
            read.body.starts_with(&entry.snippet),
            "Search snippet and reopened read disagree",
        )?;
        require(
            terms.iter().all(|term| read.body.contains(term)),
            format!("{module}: actual returned evidence lacks required facts"),
        )?;
        require(
            read.source.url == expected.source.citation_url(&expected.stored.url)
                && read.source.content_sha256 == expected.stored.content_hash
                && read.source.retrieved_at == expected.stored.retrieved_at
                && read.source.content_use == ContentUse::Full
                && read.source.source_state == "ready",
            "Reopened citation/policy changed",
        )?;
        reports.push(serde_json::json!({"module":module,"query":query,"requiredFacts":terms,"readOffset":read.offset_bytes,"readBytes":read.body.len(),"result":page}));
    }
    let page = library
        .search(
            "terraria",
            "dedicated server default connection port protocol",
            0,
        )
        .await?;
    require(
        !page.entries.is_empty(),
        "Reference source produced no search evidence",
    )?;
    for entry in &page.entries {
        require(
            entry.source.content_use == ContentUse::Reference
                && entry.snippet.chars().count() <= 400,
            "Reference search exposed more than a short excerpt",
        )?;
        for offset in [0, entry.offset_bytes] {
            require(
                matches!(
                    library.read("terraria", &entry.id, offset).await,
                    Err(KnowledgeError::Policy(_))
                ),
                "Reference document was delivered as full text",
            )?;
        }
    }
    reports
        .push(serde_json::json!({"module":"terraria","referenceReadRejected":true,"result":page}));
    Ok(reports)
}

#[tokio::test]
#[ignore = "Requires explicit frozen LANGAME_KNOWLEDGE_SNAPSHOT_DB, new LANGAME_KNOWLEDGE_REINDEX_ROOT and local LANGAME_EMBEDDING_MODEL_DIR; no network"]
async fn offline_public_snapshot_reindexes_and_reopens_with_real_multilingual_retrieval()
-> TestResult {
    let all = match std::env::var("LANGAME_KNOWLEDGE_REINDEX_ALL") {
        Ok(value) if value == "1" => true,
        Ok(value) if value == "0" => false,
        Err(std::env::VarError::NotPresent) => false,
        _ => {
            return Err(
                std::io::Error::other("LANGAME_KNOWLEDGE_REINDEX_ALL must be 0 or 1").into(),
            );
        }
    };
    let repository = std::fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))?;
    let modules = repository.join("modules");
    let (database, root) = prepare_paths(&repository)?;
    let input_hash = extract::digest(&std::fs::read(&database)?);
    let snapshots = read_snapshot(&database, &modules, all).await?;
    let library = KnowledgeLibrary::open(&root, &modules).await?;
    let result = rebuild(&library, &snapshots).await;
    library.close().await;
    result?;
    let reopened = KnowledgeLibrary::open(&root, &modules).await?;
    let result = check_reopened(&reopened, &snapshots).await;
    let chunk_count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM chunks")
        .fetch_one(&reopened.pool)
        .await;
    reopened.close().await;
    let reports = result?;
    let chunk_count = chunk_count?;
    let mut module_ids: Vec<_> = snapshots
        .iter()
        .map(|snapshot| snapshot.module.as_str())
        .collect();
    module_ids.sort_unstable();
    module_ids.dedup();
    require(
        extract::digest(&std::fs::read(&database)?) == input_hash,
        "Frozen input DB changed during acceptance",
    )?;
    std::fs::write(
        root.join("integration-report.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "inputSha256":input_hash,"documents":snapshots.len(),"model":embedding::MODEL_ID,
            "allDocuments":all,"modules":module_ids,"moduleCount":module_ids.len(),"chunkCount":chunk_count,
            "revision":embedding::REVISION,"oldVectorsRejected":true,"cancelledPreparationPreservedSnapshot":true,
            "reopened":true,"queries":reports
        }))?,
    )?;
    Ok(())
}
