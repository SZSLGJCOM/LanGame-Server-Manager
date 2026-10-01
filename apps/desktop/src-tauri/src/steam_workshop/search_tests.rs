use serde_json::json;

use super::*;

fn browse_query(page: u32) -> Value {
    json!({
        "queryKey": ["workshop_browse", {
            "appid": 322330, "browse_sort": "textsearch", "page": page,
            "num_per_page": 30, "search_text": "显示", "section": "readytouseitems"
        }, 1],
        "state": { "data": {
            "eresult": 1, "current_page": page, "total_pages": 2, "total_count": 31,
            "results": [{
                "publishedfileid": "2964299587", "consumer_appid": 322330, "file_type": 0,
                "title": "伤害显示 \"Show Damage\"", "short_description": "[b]显示伤害[/b]",
                "preview_url": "https://example.invalid/preview.jpg", "subscriptions": 1234,
                "num_children": 1, "tags": [{"tag": "server_only_mod"}]
            }]
        }}
    })
}

fn page_html(queries: Vec<Value>) -> String {
    let context = json!({"queryData": json!({"queries": queries}).to_string()}).to_string();
    format!(
        "<a href=\"https://steamcommunity.com/sharedfiles/filedetails/?id=441378551\">Learn more</a><script>window.SSR.renderContext = JSON.parse({});</script>",
        serde_json::to_string(&context).expect("synthetic JSON")
    )
}

fn parse(html: &str, page: u32) -> Result<(Vec<SteamWorkshopLookupItem>, u64, bool), String> {
    parse_browse_page(
        html,
        322330,
        "显示",
        SteamWorkshopSearchSort::Relevance,
        page,
        SteamWorkshopBrowseKind::Item,
    )
}

#[test]
fn browse_uses_ordered_catalog_not_tutorial_or_unrelated_queries() {
    let unrelated = json!({"queryKey": ["workshop_about_numbers", 322330], "state": {"data": {"total_count": 99999}}});
    let (items, total, has_more) =
        parse(&page_html(vec![unrelated, browse_query(1)]), 1).expect("catalog");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, "2964299587");
    assert_eq!(items[0].title.as_deref(), Some("伤害显示 \"Show Damage\""));
    assert_eq!(items[0].consumer_app_id, Some(322330));
    assert_eq!(items[0].subscriptions, Some(1234));
    assert_eq!(items[0].description, None);
    assert_eq!(items[0].description_excerpt.as_deref(), Some("显示伤害"));
    assert_eq!(items[0].item_kind, "item");
    assert_eq!(total, 31);
    assert!(has_more);
}

#[test]
fn browse_empty_catalog_does_not_return_the_tutorial_link() {
    let mut query = browse_query(1);
    query["state"]["data"]["results"] = json!([]);
    query["state"]["data"]["total_count"] = json!(0);
    query["state"]["data"]["total_pages"] = json!(0);
    let (items, total, has_more) = parse(&page_html(vec![query]), 1).expect("empty catalog");
    assert!(items.is_empty());
    assert_eq!(total, 0);
    assert!(!has_more);
}

#[test]
fn browse_final_page_stops_pagination() {
    let (_, total, has_more) = parse(&page_html(vec![browse_query(2)]), 2).expect("final page");
    assert_eq!(total, 31);
    assert!(!has_more);
}

#[test]
fn normal_catalog_excludes_non_installable_shared_files() {
    let mut query = browse_query(1);
    query["state"]["data"]["results"][0]["file_type"] = json!(9);
    let (items, _, _) = parse(&page_html(vec![query]), 1).expect("typed catalog");
    assert!(items.is_empty());
}

#[test]
fn browse_keeps_a_real_catalog_item_with_an_empty_title() {
    // Observed in Steam's mostrecent catalog: a published Mod can have title:"".
    let mut query = browse_query(1);
    query["state"]["data"]["results"][0]["title"] = json!("");
    let (items, total, more) = parse(&page_html(vec![query.clone()]), 1).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, "2964299587");
    assert_eq!(items[0].title.as_deref(), Some("2964299587"));
    assert_eq!(items[0].status, "resolved");
    assert_eq!(total, 31);
    assert!(more);
    for title in [Value::Null, json!(123)] {
        query["state"]["data"]["results"][0]["title"] = title;
        let error: Value =
            serde_json::from_str(&parse(&page_html(vec![query.clone()]), 1).unwrap_err()).unwrap();
        assert_eq!(error["code"], "steam_workshop_browse_unrecognized_response");
    }
}

#[tokio::test]
async fn collection_subscriber_sort_is_rejected_before_network_instead_of_accepting_an_empty_catalog()
 {
    for locale in ["zh-CN", "en-US"] {
        let error = search_public_workshop_items(
            322330,
            None,
            Some("subscribers".into()),
            Some(1),
            Some(locale.into()),
            Some("collection".into()),
        )
        .await
        .unwrap_err();
        let error: Value = serde_json::from_str(&error).expect("structured sort error");
        assert_eq!(error["code"], "steam_workshop_browse_unsupported_sort");
        assert_eq!(error["sort"], "subscribers");
        assert_eq!(error["browse_kind"], "collection");
    }
    // Steam answers this unsupported combination with an unrelated accepted/0
    // query. Even if one reaches parsing, that is not a valid empty catalog.
    let mut query = browse_query(1);
    query["queryKey"][1]["section"] = json!("collections");
    query["queryKey"][1]["browse_sort"] = json!("accepted");
    query["state"]["data"]["results"] = json!([]);
    query["state"]["data"]["total_count"] = json!(0);
    query["state"]["data"]["total_pages"] = json!(0);
    let error = parse_browse_page(
        &page_html(vec![query]),
        322330,
        "显示",
        SteamWorkshopSearchSort::Subscribers,
        1,
        SteamWorkshopBrowseKind::Collection,
    )
    .unwrap_err();
    let error: Value = serde_json::from_str(&error).unwrap();
    assert_eq!(error["code"], "steam_workshop_browse_unrecognized_response");
}

#[test]
fn unknown_incomplete_failed_and_mismatched_pages_are_errors() {
    let error: Value =
        serde_json::from_str(&parse("<html>Sign in or retry later</html>", 1).unwrap_err())
            .unwrap();
    assert_eq!(error["code"], "steam_workshop_browse_unrecognized_response");
    assert!(error["message"].as_str().unwrap().contains("hydration"));
    assert!(parse("window.SSR.renderContext = JSON.parse(\"broken\")", 1).is_err());
    assert!(parse(&page_html(vec![browse_query(2)]), 1).is_err());
    for (pointer, value) in [
        ("/state/data/eresult", json!(2)),
        ("/state/data/total_count", Value::Null),
        ("/state/data/results", json!([])),
        ("/queryKey/1/search_text", json!("other")),
        ("/state/data/results/0/consumer_appid", json!(108600)),
    ] {
        let mut query = browse_query(1);
        *query.pointer_mut(pointer).expect("fixture field") = value;
        assert!(parse(&page_html(vec![query]), 1).is_err(), "{pointer}");
    }
}

#[test]
fn search_keeps_selected_sort_and_chinese_query_with_explicit_page_size() {
    let client = reqwest::Client::new();
    for sort in [
        SteamWorkshopSearchSort::Relevance,
        SteamWorkshopSearchSort::Trend,
        SteamWorkshopSearchSort::Popular,
        SteamWorkshopSearchSort::Recent,
        SteamWorkshopSearchSort::Subscribers,
    ] {
        let request = build_search_request(
            &client,
            322330,
            "显示 + 地图",
            sort,
            21,
            "schinese",
            SteamWorkshopBrowseKind::Item,
        )
        .expect("request");
        let params = request.url().query_pairs().collect::<HashMap<_, _>>();
        assert_eq!(
            params.get("searchtext").map(|value| value.as_ref()),
            Some("显示 + 地图")
        );
        assert_eq!(
            params.get("browsesort").map(|value| value.as_ref()),
            Some(sort.browse_sort())
        );
        assert_eq!(
            params.get("numperpage").map(|value| value.as_ref()),
            Some("30")
        );
        assert_eq!(params.get("p").map(|value| value.as_ref()), Some("21"));
        assert_eq!(
            params.get("l").map(|value| value.as_ref()),
            Some("schinese")
        );
    }
}

#[test]
fn collection_browse_uses_its_own_section_and_preserves_pagination_and_type() {
    let mut query = browse_query(1);
    query["queryKey"][1]["section"] = json!("collections");
    query["state"]["data"]["results"][0]["file_type"] = json!(2);
    query["state"]["data"]["results"][0]["num_children"] = json!(23);
    let html = page_html(vec![browse_query(1), query.clone()]);
    let (items, total, more) = parse_browse_page(
        &html,
        322330,
        "显示",
        SteamWorkshopSearchSort::Relevance,
        1,
        SteamWorkshopBrowseKind::Collection,
    )
    .unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].item_kind, "collection");
    assert_eq!(items[0].child_count, 23);
    assert_eq!(total, 31);
    assert!(more);
    assert!(
        parse(&page_html(vec![query]), 1).is_err(),
        "the Mod catalog must not accept collection SSR state"
    );
    let request = build_search_request(
        &reqwest::Client::new(),
        322330,
        "显示",
        SteamWorkshopSearchSort::Recent,
        2,
        "english",
        SteamWorkshopBrowseKind::Collection,
    )
    .unwrap();
    let params = request.url().query_pairs().collect::<HashMap<_, _>>();
    assert_eq!(
        params.get("section").map(|value| value.as_ref()),
        Some("collections")
    );
    assert_eq!(params.get("p").map(|value| value.as_ref()), Some("2"));
    assert_eq!(
        params.get("browsesort").map(|value| value.as_ref()),
        Some("mostrecent")
    );
}

#[test]
fn browse_kind_defaults_to_mods_and_rejects_unknown_values_before_network_access() {
    assert_eq!(
        SteamWorkshopBrowseKind::parse(None).unwrap(),
        SteamWorkshopBrowseKind::Item
    );
    assert_eq!(
        SteamWorkshopBrowseKind::parse(Some("collection")).unwrap(),
        SteamWorkshopBrowseKind::Collection
    );
    for value in ["", "all", "collections", "../collection"] {
        assert!(SteamWorkshopBrowseKind::parse(Some(value)).is_err());
    }
}

#[test]
fn collection_browse_filters_mods_and_uninstallable_files_without_falsifying_totals() {
    let mut query = browse_query(2);
    query["queryKey"][1]["section"] = json!("collections");
    let (items, total, more) = parse_browse_page(
        &page_html(vec![query]),
        322330,
        "显示",
        SteamWorkshopSearchSort::Relevance,
        2,
        SteamWorkshopBrowseKind::Collection,
    )
    .unwrap();
    assert!(items.is_empty());
    assert_eq!(total, 31);
    assert!(!more);
}

#[test]
fn search_language_tracks_the_interface_locale() {
    assert_eq!(workshop_language(None), "schinese");
    assert_eq!(workshop_language(Some("zh-CN")), "schinese");
    assert_eq!(workshop_language(Some("en")), "english");
}

#[tokio::test]
#[ignore = "reads public Steam endpoints; requires LGSM_WORKSHOP_LIVE_PROBE=1"]
async fn live_collection_browse_smoke() {
    assert_eq!(
        std::env::var("LGSM_WORKSHOP_LIVE_PROBE").as_deref(),
        Ok("1")
    );
    tokio::time::timeout(std::time::Duration::from_secs(60), async {
        let mut first_page_ids = Vec::new();
        for page in [1, 2] {
            let result = search_public_workshop_items(
                322330,
                None,
                Some("trend".into()),
                Some(page),
                Some("en-US".into()),
                Some("collection".into()),
            )
            .await
            .expect("public collection catalog");
            assert_eq!(result.browse_kind, "collection");
            assert_eq!(result.page, page);
            assert_eq!(result.items.len(), 30);
            assert!(
                result
                    .items
                    .iter()
                    .all(|item| item.item_kind == "collection"
                        && item.consumer_app_id == Some(322330))
            );
            assert!(result.total_count.is_some_and(|count| count > 30));
            assert!(result.has_more);
            assert!(result.source_url.contains("section=collections"));
            let ids = result
                .items
                .iter()
                .map(|item| item.id.clone())
                .collect::<Vec<_>>();
            if page == 1 {
                first_page_ids = ids;
            } else {
                assert_ne!(ids, first_page_ids);
            }
            println!(
                "collection page {page}: {} items, total {:?}",
                result.items.len(),
                result.total_count
            );
        }
    })
    .await
    .expect("collection browse probe must finish within 60 seconds");
}

#[tokio::test]
#[ignore = "reads public Steam endpoints; requires LGSM_WORKSHOP_LIVE_PROBE=1"]
async fn live_browse_sort_contract() {
    assert_eq!(
        std::env::var("LGSM_WORKSHOP_LIVE_PROBE").as_deref(),
        Ok("1")
    );
    tokio::time::timeout(std::time::Duration::from_secs(60), async {
        for (kind, sort, query, locale, empty) in [
            ("collection", "popular", "", "zh-CN", false),
            ("collection", "relevance", "显示", "en-US", false),
            ("item", "recent", "", "zh-CN", false),
            ("item", "relevance", "zzqplmnofjkeiacvuxth", "en-US", true),
        ] {
            let result = search_public_workshop_items(
                322330,
                Some(query.into()),
                Some(sort.into()),
                Some(1),
                Some(locale.into()),
                Some(kind.into()),
            )
            .await
            .expect("public typed catalog");
            assert_eq!(result.browse_kind, kind);
            assert_eq!(result.sort, sort);
            assert_eq!(result.page, 1);
            assert_eq!(result.items.is_empty(), empty);
            assert!(
                result
                    .items
                    .iter()
                    .all(|item| item.consumer_app_id == Some(322330)
                        && item.item_kind == kind
                        && item.title.is_some())
            );
            if empty {
                assert_eq!(result.total_count, Some(0));
                assert!(!result.has_more);
            } else {
                assert_eq!(result.items.len(), 30);
                assert!(result.has_more);
            }
            println!(
                "{kind}/{sort}/{locale}: {} items, total {:?}",
                result.items.len(),
                result.total_count
            );
        }
    })
    .await
    .expect("browse sort probe must finish within 60 seconds");
}

#[test]
fn default_sort_matches_query_intent_and_explicit_sort_is_preserved() {
    assert_eq!(
        normalize_search_sort(None, false),
        SteamWorkshopSearchSort::Relevance
    );
    assert_eq!(
        normalize_search_sort(None, true),
        SteamWorkshopSearchSort::Trend
    );
    assert_eq!(
        normalize_search_sort(Some("relevance"), true),
        SteamWorkshopSearchSort::Trend
    );
    assert_eq!(
        normalize_search_sort(Some("subscribed"), false),
        SteamWorkshopSearchSort::Subscribers
    );
}

#[test]
fn direct_lookup_accepts_only_numeric_ids_or_steam_detail_urls() {
    for query in [
        "2964299587",
        "https://steamcommunity.com/sharedfiles/filedetails/?id=2964299587&searchtext=hi",
    ] {
        assert_eq!(
            exact_workshop_id(query).expect("ID"),
            Some("2964299587".to_string())
        );
    }
    for query in [
        "显示",
        "https://example.invalid/sharedfiles/filedetails/?id=2964299587",
        "https://steamcommunity.com/workshop/browse/?id=2964299587",
    ] {
        assert_eq!(exact_workshop_id(query).expect("text"), None);
    }
    assert!(exact_workshop_id("18446744073709551616").is_err());
}

#[test]
fn exact_lookup_keeps_unverified_identity_without_cross_game_or_known_type_leaks() {
    let id = "890030001";
    let details = HashMap::from([(
        id.to_string(),
        json!({"result":1,"consumer_app_id":322330,"title":"Original title"}),
    )]);
    let mut item = build_lookup_item(id, &details, &HashMap::new(), &details).unwrap();
    assert_eq!(item.status, "unverified");
    assert_eq!(item.item_kind, "unknown");
    for kind in [
        SteamWorkshopBrowseKind::Item,
        SteamWorkshopBrowseKind::Collection,
    ] {
        assert!(matches_exact_item(&item, 322330, kind));
        assert!(!matches_exact_item(&item, 108600, kind));
    }
    item.consumer_app_id = None;
    assert!(!matches_exact_item(
        &item,
        322330,
        SteamWorkshopBrowseKind::Item
    ));
    item.consumer_app_id = Some(322330);
    item.status = "not_found".into();
    assert!(!matches_exact_item(
        &item,
        322330,
        SteamWorkshopBrowseKind::Item
    ));
    item.status = "unsupported".into();
    item.item_kind = "guide".into();
    assert!(!matches_exact_item(
        &item,
        322330,
        SteamWorkshopBrowseKind::Item
    ));
    assert!(!matches_exact_item(
        &item,
        322330,
        SteamWorkshopBrowseKind::Collection
    ));
    item.status = "resolved".into();
    for (item_kind, selected, other) in [
        (
            "item",
            SteamWorkshopBrowseKind::Item,
            SteamWorkshopBrowseKind::Collection,
        ),
        (
            "collection",
            SteamWorkshopBrowseKind::Collection,
            SteamWorkshopBrowseKind::Item,
        ),
    ] {
        item.item_kind = item_kind.into();
        assert!(matches_exact_item(&item, 322330, selected));
        assert!(!matches_exact_item(&item, 322330, other));
    }
}

#[tokio::test]
#[ignore = "reads public Steam endpoints; requires LGSM_WORKSHOP_LIVE_PROBE=1"]
async fn live_workshop_search_smoke() {
    use std::time::{Duration, Instant};

    assert_eq!(
        std::env::var("LGSM_WORKSHOP_LIVE_PROBE").as_deref(),
        Ok("1"),
        "Set LGSM_WORKSHOP_LIVE_PROBE=1 before explicitly running this live probe."
    );
    // Five bounded public search operations. The exact URL operation also resolves
    // collection metadata through the existing read-only lookup API.
    let cases = [
        (322330, "显示", "relevance", 1),
        (322330, "显示", "relevance", 2),
        (322330, "显示", "popular", 1),
        (108600, "", "trend", 1),
        (
            322330,
            "https://steamcommunity.com/sharedfiles/filedetails/?id=2964299587",
            "relevance",
            1,
        ),
    ];
    tokio::time::timeout(Duration::from_secs(120), async {
        let mut first_page_ids = Vec::new();
        for (index, (app_id, query, sort, page)) in cases.into_iter().enumerate() {
            let started = Instant::now();
            let result = search_public_workshop_items(
                app_id,
                Some(query.to_string()),
                Some(sort.to_string()),
                Some(page),
                Some("zh-CN".to_string()),
                None,
            )
            .await
            .expect("public Steam search should succeed");
            let ids = result
                .items
                .iter()
                .map(|item| item.id.clone())
                .collect::<Vec<_>>();
            println!(
                "{}",
                json!({
                    "app_id": app_id, "query": query, "sort": sort, "page": result.page,
                    "total_count": result.total_count, "item_count": result.items.len(),
                    "first_ids": ids.iter().take(3).collect::<Vec<_>>(),
                    "elapsed_ms": started.elapsed().as_millis()
                })
            );
            assert_eq!(result.app_id, app_id);
            assert_eq!(result.page, page);
            assert_eq!(result.sort, sort);
            assert_eq!(result.page_size, SEARCH_PAGE_SIZE);
            assert!(!result.items.is_empty());
            assert!(result.total_count.is_some_and(|count| count > 0));
            assert!(result.items.iter().all(|item| {
                item.consumer_app_id == Some(app_id)
                    && item.status == "resolved"
                    && item.id != "441378551"
                    && item.id != "2872282653"
            }));
            if index == 0 {
                first_page_ids = ids;
            } else if index == 1 {
                assert_ne!(
                    ids, first_page_ids,
                    "next page must not repeat the first page"
                );
            } else if index == 4 {
                assert_eq!(ids, ["2964299587"]);
                assert_eq!(result.total_count, Some(1));
                assert!(!result.has_more);
            }
        }
    })
    .await
    .expect("the public Workshop smoke probe must finish within 120 seconds");
}
