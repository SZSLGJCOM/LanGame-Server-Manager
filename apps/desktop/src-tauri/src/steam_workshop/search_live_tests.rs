use std::time::{Duration, Instant};

use serde_json::json;

use super::{SEARCH_PAGE_SIZE, search_public_workshop_items};

#[tokio::test]
#[ignore = "reads public Steam endpoints; requires LGSM_WORKSHOP_LIVE_PROBE=1"]
async fn live_issue22_browse_matrix() {
    assert_eq!(
        std::env::var("LGSM_WORKSHOP_LIVE_PROBE").as_deref(),
        Ok("1"),
        "Set LGSM_WORKSHOP_LIVE_PROBE=1 before explicitly running this live probe."
    );
    let locales: &[&str] = match std::env::var("LGSM_WORKSHOP_LIVE_LOCALE").as_deref() {
        Ok("zh-CN") => &["zh-CN"],
        Ok("en-US") => &["en-US"],
        Err(std::env::VarError::NotPresent) => &["zh-CN", "en-US"],
        _ => panic!("LGSM_WORKSHOP_LIVE_LOCALE must be zh-CN or en-US when set."),
    };
    // Keep each locale to six anonymous reads through the product network and parser path.
    // Pippi is a public Conan Workshop search; assert its existence, not changing counts or IDs.
    let cases = [
        ("conan_trend_first", 440900, "item", "trend", 1, "", false),
        ("conan_trend_next", 440900, "item", "trend", 2, "", false),
        (
            "conan_popular_search",
            440900,
            "item",
            "popular",
            1,
            "Pippi",
            false,
        ),
        (
            "conan_empty_search",
            440900,
            "item",
            "relevance",
            1,
            "zzqplmnofjkeiacvuxthlgsmempty",
            true,
        ),
        (
            "dst_collections",
            322330,
            "collection",
            "trend",
            1,
            "",
            false,
        ),
        ("zomboid_items", 108600, "item", "trend", 1, "", false),
    ];

    tokio::time::timeout(Duration::from_secs(300), async {
        for locale in locales {
            let mut first_page_ids = Vec::new();
            for (case, app_id, kind, sort, page, query, empty) in cases {
                let started = Instant::now();
                let result = search_public_workshop_items(
                    app_id,
                    Some(query.to_string()),
                    Some(sort.to_string()),
                    Some(page),
                    Some((*locale).to_string()),
                    Some(kind.to_string()),
                )
                .await
                .unwrap_or_else(|error| panic!("{case}/{locale}: browse failed: {error}"));
                println!(
                    "{}",
                    json!({
                        "case": case,
                        "app": result.app_id,
                        "kind": result.browse_kind,
                        "sort": result.sort,
                        "page": result.page,
                        "locale": locale,
                        "count": result.items.len(),
                        "total": result.total_count,
                        "has_more": result.has_more,
                        "elapsed_ms": started.elapsed().as_millis(),
                    })
                );
                assert_eq!(result.app_id, app_id, "{case}/{locale}: requested app");
                assert_eq!(result.browse_kind, kind, "{case}/{locale}: requested kind");
                assert_eq!(result.sort, sort, "{case}/{locale}: requested sort");
                assert_eq!(result.page, page, "{case}/{locale}: requested page");
                assert_eq!(result.page_size, SEARCH_PAGE_SIZE);
                assert!(
                    result.items.iter().all(|item| {
                        item.consumer_app_id == Some(app_id)
                            && item.item_kind == kind
                            && item.status == "resolved"
                    }),
                    "{case}/{locale}: every result must belong to the requested catalog"
                );
                if empty {
                    assert!(result.items.is_empty(), "{case}/{locale}: no matches");
                    assert_eq!(result.total_count, Some(0), "{case}/{locale}: empty total");
                    assert!(!result.has_more, "{case}/{locale}: no following page");
                } else {
                    assert!(!result.items.is_empty(), "{case}/{locale}: public results");
                    assert!(
                        result.total_count.is_some_and(|total| total > 0),
                        "{case}/{locale}: nonzero total"
                    );
                }
                if case == "conan_trend_first" {
                    assert!(result.has_more, "{case}/{locale}: a second page exists");
                    first_page_ids = result.items.iter().map(|item| item.id.clone()).collect();
                } else if case == "conan_trend_next" {
                    let next_page_ids: Vec<_> =
                        result.items.iter().map(|item| item.id.clone()).collect();
                    // Use a boolean assertion so failure output does not disclose item IDs.
                    assert!(
                        first_page_ids != next_page_ids,
                        "{case}/{locale}: page two must differ from page one"
                    );
                }
            }
        }
    })
    .await
    .expect("the Workshop browse matrix must finish within 300 seconds");
}
