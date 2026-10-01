use super::tests::{API, response, source};
use crate::{ContentUse, KnowledgeLibrary, embedding, extract, sources, store};

#[tokio::test]
async fn article_cache_keeps_api_etag_but_reopened_reads_and_status_cite_the_official_page() {
    let root = std::env::temp_dir().join(format!("langame-zendesk-{}", uuid::Uuid::new_v4()));
    let modules = root.join("modules");
    std::fs::create_dir_all(modules.join("enshrouded")).unwrap();
    let source = source();
    let catalog = sources::Catalog {
        schema_version: 1,
        module_id: "enshrouded".into(),
        scope: "Dedicated server documentation".into(),
        gaps: Vec::new(),
        sources: vec![source.clone()],
    };
    std::fs::write(
        modules.join("enshrouded/knowledge-sources.toml"),
        toml::to_string(&catalog).unwrap(),
    )
    .unwrap();
    let content = extract::extract(
        &source,
        &reqwest::Url::parse(API).unwrap(),
        "application/json",
        &serde_json::to_vec(&response()).unwrap(),
    )
    .unwrap();
    let expected_body = content.body.clone();
    let content_hash = extract::digest(content.body.as_bytes());
    let mut chunks = extract::chunks(&content.title, &content.body);
    // Only the numerical-model boundary is replaced. Publication, SQLite
    // persistence, scope admission and public read/citation paths stay real.
    for chunk in &mut chunks {
        chunk.vector = vec![0.0; embedding::DIMENSIONS];
        chunk.vector[0] = 1.0;
    }
    let library = KnowledgeLibrary::open(&root.join("knowledge"), &modules)
        .await
        .unwrap();
    store::publish(
        &library.pool,
        "enshrouded",
        &source,
        vec![store::PreparedDocument {
            stored: store::StoredDocument {
                url: API.into(),
                content,
                content_hash: content_hash.clone(),
                content_use: ContentUse::Full,
                etag: Some("W/\"publisher-revision\"".into()),
                modified: None,
                retrieved_at: 1234,
                model_revision: embedding::REVISION.into(),
            },
            chunks: Some(chunks),
        }],
        "reviewed",
    )
    .await
    .unwrap();
    library.close().await;

    let reopened = KnowledgeLibrary::open(&root.join("knowledge"), &modules)
        .await
        .unwrap();
    let cached = store::cached(&reopened.pool, "enshrouded", &source.id)
        .await
        .unwrap();
    assert_eq!(cached.len(), 1);
    assert_eq!(cached[API].url, API);
    assert_eq!(
        cached[API].etag.as_deref(),
        Some("W/\"publisher-revision\"")
    );
    assert_eq!(cached[API].content.body, expected_body);
    let official = "https://enshrouded.zendesk.com/hc/en-us/articles/16055441447709";
    assert!(!cached.contains_key(official));

    let id = extract::document_id(&source.id, API);
    let page = reopened.read("enshrouded", &id, 0).await.unwrap();
    assert_eq!(page.source.url, official);
    assert_eq!(page.source.content_sha256, content_hash);
    assert_eq!(page.source.retrieved_at, 1234);
    assert_eq!(page.title, "Dedicated Server Configuration");
    assert_eq!(page.body, expected_body);
    assert!(page.next_offset_bytes.is_none());
    let status = reopened.status().await.unwrap();
    assert_eq!(status.games[0].sources[0].url, official);
    assert_eq!(status.games[0].sources[0].document_count, 1);
    assert_eq!(status.games[0].sources[0].state, "ready");
    reopened.close().await;
    std::fs::remove_dir_all(root).unwrap();
}
