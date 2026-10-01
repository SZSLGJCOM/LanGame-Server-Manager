use super::*;
use std::future::ready;

#[test]
fn game_knowledge_transport_preserves_public_commands_without_exempting_private_reads() {
    let body = "Mojang Studios / Microsoft\nPATH=/usr/bin\nServerPassword=fixture-server-password\njava -Xmx1024M -jar server.jar nogui";
    let public = json!({"ok":true,"data":{"scope":"game_documentation","body":body,"offsetBytes":0,"nextOffsetBytes":body.len()}});
    for request in [
        AssistantReadTool::SearchGameDocs {
            query: "setup".into(),
            offset: 0,
        },
        AssistantReadTool::ReadGameDoc {
            document_id: "a".repeat(32),
            offset: 0,
        },
    ] {
        let delivered: Value =
            serde_json::from_str(&assistant_read_result_text(&request, &public)).unwrap();
        assert_eq!(delivered, public);
        let failure = json!({"ok":false,"error":"password=synthetic-fixture"});
        assert!(
            !assistant_read_result_text(&request, &failure)
                .contains("synthetic-fixture")
        );
        let oversized = json!({"ok":true,"data":{"body":"x".repeat(ASSISTANT_TOOL_RESULT_BYTES)}});
        let rejected: Value =
            serde_json::from_str(&assistant_read_result_text(&request, &oversized)).unwrap();
        assert_eq!(rejected["ok"], false);
    }
    let private = json!({"ok":true,"data":{"scope":"game_documentation","content":"password=synthetic-fixture"}});
    let delivered =
        assistant_read_result_text(&AssistantReadTool::ReadRuntime { lines: 1 }, &private);
    assert!(
        !delivered.contains("synthetic-fixture"),
        "A forged scope marker cannot bypass private-read redaction"
    );
}

struct KnowledgeFixture(PathBuf);

impl KnowledgeFixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("lgsm-game-docs-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("schema-fixture")).unwrap();
        std::fs::write(
            root.join("schema-fixture/knowledge.toml"),
            r#"
schema_version = 1
module_id = "schema-fixture"
[[documents]]
id = "setup"
title = "开服准备"
summary = "先确认版本，再备份存档。"
body = "Before updating, stop the server and preserve a backup. 开服前核对版本。"
keywords = ["开服", "setup", "backup"]
limitations = ["No running server was inspected."]
[documents.source]
url = "https://example.test/server/setup"
title = "Publisher dedicated server guide"
authority = "Fixture publisher"
kind = "official"
checkedOn = "2000-01-01"
verification = "source_reviewed"
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("schema-fixture/knowledge-sources.toml"),
            r#"
schema_version = 1
module_id = "schema-fixture"
scope = "Official server setup documentation"
gaps = []
[[sources]]
id = "publisher"
title = "Publisher dedicated server guide"
authority = "Fixture publisher"
kind = "official"
seeds = ["https://docs.example.com/server/setup"]
allowed_prefixes = []
discover_links = false
max_pages = 1
authority_evidence = "https://docs.example.com/server/setup"
license_note = "Controlled fixture text only."
reviewed_on = "2026-09-28"
"#,
        )
        .unwrap();
        Self(root)
    }
}

impl Drop for KnowledgeFixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("remove this test's knowledge fixture");
    }
}

#[test]
fn game_knowledge_scope_is_bound_by_the_application() {
    let module = module_settings_tests::schema_module(json!({"properties":{}}));
    let (_, mut instance) = task_tests::task_fixture(false);
    assert!(assistant_knowledge_module_id(None, None).is_err());
    assert_eq!(
        assistant_knowledge_module_id(None, Some(&module)).unwrap(),
        "schema-fixture"
    );
    assert!(assistant_knowledge_module_id(Some(&instance), Some(&module)).is_err());
    instance.summary.module_id = "schema-fixture".into();
    assert_eq!(
        assistant_knowledge_module_id(Some(&instance), None).unwrap(),
        "schema-fixture"
    );
    for extra in ["moduleId", "root", "url", "instanceId"] {
        for mut request in [
            json!({"tool":"search_game_docs", "query":"setup", "offset":0}),
            json!({"tool":"read_game_doc", "documentId":"0123456789abcdef0123456789abcdef", "offset":1024}),
        ] {
            request[extra] = json!("another-scope");
            assert!(serde_json::from_value::<AssistantReadTool>(request).is_err());
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn game_knowledge_missing_synchronized_cache_never_falls_back_to_bundled_summaries() {
    let fixture = KnowledgeFixture::new();
    let cache = fixture.0.join("isolated-cache");
    let library = app_knowledge::KnowledgeLibrary::open(&cache, &fixture.0)
        .await
        .unwrap();
    let search = assistant_game_knowledge_evidence(
        &library,
        "schema-fixture",
        AssistantReadTool::SearchGameDocs {
            query: "开服".into(),
            offset: 0,
        },
    )
    .await;
    let read = assistant_game_knowledge_evidence(
        &library,
        "schema-fixture",
        AssistantReadTool::ReadGameDoc {
            document_id: "0123456789abcdef0123456789abcdef".into(),
            offset: 1024,
        },
    )
    .await;
    library.close().await;
    let search_error = search.unwrap_err();
    assert!(
        search_error.contains("have not been synchronized"),
        "{search_error}"
    );
    assert!(
        read.unwrap_err()
            .contains("absent from this game's synchronized sources")
    );
    assert!(
        fixture.0.join("schema-fixture/knowledge.toml").exists(),
        "An authored summary is present but must not replace missing full-text evidence"
    );
    assert!(
        !cache.join("model").exists(),
        "A read-only evidence gap must not download a model"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn game_knowledge_protocol_keeps_citations_freshness_and_byte_offsets_without_mutations() {
    let module = module_settings_tests::schema_module(json!({"properties":{}}));
    let mut turns = 0;
    let mut reads = 0;
    let context = AssistantInvestigationContext {
        prompt: "查阅开服文档，不修改实例。".into(),
        initial_reads: vec![],
        tools: assistant_investigation_tools(None, Some(&module), false),
        draft: None,
        instance: None,
        module: Some(&module),
        completion: AssistantInvestigationCompletion::ReadOnlyAnswer,
    };
    let answer =
        "先停止服务器并保留备份。来源：https://example.test/server/setup；该资料需要重新核验。";
    let output = Box::pin(run_assistant_tool_investigation(context, |messages, tools| {
        turns += 1;
        assert!(tools.iter().all(|tool| tool.name != "propose_operation"));
        if turns > 1 {
            let expected_tool = if turns == 2 { "search_game_docs" } else { "read_game_doc" };
            let receipt = messages.iter().find_map(|message| match message {
                AssistantToolMessage::ToolResult { name, is_error: false, content, .. } if name == expected_tool => Some(content),
                _ => None,
            }).expect("The next provider turn must receive the actual native tool receipt");
            let value: Value = serde_json::from_str(receipt).unwrap();
            let data = &value["data"];
            let doc = data.get("entries").map_or(data, |entries| &entries[0]);
            assert_eq!(value["ok"], true);
            assert_eq!(data["instanceObserved"], false);
            assert_eq!(doc["citationId"], "0123456789abcdef0123456789abcdef:1024");
            assert_eq!(doc["offsetBytes"], 1024);
            assert_eq!(doc["source"]["sourceState"], "failed");
            assert_eq!(doc["source"]["retrievedAt"], 1_750_000_000_u64);
            if expected_tool == "read_game_doc" { assert_eq!(data["nextOffsetBytes"], 2048); }
        }
        ready(Ok(match turns {
            1 | 2 => {
                if turns == 2 {
                    assert!(messages.iter().any(|message| matches!(message,
                        AssistantToolMessage::ToolResult { is_error:false, content, .. }
                        if content.contains("0123456789abcdef0123456789abcdef:1024"))));
                }
                AssistantToolReply {
                    content: String::new(),
                    calls: vec![AssistantToolCall {
                        id: format!("docs-{turns}"),
                        name: if turns == 1 { "search_game_docs" } else { "read_game_doc" }.into(),
                        arguments: if turns == 1 { json!({"query":"开服", "offset":5}) } else { json!({"documentId":"0123456789abcdef0123456789abcdef", "offset":1024}) },
                    }],
                    raw_message: Value::Null,
                }
            }
            _ => {
                assert!(messages.iter().any(|message| matches!(message,
                    AssistantToolMessage::ToolResult { name, is_error:false, content, .. }
                    if name == "read_game_doc" && content.contains("Before updating") && content.contains("sourceState") && content.contains("failed"))));
                AssistantToolReply { content: answer.into(), calls: vec![], raw_message: Value::Null }
            }
        }))
    }, |request| {
        reads += 1;
        // This fixture covers native provider/tool transport. The crate's tests
        // exercise actual retrieval; a protocol fixture must not download a model.
        let source = json!({"url":"https://example.test/server/setup", "title":"Publisher dedicated server guide", "authority":"Fixture publisher", "kind":"official", "sourceId":"publisher", "retrievedAt":1_750_000_000_u64, "contentSha256":"a".repeat(64), "sourceState":"failed"});
        let mut evidence = match request {
            AssistantReadTool::SearchGameDocs { query, offset } => {
                assert_eq!(query, "开服"); assert_eq!(offset, 5);
                json!({"moduleId":"schema-fixture", "query":query, "entries":[{"id":"0123456789abcdef0123456789abcdef", "citationId":"0123456789abcdef0123456789abcdef:1024", "title":"开服准备", "heading":"Backups", "snippet":"Before updating", "offsetBytes":1024, "source":source, "semanticScore":0.75}], "nextOffset":null, "retrieval":"learned_multilingual_vectors+fts5_rrf", "model":"fixture", "evidenceNotice":"Reference only; no server inspected."})
            }
            AssistantReadTool::ReadGameDoc { document_id, offset } => {
                assert_eq!(document_id, "0123456789abcdef0123456789abcdef"); assert_eq!(offset, 1024);
                json!({"moduleId":"schema-fixture", "id":document_id, "citationId":"0123456789abcdef0123456789abcdef:1024", "title":"开服准备", "body":"Before updating, stop the server and preserve a backup.", "offsetBytes":offset, "nextOffsetBytes":2048, "totalBytes":4096, "source":source, "evidenceNotice":"Reference only; no server inspected."})
            }
            _ => panic!("Only scoped knowledge tools belong in this fixture"),
        };
        evidence["scope"] = json!("game_documentation");
        evidence["instanceObserved"] = json!(false);
        let delivered = assistant_tool_result_text(&json!({"ok":true,"data":evidence}));
        assert!(delivered.len() <= ASSISTANT_TOOL_RESULT_BYTES);
        let parsed: Value = serde_json::from_str(&delivered).unwrap();
        let data = &parsed["data"];
        let doc = data.get("entries").map_or(data, |entries| &entries[0]);
        assert_eq!(data["scope"], "game_documentation");
        assert_eq!(data["instanceObserved"], false);
        assert_eq!(doc["citationId"], "0123456789abcdef0123456789abcdef:1024");
        assert_eq!(doc["offsetBytes"], 1024);
        assert_eq!(doc["source"]["url"], "https://example.test/server/setup");
        assert_eq!(doc["source"]["retrievedAt"], 1_750_000_000_u64);
        assert_eq!(doc["source"]["sourceState"], "failed");
        ready(Ok(evidence))
    }, &|operation| {
        assert_eq!(serde_json::from_str::<Value>(operation).unwrap()["action"], "none");
        Ok(())
    })).await.unwrap();
    assert_eq!(reads, 2);
    assert_eq!(turns, 3);
    assert_eq!(
        serde_json::from_str::<Value>(&output).unwrap()["reason"],
        answer
    );
}
