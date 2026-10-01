use super::{citation_url, parse, validate_source_module};
use crate::{ContentUse, KnowledgeError, KnowledgeLibrary, embedding, extract, sources, store};
use reqwest::Url;
use serde_json::{Value, json};

struct ReviewedArticle {
    module_id: &'static str,
    source_id: &'static str,
    api: &'static str,
    response_url: &'static str,
    citation: &'static str,
    id: u64,
    title: &'static str,
}

const ARTICLES: &[ReviewedArticle] = &[
    ReviewedArticle {
        module_id: "dontstarve",
        source_id: "klei-command-line",
        api: "https://support.klei.com/api/v2/help_center/en-us/articles/360029556192.json",
        response_url: "https://support.klei.com/hc/en-us/articles/360029556192-Dedicated-Server-Command-Line-Options-Guide",
        citation: "https://support.klei.com/hc/en-us/articles/360029556192",
        id: 360029556192,
        title: "Dedicated Server Command Line Options Guide",
    },
    ReviewedArticle {
        module_id: "sevendaystodie",
        source_id: "fun-pimps-server-migration",
        api: "https://7-days-to-die.zendesk.com/api/v2/help_center/en-us/articles/50318172509972.json",
        response_url: "https://7-days-to-die.zendesk.com/hc/en-us/articles/50318172509972-V3-0-Dead-Hot-Summer-Release-Note",
        citation: "https://7-days-to-die.zendesk.com/hc/en-us/articles/50318172509972",
        id: 50318172509972,
        title: "V3.0 Dead Hot Summer Release Note",
    },
    ReviewedArticle {
        module_id: "minecraft",
        source_id: "mojang-java-help",
        api: "https://help.minecraft.net/help_center/en-us/articles/360058525452",
        response_url: "https://minecrafthelp.zendesk.com/hc/en-us/articles/360058525452-How-to-Setup-a-Minecraft-Java-Edition-Server",
        citation: "https://help.minecraft.net/hc/en-us/articles/360058525452",
        id: 360058525452,
        title: "How to Setup a Minecraft: Java Edition Server",
    },
];

fn source(article: &ReviewedArticle) -> sources::Source {
    let mut source = super::tests::source();
    source.id = article.source_id.into();
    source.seeds = vec![article.api.into()];
    source.authority_evidence = article.citation.into();
    source
}

fn response(article: &ReviewedArticle) -> Value {
    json!({"article": {
        "id": article.id,
        "locale": "en-us",
        "draft": false,
        "title": article.title,
        "body": "<h2>Dedicated server</h2><p>Reviewed public instructions with version-specific settings.</p>",
        "html_url": article.response_url,
        "user_segment_id": null,
        "user_segment_ids": []
    }})
}

fn decode(
    article: &ReviewedArticle,
    source: &sources::Source,
    value: &Value,
) -> super::Result<super::PublicArticle> {
    parse(
        source,
        &Url::parse(article.api).unwrap(),
        "application/json",
        &serde_json::to_vec(value).unwrap(),
    )
}

#[test]
fn reviewed_public_articles_extract_body_and_keep_the_reader_on_the_official_site() {
    for article in ARTICLES {
        let source = source(article);
        validate_source_module(article.module_id, &source).unwrap();
        let mut value = response(article);
        // Klei's actual public response omits the plural field.
        if article.module_id == "dontstarve" {
            value["article"]
                .as_object_mut()
                .unwrap()
                .remove("user_segment_ids");
        }
        let decoded = decode(article, &source, &value).unwrap();
        assert_eq!(decoded.title, article.title);
        assert_eq!(
            decoded.canonical_url.host_str(),
            Url::parse(article.citation).unwrap().host_str()
        );
        assert_eq!(
            citation_url(&source, &Url::parse(article.api).unwrap())
                .unwrap()
                .as_str(),
            article.citation
        );
        let extracted = extract::extract(
            &source,
            &Url::parse(article.api).unwrap(),
            "application/json",
            &serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        assert!(extracted.body.contains("version-specific settings"));
        assert!(extracted.links.is_empty());
    }
}

#[test]
fn publisher_source_module_and_exact_article_scope_cannot_be_rebound() {
    for article in ARTICLES {
        let original = source(article);
        assert!(validate_source_module("other-game", &original).is_err());
        for mutate in 0..5 {
            let mut changed = original.clone();
            match mutate {
                0 => changed.id = "unreviewed-source".into(),
                1 => changed.kind = "community".into(),
                2 => changed.reference_only = true,
                3 => {
                    changed.seeds = vec![
                        article
                            .api
                            .replace(&article.id.to_string(), &(article.id + 1).to_string()),
                    ]
                }
                _ => changed.seeds = vec![article.api.replace("en-us", "de")],
            }
            assert!(!super::is_article_url(
                &changed,
                &Url::parse(&changed.seeds[0]).unwrap()
            ));
            if mutate != 0 {
                assert!(validate_source_module(article.module_id, &changed).is_err());
            }
        }
        for extra_scope in ["prefix", "sitemap", "discovery"] {
            let mut changed = original.clone();
            match extra_scope {
                "prefix" => changed.allowed_prefixes = vec!["https://example.com/manual/".into()],
                "sitemap" => changed.sitemaps = vec!["https://example.com/sitemap.xml".into()],
                _ => changed.discover_links = true,
            }
            assert!(validate_source_module(article.module_id, &changed).is_err());
        }
        for url in [
            format!("{}?include=drafts", article.api),
            format!("{}#fragment", article.api),
            article.api.replacen("https://", "https://other.", 1),
        ] {
            let mut changed = original.clone();
            changed.seeds = vec![url.clone()];
            assert!(!super::is_article_url(&changed, &Url::parse(&url).unwrap()));
        }
    }
}

#[test]
fn new_publishers_preserve_public_visibility_identity_and_body_policy_checks() {
    for article in ARTICLES {
        let source = source(article);
        for patch in [
            json!({"draft": true}),
            json!({"user_segment_id": 42}),
            json!({"user_segment_ids": [42]}),
        ] {
            let mut value = response(article);
            value["article"]
                .as_object_mut()
                .unwrap()
                .extend(patch.as_object().unwrap().clone());
            assert!(matches!(
                decode(article, &source, &value),
                Err(KnowledgeError::Policy(_))
            ));
        }
        for patch in [
            json!({"id": article.id + 1}),
            json!({"locale": "de"}),
            json!({"html_url": article.response_url.replacen("https://", "https://other.", 1)}),
        ] {
            let mut value = response(article);
            value["article"]
                .as_object_mut()
                .unwrap()
                .extend(patch.as_object().unwrap().clone());
            assert!(decode(article, &source, &value).is_err());
        }
        let mut value = response(article);
        value["article"]["body"] = json!(
            "<meta name='robots' content='noarchive'><p>Restricted publisher instructions.</p>"
        );
        assert!(matches!(
            extract::extract(
                &source,
                &Url::parse(article.api).unwrap(),
                "application/json",
                &serde_json::to_vec(&value).unwrap()
            ),
            Err(KnowledgeError::Policy(_))
        ));
    }
}

#[tokio::test]
async fn restored_sources_persist_and_reopen_with_api_cache_keys_and_official_citations() {
    let root = std::env::temp_dir().join(format!(
        "langame-reviewed-articles-{}",
        uuid::Uuid::new_v4()
    ));
    let modules = root.join("modules");
    for article in ARTICLES {
        let directory = modules.join(article.module_id);
        std::fs::create_dir_all(&directory).unwrap();
        let catalog = sources::Catalog {
            schema_version: 1,
            module_id: article.module_id.into(),
            scope: "Dedicated server documentation".into(),
            gaps: Vec::new(),
            sources: vec![source(article)],
        };
        std::fs::write(
            directory.join("knowledge-sources.toml"),
            toml::to_string(&catalog).unwrap(),
        )
        .unwrap();
    }
    let library = KnowledgeLibrary::open(&root.join("knowledge"), &modules)
        .await
        .unwrap();
    for article in ARTICLES {
        let source = source(article);
        let content = extract::extract(
            &source,
            &Url::parse(article.api).unwrap(),
            "application/json",
            &serde_json::to_vec(&response(article)).unwrap(),
        )
        .unwrap();
        let mut chunks = extract::chunks(&content.title, &content.body);
        // Replace only the numerical model; scope admission and persistence stay real.
        for chunk in &mut chunks {
            chunk.vector = vec![0.0; embedding::DIMENSIONS];
            chunk.vector[0] = 1.0;
        }
        let content_hash = extract::digest(content.body.as_bytes());
        store::publish(
            &library.pool,
            article.module_id,
            &source,
            vec![store::PreparedDocument {
                stored: store::StoredDocument {
                    url: article.api.into(),
                    content,
                    content_hash,
                    content_use: ContentUse::Full,
                    etag: None,
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
    }
    library.close().await;
    let reopened = KnowledgeLibrary::open(&root.join("knowledge"), &modules)
        .await
        .unwrap();
    for article in ARTICLES {
        let cached = store::cached(&reopened.pool, article.module_id, article.source_id)
            .await
            .unwrap();
        assert!(cached.contains_key(article.api));
        let id = extract::document_id(article.source_id, article.api);
        let page = reopened.read(article.module_id, &id, 0).await.unwrap();
        assert_eq!(page.source.url, article.citation);
        assert_eq!(page.title, article.title);
        assert!(page.body.contains("version-specific settings"));
    }
    reopened.close().await;
    std::fs::remove_dir_all(root).unwrap();
}
