use serde_json::json;

use super::*;

#[test]
fn normalize_lookup_ids_deduplicates_and_preserves_order() {
    let ids = normalize_lookup_ids(vec![
        String::from("2039181790"),
        String::from("2039181790"),
        String::from("1909182187"),
    ])
    .expect("lookup ids should normalize");
    assert_eq!(ids, vec!["2039181790", "1909182187"]);
}

#[test]
fn normalize_lookup_ids_rejects_overflow_and_zero() {
    for id in ["18446744073709551616", "000000", "12hello"] {
        assert!(normalize_lookup_ids(vec![id.to_string()]).is_err());
    }
}

#[test]
fn summarize_description_collapses_whitespace() {
    assert_eq!(
        summarize_description("Line one\n\n  Line two  "),
        "Line one Line two"
    );
}

#[test]
fn workshop_markup_is_removed_from_public_descriptions() {
    assert_eq!(
        strip_workshop_markup("[h1]Title[/h1]\n[b]Body[/b]"),
        "Title\nBody"
    );
}

#[test]
fn failed_item_result_is_not_reported_as_resolved() {
    let details = HashMap::from([(
        "999999".to_string(),
        json!({"result": 9, "publishedfileid": "999999"}),
    )]);
    let item = build_lookup_item("999999", &details, &HashMap::new(), &HashMap::new())
        .expect("lookup item");
    assert_eq!(item.status, "not_found");
    assert!(item.message.is_some());
}

#[test]
fn normal_mod_with_dependencies_is_not_a_collection() {
    let details = HashMap::from([(
        "123456".to_string(),
        json!({"result": 1, "num_children": 2, "file_type": 0}),
    )]);
    let item = build_lookup_item("123456", &details, &HashMap::new(), &HashMap::new())
        .expect("lookup item");
    assert_eq!(item.item_kind, "item");
}

#[test]
fn dst_mod_pipeline_nested_collection_requires_explicit_expansion() {
    let details = HashMap::from([("900000".to_owned(), json!({"result":1}))]);
    let children = HashMap::from([
        (
            "100000".to_owned(),
            json!({"result":1,"file_type":0,"consumer_app_id":322330}),
        ),
        (
            "900001".to_owned(),
            json!({"result":1,"file_type":2,"consumer_app_id":322330}),
        ),
    ]);
    let collections = HashMap::from([(
        "900000".to_owned(),
        vec!["100000".to_owned(), "900001".to_owned()],
    )]);
    let item = build_lookup_item("900000", &details, &collections, &children).expect("lookup");
    assert_eq!(item.status, "resolved");
    assert!(
        item.message
            .as_deref()
            .is_some_and(|message| message.contains("nested"))
    );
}

#[test]
fn empty_collection_is_still_a_collection() {
    let details = HashMap::from([("123456".to_string(), json!({"result": 1}))]);
    let collections = HashMap::from([("123456".to_string(), Vec::new())]);
    let item =
        build_lookup_item("123456", &details, &collections, &HashMap::new()).expect("lookup item");
    assert_eq!(item.item_kind, "collection");
    assert_eq!(item.child_count, 0);
}

#[test]
fn public_details_without_verified_type_are_not_a_resolved_mod() {
    let details = HashMap::from([(
        "345678".to_string(),
        json!({"result": 1, "consumer_app_id": 322330, "title": "Saved Mod", "type_error": "HTTP 429 fixture"}),
    )]);
    let item = build_lookup_item("345678", &details, &HashMap::new(), &HashMap::new())
        .expect("lookup item");
    assert_eq!(item.status, "unverified");
    assert_eq!(item.item_kind, "unknown");
    assert_eq!(item.title.as_deref(), Some("Saved Mod"));
    assert_eq!(item.message.as_deref(), Some("HTTP 429 fixture"));
}

#[test]
fn mixed_collections_keep_mods_guides_and_subcollections_distinguishable() {
    let details = HashMap::from([("456789".to_string(), json!({"result": 1}))]);
    let children = HashMap::from([
        (
            "567890".to_string(),
            json!({"result": 1, "file_type": 0, "consumer_app_id": 322330}),
        ),
        (
            "678901".to_string(),
            json!({"result": 1, "file_type": 9, "consumer_app_id": 322330}),
        ),
        (
            "789012".to_string(),
            json!({"result": 1, "file_type": 2, "consumer_app_id": 322330}),
        ),
    ]);
    let collections = HashMap::from([(
        "456789".to_string(),
        vec![
            "567890".to_string(),
            "678901".to_string(),
            "789012".to_string(),
        ],
    )]);
    let item = build_lookup_item("456789", &details, &collections, &children).expect("collection");
    assert_eq!(item.item_kind, "collection");
    assert_eq!(item.status, "resolved");
    assert_eq!(item.child_count, 3);
    assert_eq!(
        (
            item.children[0].item_kind.as_str(),
            item.children[0].status.as_str()
        ),
        ("item", "resolved")
    );
    assert_eq!(
        (
            item.children[1].item_kind.as_str(),
            item.children[1].status.as_str()
        ),
        ("guide", "unsupported")
    );
    assert_eq!(
        (
            item.children[2].item_kind.as_str(),
            item.children[2].status.as_str()
        ),
        ("collection", "resolved")
    );
}

#[test]
fn collection_child_throttling_is_visible_without_misclassifying_the_collection() {
    let details = HashMap::from([("456789".into(), json!({"result": 1, "file_type": 2}))]);
    let children = HashMap::from([(
        "567890".into(),
        json!({"result": 1, "title": "Saved child", "type_error": "HTTP 429 fixture"}),
    )]);
    let collections = HashMap::from([("456789".into(), vec!["567890".into()])]);
    let item = build_lookup_item("456789", &details, &collections, &children).unwrap();
    assert_eq!(item.item_kind, "collection");
    assert_eq!(item.status, "unverified");
    assert_eq!(item.message.as_deref(), Some("HTTP 429 fixture"));
    assert_eq!(item.children[0].title.as_deref(), Some("Saved child"));
    assert_eq!(item.children[0].status, "unverified");
}

#[test]
fn collection_children_preserve_client_tags_for_server_install_review() {
    let details = HashMap::from([(
        "456789".into(),
        json!({"result": 1, "file_type": 2, "consumer_app_id": 322330}),
    )]);
    let children = HashMap::from([
        (
            "1365141672".into(),
            json!({"result": 1, "file_type": 0, "consumer_app_id": 322330,
                "tags": [{"tag": "client_only_mod"}, {"tag": "utility"}]}),
        ),
        (
            "567890".into(),
            json!({"result": 1, "file_type": 0, "consumer_app_id": 322330}),
        ),
    ]);
    let collections =
        HashMap::from([("456789".into(), vec!["1365141672".into(), "567890".into()])]);
    let item = build_lookup_item("456789", &details, &collections, &children).unwrap();
    assert_eq!(item.status, "resolved");
    assert_eq!(item.children[0].tags, ["client_only_mod", "utility"]);
    assert!(item.children[1].tags.is_empty());
    let serialized = serde_json::to_value(item).unwrap();
    assert_eq!(serialized["children"][0]["tags"][0], "client_only_mod");
    assert_eq!(serialized["children"][1]["tags"], json!([]));
}

#[test]
fn cached_child_type_without_public_metadata_cannot_imply_server_compatibility() {
    let id = "985763240178";
    file_types::remember_file_type(id, 0).unwrap();
    let details = HashMap::from([("456789".into(), json!({"result": 1, "file_type": 2}))]);
    let collections = HashMap::from([("456789".into(), vec![id.into()])]);
    let item = build_lookup_item("456789", &details, &collections, &HashMap::new()).unwrap();
    assert_eq!(item.children[0].status, "unverified");
    assert!(item.children[0].consumer_app_id.is_none());
    assert_eq!(
        item.status, "resolved",
        "The manifest reviewer can fetch omitted child metadata"
    );
}

#[tokio::test]
#[ignore = "reads public Steam endpoints; requires LGSM_WORKSHOP_LIVE_PROBE=1"]
async fn live_workshop_type_safety_smoke() {
    use std::time::{Duration, Instant};
    assert_eq!(
        std::env::var("LGSM_WORKSHOP_LIVE_PROBE").as_deref(),
        Ok("1")
    );
    let started = Instant::now();
    let ids = vec![
        "441378551".to_string(),
        "2964299587".to_string(),
        "3739491677".to_string(),
    ];
    let items = tokio::time::timeout(
        Duration::from_secs(50),
        lookup_public_workshop_items(ids, app_network::SourcePreference::InternationalFirst),
    )
    .await
    .expect("bounded live lookup")
    .expect("verified public item types");
    println!(
        "{}",
        json!({"elapsed_ms": started.elapsed().as_millis(), "items": items.iter().map(|item| json!({
        "id": item.id, "item_kind": item.item_kind, "status": item.status,
        "app_id": item.consumer_app_id, "client_only": item.tags.iter().any(|tag| tag == "client_only_mod")
    })).collect::<Vec<_>>()})
    );
    assert_eq!(items.len(), 3);
    assert_eq!(
        (items[0].item_kind.as_str(), items[0].status.as_str()),
        ("guide", "unsupported")
    );
    assert_eq!(
        (items[1].item_kind.as_str(), items[1].status.as_str()),
        ("item", "resolved")
    );
    assert_eq!(
        (items[2].item_kind.as_str(), items[2].status.as_str()),
        ("item", "resolved")
    );
    assert!(items[2].tags.iter().any(|tag| tag == "client_only_mod"));
    assert!(
        items
            .iter()
            .all(|item| item.consumer_app_id == Some(322330))
    );
}
