use super::*;
use serde_json::json;

fn page(id: &str, title: &str, description: &str) -> String {
    format!(
        r#"<html><script>var publishedfileid = '{id}'; var searchText = "";</script>
        <div class="workshopItemDetailsHeader"><div class="workshopItemTitle">{title}</div></div>
        <div class="workshopItemDescription" id="highlightContent">{description}</div>
        <a onclick="PublishedFileAward( '{id}', 0, 1 )"></a></html>"#
    )
}

#[test]
fn localized_text_uses_only_the_verified_item_containers_and_decodes_entities() {
    let html = page("666155465", "中文 &amp; English &#x1F600;", "第一段<br>第二段<div>标题</div><a href='https://example.invalid'>链接</a><script>secret()</script><style>hidden{}</style>")
        .replace("</html>", "<div class='workshopItemTitle'>Unrelated title</div><footer>Footer text</footer></html>");
    let text = parse_localized_text(&html, "666155465").expect("localized text");
    assert_eq!(text.title, "中文 & English 😀");
    let description = text.description.as_deref().expect("description");
    for part in ["第一段", "第二段", "标题", "链接"] {
        assert!(description.contains(part));
    }
    assert!(description.contains('\n'));
    for excluded in [
        "secret",
        "hidden",
        "Footer",
        "Unrelated",
        "<br>",
        "https://",
    ] {
        assert!(!description.contains(excluded));
    }
    assert_eq!(text.file_type, Some(0));
}

#[test]
fn native_item_script_prefix_does_not_hide_identity_and_script_strings_are_not_evidence() {
    let valid = page("666155465", "Localized title", "Localized description");
    let real_structure = valid.replace(
        "var publishedfileid",
        "var bSkipVideos = true;\n\tvar SESSION_ID = 'synthetic-session';\n\tvar publishedfileid",
    );
    assert_eq!(
        parse_localized_text(&real_structure, "666155465")
            .unwrap()
            .title,
        "Localized title"
    );
    for script in [
        "// var publishedfileid = '666155465';",
        "/* var publishedfileid = '666155465'; */",
        "function f() { var publishedfileid = '666155465'; }",
        "var text = \"var publishedfileid = '666155465';\";",
        "var text = `\nvar publishedfileid = '666155465';\n`;",
        "var publishedfileid = '123456789';",
        "var publishedfileid = '666155465'; var publishedfileid = '123456789';",
    ] {
        let html = valid.replace(
            "var publishedfileid = '666155465'; var searchText = \"\";",
            script,
        );
        assert!(
            parse_localized_text(&html, "666155465").is_err(),
            "{script}"
        );
    }
}

#[test]
fn localized_text_rejects_wrong_ambiguous_and_missing_identity_or_containers() {
    let valid = page("666155465", "Mod", "Description");
    let cases = [
        page("123456789", "Other Mod", "Description"),
        valid.replace("var publishedfileid", "var otherId"),
        valid.replace(
            "</html>",
            "<script>var publishedfileid = '666155465';</script></html>",
        ),
        valid.replace("workshopItemDetailsHeader", "unrecognizedHeader"),
        valid.replace(
            "</html>",
            "<div class='workshopItemDescription' id='highlightContent'>duplicate</div></html>",
        ),
        "<div id='highlightContent'><script>var publishedfileid = '666155465';</script></div>"
            .into(),
    ];
    for html in cases {
        let error = parse_localized_text(&html, "666155465")
            .err()
            .expect("invalid page");
        assert_eq!(
            serde_json::from_str::<Value>(&error).unwrap()["code"],
            "steam_workshop_details_unrecognized_response"
        );
    }
}

#[test]
fn localized_display_preserves_author_fallback_and_metadata_identity_type_and_children() {
    let html = page("666155465", "Author's English fallback", "English fallback &amp; details").replace("</html>", r#"
        <div id="mainContentsCollection"><div class="collectionChildren">
          <div class="collectionItem" id="sharedfile_123456789"><div class="collectionItemDetails"><a><div class="workshopItemTitle">成员中文</div></a></div></div>
          <div class="collectionItem" id="sharedfile_987654321"><div class="collectionItemDetails"><a><div class="workshopItemTitle">Unlisted member</div></a></div></div>
        </div></div></html>"#);
    let text = parse_localized_text(&html, "666155465").unwrap();
    let mut metadata = HashMap::from([
        (
            "666155465".into(),
            json!({"result":1,"consumer_app_id":322330,"file_type":2,"title":"canonical","description":"canonical English"}),
        ),
        (
            "123456789".into(),
            json!({"result":1,"consumer_app_id":322330,"file_type":9,"title":"canonical child","tags":[{"tag":"test"}]}),
        ),
    ]);
    apply_localized_text(&mut metadata, "666155465", &text).unwrap();
    let collections = HashMap::from([("666155465".into(), vec!["123456789".into()])]);
    let item =
        super::super::build_lookup_item("666155465", &metadata, &collections, &metadata).unwrap();
    assert_eq!(item.title.as_deref(), Some("Author's English fallback"));
    assert_eq!(
        item.description.as_deref(),
        Some("English fallback & details")
    );
    assert_eq!(item.description_excerpt, item.description);
    assert_eq!(item.consumer_app_id, Some(322330));
    assert_eq!(item.item_kind, "collection");
    assert_eq!(item.children.len(), 1);
    assert_eq!(item.children[0].title.as_deref(), Some("成员中文"));
    assert_eq!(item.children[0].item_kind, "guide");
    assert_eq!(item.children[0].status, "unsupported");
    assert_eq!(item.children[0].tags, ["test"]);
    assert!(!metadata.contains_key("987654321"));
}

#[test]
fn localized_empty_text_clears_canonical_description_and_excerpt() {
    let text = parse_localized_text(&page("666155465", "", ""), "666155465").unwrap();
    let mut metadata = HashMap::from([(
        "666155465".into(),
        json!({"result":1,"file_type":0,"title":"Old title","description":"Old English"}),
    )]);
    apply_localized_text(&mut metadata, "666155465", &text).unwrap();
    let item = super::super::build_lookup_item("666155465", &metadata, &HashMap::new(), &metadata)
        .unwrap();
    assert_eq!(item.title.as_deref(), Some("666155465"));
    assert_eq!(item.description, None);
    assert_eq!(item.description_excerpt, None);

    let text = LocalizedText {
        description: None,
        ..text
    };
    metadata.get_mut("666155465").unwrap()["description"] =
        Value::from("Expected nonempty description");
    assert!(apply_localized_text(&mut metadata, "666155465", &text).is_err());
    metadata.get_mut("666155465").unwrap()["description"] = Value::from("");
    assert!(
        apply_localized_text(&mut metadata, "666155465", &text).is_ok(),
        "Steam omits the description node for empty collections"
    );
}

#[test]
fn browse_and_details_share_the_same_steam_language_mapping() {
    for locale in [None, Some("zh-CN"), Some(" ZH-cn "), Some("zh")] {
        assert_eq!(workshop_language(locale), "schinese");
    }
    for locale in [Some("en-US"), Some("en"), Some("fr")] {
        assert_eq!(workshop_language(locale), "english");
    }
}

#[tokio::test]
async fn localized_failure_preserves_metadata_without_retrying_community_or_authorizing_unknown_types()
 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = reqwest::Client::builder()
        .no_proxy()
        .proxy(reqwest::Proxy::all(format!("http://{}", listener.local_addr().unwrap())).unwrap())
        .timeout(Duration::from_millis(100))
        .build()
        .unwrap();
    let mod_id = "890010001";
    let unknown_id = "890010002";
    let collection_id = "890010003";
    let missing_id = "890010004";
    let guide_id = "890010005";
    let mut failures = vec![
        super::super::network_error::workshop_network_error(
            "details",
            &app_network::NetworkError::Deadline {
                attempts: 2,
                origin: "https://steamcommunity.com".into(),
            },
        ),
        invalid_page("response is not UTF-8"),
        invalid_page("page does not uniquely identify the requested item"),
    ];
    failures.extend([401, 403, 429].map(|status| {
        super::super::network_error::workshop_network_error(
            "details",
            &app_network::NetworkError::Status {
                status: reqwest::StatusCode::from_u16(status).unwrap(),
                origin: "https://steamcommunity-a.akamaihd.net".into(),
            },
        )
    }));
    for failure in failures {
        let details = HashMap::from([
            (
                mod_id.to_string(),
                json!({"result":1,"consumer_app_id":322330,"file_type":0,"title":"Original title","description":"Original description"}),
            ),
            (
                unknown_id.to_string(),
                json!({"result":1,"consumer_app_id":322330,"title":"Unverified original","description":"Retained original text"}),
            ),
            (
                collection_id.to_string(),
                json!({"result":1,"consumer_app_id":322330,"file_type":2,"title":"Original collection"}),
            ),
            (missing_id.to_string(), json!({"result":9})),
            (
                guide_id.to_string(),
                json!({"result":1,"consumer_app_id":322330,"file_type":9,"title":"Original guide"}),
            ),
        ]);
        // A collection with an unverified child cannot acquire installation rights
        // simply because the root's display can now survive an HTML failure.
        let collection = finish_localized_details(
            &client,
            collection_id,
            SourcePreference::ChinaFirst,
            details.clone(),
            HashMap::from([(collection_id.to_string(), vec![unknown_id.to_string()])]),
            Err(failure.clone()),
        )
        .await
        .unwrap();
        assert_eq!(collection.status, "unverified");
        assert_eq!(collection.children[0].status, "unverified");
        assert_eq!(collection.children[0].item_kind, "unknown");
        for (id, expected_status, expected_kind) in [
            (mod_id, "resolved", "item"),
            (unknown_id, "unverified", "unknown"),
            (guide_id, "unsupported", "guide"),
        ] {
            let item = finish_localized_details(
                &client,
                id,
                SourcePreference::ChinaFirst,
                details.clone(),
                HashMap::new(),
                Err(failure.clone()),
            )
            .await
            .unwrap();
            assert_eq!(item.status, expected_status);
            assert_eq!(item.item_kind, expected_kind);
            assert_eq!(item.consumer_app_id, Some(322330));
            assert_eq!(item.title.as_deref(), details[id]["title"].as_str());
            assert_eq!(
                item.description.as_deref(),
                details[id]["description"].as_str()
            );
            assert_eq!(item.localization_warning.as_deref(), Some(failure.as_str()));
            if id == unknown_id {
                assert_eq!(item.message.as_deref(), Some(failure.as_str()));
            }
        }
        let missing = finish_localized_details(
            &client,
            missing_id,
            SourcePreference::ChinaFirst,
            HashMap::from([(missing_id.to_string(), details[missing_id].clone())]),
            HashMap::new(),
            Err(failure),
        )
        .await
        .unwrap();
        assert_eq!(missing.status, "not_found");
        assert!(missing.title.is_none() && missing.localization_warning.is_none());
    }
    let listener = listener.into_std().unwrap();
    assert_eq!(
        listener
            .accept()
            .expect_err("fallback must not issue another Community request")
            .kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[tokio::test]
async fn localized_retry_restores_text_without_changing_verified_identity() {
    let id = "890020001";
    let details = HashMap::from([(
        id.to_string(),
        json!({"result":1,"consumer_app_id":322330,"file_type":0,"title":"Original title","description":"Original description"}),
    )]);
    let client = reqwest::Client::new();
    let fallback = finish_localized_details(
        &client,
        id,
        SourcePreference::ChinaFirst,
        details.clone(),
        HashMap::new(),
        Err("Temporary HTML outage".into()),
    )
    .await
    .unwrap();
    assert_eq!(
        fallback.description.as_deref(),
        Some("Original description")
    );
    assert!(fallback.localization_warning.is_some());
    let localized = finish_localized_details(
        &client,
        id,
        SourcePreference::ChinaFirst,
        details,
        HashMap::new(),
        parse_localized_text(&page(id, "中文标题", "中文说明"), id),
    )
    .await
    .unwrap();
    assert_eq!(localized.title.as_deref(), Some("中文标题"));
    assert_eq!(localized.description.as_deref(), Some("中文说明"));
    assert_eq!(localized.consumer_app_id, fallback.consumer_app_id);
    assert_eq!(localized.item_kind, fallback.item_kind);
    assert_eq!(localized.status, "resolved");
    assert!(localized.localization_warning.is_none());
}

#[tokio::test]
#[ignore = "reads public Steam endpoints; requires LGSM_WORKSHOP_LIVE_PROBE=1"]
async fn live_localized_details_and_collection_titles() {
    assert_eq!(
        std::env::var("LGSM_WORKSHOP_LIVE_PROBE").as_deref(),
        Ok("1")
    );
    tokio::time::timeout(Duration::from_secs(140), async {
        let english = read_public_workshop_item_details("666155465".into(), Some("en-US"))
            .await
            .expect("English details");
        let chinese = super::super::search_public_workshop_items(
            322330,
            Some("666155465".into()),
            Some("relevance".into()),
            Some(1),
            Some("zh-CN".into()),
            Some("item".into()),
        )
        .await
        .expect("Chinese exact-ID search")
        .items
        .remove(0);
        assert_eq!(english.id, chinese.id);
        assert_eq!(english.consumer_app_id, Some(322330));
        assert_eq!(english.consumer_app_id, chinese.consumer_app_id);
        assert_eq!(english.item_kind, "item");
        assert_eq!(chinese.item_kind, "item");
        assert_eq!(english.status, "resolved");
        assert_eq!(chinese.status, "resolved");
        assert_ne!(
            english.description, chinese.description,
            "This established public item supplies both author translations"
        );
        assert!(
            chinese
                .description
                .as_deref()
                .is_some_and(|text| text.contains("服务端"))
        );
        assert!(
            english
                .description
                .as_deref()
                .is_some_and(|text| text.contains("server mod"))
        );
        assert_ne!(english.description_excerpt, chinese.description_excerpt);
        let collection = read_public_workshop_item_details("3807110406".into(), Some("zh-CN"))
            .await
            .expect("Chinese collection details");
        assert_eq!(collection.consumer_app_id, Some(322330));
        assert_eq!(collection.item_kind, "collection");
        let child = collection
            .children
            .iter()
            .find(|child| child.id == "2373373592")
            .expect("known public collection member");
        assert_eq!(child.title.as_deref(), Some("新版血条动画（幽冥汉化版）"));
        assert_eq!(child.consumer_app_id, Some(322330));
        assert_eq!(child.item_kind, "item");
    })
    .await
    .expect("bounded localization live probe");
}
