use crate::{
    KnowledgeError, KnowledgeLibrary, KnowledgeSettings, embedding, extract, sources, store,
};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

struct Fixture {
    root: PathBuf,
    library: KnowledgeLibrary,
    source: sources::Source,
}

impl Fixture {
    async fn new() -> Self {
        let root = std::env::temp_dir().join(format!("langame-knowledge-{}", uuid::Uuid::new_v4()));
        let modules = root.join("modules");
        std::fs::create_dir_all(modules.join("minecraft")).unwrap();
        let source = sources::Source {
            id: "publisher".into(),
            title: "Publisher manual".into(),
            authority: "Publisher".into(),
            kind: "official".into(),
            seeds: vec!["https://example.com/docs/start".into()],
            allowed_prefixes: vec!["https://example.com/docs/".into()],
            discover_links: true,
            reference_only: false,
            max_pages: 32,
            content_selector: None,
            discovery_selector: None,
            sitemaps: vec![],
            authority_evidence: "https://example.com/about".into(),
            license_note: "Public reference documents".into(),
            license_url: None,
            reviewed_on: "2026-09-28".into(),
        };
        let catalog = sources::Catalog {
            schema_version: 1,
            module_id: "minecraft".into(),
            scope: "Server reference".into(),
            gaps: vec![],
            sources: vec![source.clone()],
        };
        std::fs::write(
            modules.join("minecraft/knowledge-sources.toml"),
            toml::to_string(&catalog).unwrap(),
        )
        .unwrap();
        let library = KnowledgeLibrary::open(&root.join("knowledge"), &modules)
            .await
            .unwrap();
        Self {
            root,
            library,
            source,
        }
    }

    async fn publish(&self, body: &str) -> String {
        let doc = document(body, "start");
        let id = extract::document_id("publisher", &doc.stored.url);
        store::publish(
            &self.library.pool,
            "minecraft",
            &self.source,
            vec![doc],
            "reviewed",
        )
        .await
        .unwrap();
        id
    }

    async fn finish(self) {
        self.library.close().await;
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}

fn document(body: &str, page: &str) -> store::PreparedDocument {
    let title = "Official server setup";
    let mut chunks = extract::chunks(title, body);
    for chunk in &mut chunks {
        chunk.vector = vec![0.0; embedding::DIMENSIONS];
        chunk.vector[0] = 1.0;
    }
    store::PreparedDocument {
        stored: store::StoredDocument {
            content_use: crate::ContentUse::Full,
            url: format!("https://example.com/docs/{page}"),
            content: extract::Extracted {
                title: title.into(),
                body: body.into(),
                links: vec![],
            },
            content_hash: extract::digest(body.as_bytes()),
            etag: Some("version-1".into()),
            modified: None,
            retrieved_at: 1234,
            model_revision: embedding::REVISION.into(),
        },
        chunks: Some(chunks),
    }
}

#[tokio::test]
async fn source_snapshot_persists_settings_body_vectors_and_citations_after_reopen() {
    let fixture = Fixture::new().await;
    let body = "# Dedicated server\n\nSet server-port=25565 and enable the whitelist. ".repeat(200);
    let id = fixture.publish(&body).await;
    fixture
        .library
        .update_settings(KnowledgeSettings {
            auto_update: false,
            interval_hours: 48,
        })
        .await
        .unwrap();
    fixture.library.close().await;
    let reopened = KnowledgeLibrary::open(
        &fixture.root.join("knowledge"),
        &fixture.root.join("modules"),
    )
    .await
    .unwrap();
    let settings = reopened.settings().await.unwrap();
    assert!(!settings.auto_update);
    assert_eq!(settings.interval_hours, 48);
    let status = reopened.status().await.unwrap();
    assert_eq!(status.games[0].sources[0].document_count, 1);
    assert!(status.games[0].sources[0].chunk_count > 2);
    assert_eq!(status.games[0].sources[0].state, "ready");
    let mut restored = String::new();
    let mut offset = 0;
    loop {
        let page = reopened.read("minecraft", &id, offset).await.unwrap();
        assert_eq!(page.source.url, "https://example.com/docs/start");
        assert_eq!(page.source.retrieved_at, 1234);
        assert_eq!(page.source.content_sha256, extract::digest(body.as_bytes()));
        assert!(page.body.len() <= 8192);
        restored.push_str(&page.body);
        match page.next_offset_bytes {
            Some(next) => offset = next,
            None => break,
        }
    }
    assert_eq!(restored, body);
    reopened.close().await;
    fixture.finish().await;
}

#[tokio::test]
async fn failed_publish_keeps_last_usable_body_vectors_and_fts() {
    let fixture = Fixture::new().await;
    let id = fixture
        .publish("Original configuration: whitelist is enabled and server-port is 25565.")
        .await;
    let mut corrupt = document("Corrupted new document", "start");
    corrupt.chunks.as_mut().unwrap()[0].vector = vec![f32::NAN; embedding::DIMENSIONS];
    assert!(
        store::publish(
            &fixture.library.pool,
            "minecraft",
            &fixture.source,
            vec![corrupt],
            "reviewed"
        )
        .await
        .is_err()
    );
    store::source_state(
        &fixture.library.pool,
        "minecraft",
        "publisher",
        "error",
        Some("Publisher timeout"),
    )
    .await
    .unwrap();
    let page = fixture.library.read("minecraft", &id, 0).await.unwrap();
    assert!(page.body.starts_with("Original configuration"));
    assert_eq!(page.source.content_use, crate::ContentUse::Full);
    assert_eq!(page.source.source_state, "error");
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM chunk_fts WHERE chunk_fts MATCH 'whitelist'")
            .fetch_one(&fixture.library.pool)
            .await
            .unwrap();
    assert_eq!(count, 1);
    fixture.finish().await;
}

#[tokio::test]
async fn reference_policy_survives_reopen_and_prevents_every_fulltext_offset() {
    let fixture = Fixture::new().await;
    let body = "Publisher configuration reference. ".repeat(500);
    let mut doc = document(&body, "start");
    doc.stored.content_use = crate::ContentUse::Reference;
    let id = extract::document_id("publisher", &doc.stored.url);
    store::publish(
        &fixture.library.pool,
        "minecraft",
        &fixture.source,
        vec![doc],
        "reviewed",
    )
    .await
    .unwrap();
    fixture.library.close().await;
    let reopened = KnowledgeLibrary::open(
        &fixture.root.join("knowledge"),
        &fixture.root.join("modules"),
    )
    .await
    .unwrap();
    let cached = store::cached(&reopened.pool, "minecraft", "publisher")
        .await
        .unwrap();
    assert_eq!(cached["https://example.com/docs/start"].content.body, body);
    assert_eq!(
        cached["https://example.com/docs/start"].content_use,
        crate::ContentUse::Reference
    );
    for offset in [0, 40, 400, 8192] {
        let error = reopened.read("minecraft", &id, offset).await.unwrap_err();
        assert!(
            matches!(error, KnowledgeError::Policy(ref reason) if reason.contains("https://example.com/docs/start")),
            "{error}"
        );
        assert!(!error.to_string().contains("Publisher configuration"));
    }
    reopened.close().await;
    fixture.finish().await;
}

#[tokio::test]
async fn observed_reference_policy_limits_the_whole_retained_source_when_refresh_fails() {
    let fixture = Fixture::new().await;
    let first = document("First existing body", "start");
    let second = document("Second existing body", "later");
    let ids = [&first, &second].map(|doc| extract::document_id("publisher", &doc.stored.url));
    store::publish(
        &fixture.library.pool,
        "minecraft",
        &fixture.source,
        vec![first, second],
        "reviewed",
    )
    .await
    .unwrap();
    // First current response exposes a tighter policy, then another page fails.
    store::restrict_source_use(
        &fixture.library.pool,
        "minecraft",
        "publisher",
        crate::ContentUse::Reference,
    )
    .await
    .unwrap();
    store::source_state(
        &fixture.library.pool,
        "minecraft",
        "publisher",
        "error",
        Some("Later page timed out"),
    )
    .await
    .unwrap();
    let cached = store::cached(&fixture.library.pool, "minecraft", "publisher")
        .await
        .unwrap();
    assert_eq!(cached.len(), 2);
    assert!(
        cached
            .values()
            .all(|doc| doc.content_use == crate::ContentUse::Reference)
    );
    assert_eq!(
        cached["https://example.com/docs/later"].content.body,
        "Second existing body"
    );
    for id in ids {
        assert!(matches!(
            fixture.library.read("minecraft", &id, 0).await,
            Err(KnowledgeError::Policy(_))
        ));
    }
    fixture.finish().await;
}

#[tokio::test]
async fn pre_policy_database_upgrades_without_losing_body_vectors_or_settings() {
    let fixture = Fixture::new().await;
    let id = fixture
        .publish("Existing publisher server configuration remains intact.")
        .await;
    fixture
        .library
        .update_settings(KnowledgeSettings {
            auto_update: false,
            interval_hours: 72,
        })
        .await
        .unwrap();
    fixture.library.close().await;
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(fixture.root.join("knowledge/library.sqlite3")),
        )
        .await
        .unwrap();
    sqlx::query("ALTER TABLE documents DROP COLUMN content_use")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    let reopened = KnowledgeLibrary::open(
        &fixture.root.join("knowledge"),
        &fixture.root.join("modules"),
    )
    .await
    .unwrap();
    assert_eq!(reopened.settings().await.unwrap().interval_hours, 72);
    let cached = store::cached(&reopened.pool, "minecraft", "publisher")
        .await
        .unwrap();
    assert_eq!(
        cached["https://example.com/docs/start"].content_use,
        crate::ContentUse::Reference
    );
    assert_eq!(
        cached["https://example.com/docs/start"].content.body,
        "Existing publisher server configuration remains intact."
    );
    assert!(matches!(
        reopened.read("minecraft", &id, 0).await,
        Err(KnowledgeError::Policy(_))
    ));
    let vectors: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chunks")
        .fetch_one(&reopened.pool)
        .await
        .unwrap();
    let indexed: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chunk_fts")
        .fetch_one(&reopened.pool)
        .await
        .unwrap();
    assert_eq!(vectors, 1);
    assert_eq!(indexed, 1);
    reopened.close().await;
    fixture.finish().await;
}

#[tokio::test]
async fn publisher_restriction_hides_retained_evidence_until_a_successful_refresh() {
    let fixture = Fixture::new().await;
    let id = fixture
        .publish("Previously permitted server instructions.")
        .await;
    store::source_state(
        &fixture.library.pool,
        "minecraft",
        "publisher",
        "restricted",
        Some("Publisher disallows ingestion"),
    )
    .await
    .unwrap();
    assert!(matches!(
        fixture.library.read("minecraft", &id, 0).await,
        Err(KnowledgeError::Policy(_))
    ));
    store::source_state(
        &fixture.library.pool,
        "minecraft",
        "publisher",
        "error",
        Some("Temporary DNS failure"),
    )
    .await
    .unwrap();
    assert!(matches!(
        fixture.library.read("minecraft", &id, 0).await,
        Err(KnowledgeError::Policy(_))
    ));
    let error = fixture
        .library
        .search("minecraft", "server", 0)
        .await
        .unwrap_err();
    assert!(
        matches!(error, KnowledgeError::Unavailable(_)),
        "Restricted vectors must be excluded before loading any model"
    );
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM documents")
        .fetch_one(&fixture.library.pool)
        .await
        .unwrap();
    assert_eq!(
        count, 1,
        "Policy changes must not silently delete retained data"
    );
    fixture
        .publish("Publisher permits these refreshed server instructions.")
        .await;
    assert!(fixture.library.read("minecraft", &id, 0).await.is_ok());
    fixture.finish().await;
}

#[tokio::test]
async fn complete_update_retires_removed_pages_without_leaving_fts_or_vectors() {
    let fixture = Fixture::new().await;
    let old = document("Obsolete old configuration with removed setting.", "old");
    let old_id = extract::document_id("publisher", &old.stored.url);
    store::publish(
        &fixture.library.pool,
        "minecraft",
        &fixture.source,
        vec![old, document("Current server configuration", "start")],
        "reviewed",
    )
    .await
    .unwrap();
    fixture
        .publish("Current server configuration after refresh")
        .await;
    assert!(fixture.library.read("minecraft", &old_id, 0).await.is_err());
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM chunk_fts WHERE chunk_fts MATCH 'Obsolete'")
            .fetch_one(&fixture.library.pool)
            .await
            .unwrap();
    assert_eq!(count, 0);
    fixture.finish().await;
}

#[tokio::test]
async fn unchanged_body_reuses_vectors_and_refreshes_provenance() {
    let fixture = Fixture::new().await;
    let id = fixture
        .publish("Server settings and ports are documented here.")
        .await;
    let mut docs = store::cached(&fixture.library.pool, "minecraft", "publisher")
        .await
        .unwrap();
    let mut stored = docs.remove("https://example.com/docs/start").unwrap();
    stored.retrieved_at = 2345;
    let changed = store::publish(
        &fixture.library.pool,
        "minecraft",
        &fixture.source,
        vec![store::PreparedDocument {
            stored,
            chunks: None,
        }],
        "reviewed",
    )
    .await
    .unwrap();
    assert_eq!(changed, 0);
    let page = fixture.library.read("minecraft", &id, 0).await.unwrap();
    assert_eq!(page.source.retrieved_at, 2345);
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chunks")
        .fetch_one(&fixture.library.pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
    fixture.finish().await;
}

#[tokio::test]
async fn document_reads_enforce_game_scope_and_unicode_page_boundaries() {
    let fixture = Fixture::new().await;
    let body = "配置白名单与服务器密码🔒\n".repeat(700);
    let id = fixture.publish(&body).await;
    let page = fixture.library.read("minecraft", &id, 0).await.unwrap();
    assert!(body.is_char_boundary(page.next_offset_bytes.unwrap()));
    assert!(fixture.library.read("minecraft", &id, 1).await.is_err());
    assert!(fixture.library.read("../minecraft", &id, 0).await.is_err());
    assert!(
        fixture
            .library
            .read("minecraft", "https://attacker.invalid", 0)
            .await
            .is_err()
    );
    assert!(fixture.library.read("palworld", &id, 0).await.is_err());
    assert!(
        fixture
            .library
            .update_settings(KnowledgeSettings {
                auto_update: true,
                interval_hours: 0
            })
            .await
            .is_err()
    );
    fixture.finish().await;
}

#[tokio::test]
async fn escaped_document_text_is_paged_before_json_transport_overflows() {
    let fixture = Fixture::new().await;
    let body = "\"\\\"\n".repeat(5000);
    let id = fixture.publish(&body).await;
    let mut recovered = String::new();
    let mut offset = 0;
    loop {
        let page = fixture
            .library
            .read("minecraft", &id, offset)
            .await
            .unwrap();
        assert!(serde_json::to_vec(&page).unwrap().len() <= 11 * 1024);
        recovered.push_str(&page.body);
        match page.next_offset_bytes {
            Some(next) => offset = next,
            None => break,
        }
    }
    assert_eq!(recovered, body);
    fixture.finish().await;
}

#[test]
fn cancellation_and_vector_validation_are_explicit() {
    assert!(matches!(
        crate::check_cancel(&AtomicBool::new(true)),
        Err(KnowledgeError::Cancelled)
    ));
    assert!(crate::search::cosine(&[1.0], &[0, 0, 0, 0]).is_err());
    let vector = vec![0.0; embedding::DIMENSIONS];
    let invalid: Vec<u8> = (0..embedding::DIMENSIONS)
        .flat_map(|_| f32::NAN.to_le_bytes())
        .collect();
    assert!(crate::search::cosine(&vector, &invalid).is_err());
    assert_eq!(
        crate::search::lexical_query("server-port OR \"*\""),
        "\"server\" OR \"port\" OR \"OR\""
    );
}
