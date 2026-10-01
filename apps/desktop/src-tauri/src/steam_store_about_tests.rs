use super::*;
use serde_json::json;

fn description(id: u64, about: &str) -> Value {
    json!({"success": true, "data": {"steam_appid": id, "about_the_game": about}})
}

#[tokio::test]
async fn regional_full_copy_does_not_depend_on_the_old_store_host() {
    let mut calls = Vec::new();
    let html = fetch_providers(Duration::from_secs(16), |primary, budget| {
        calls.push(primary);
        assert!(budget <= Duration::from_secs(8));
        std::future::ready(if primary {
            Ok(Some("Full regional description".to_owned()))
        } else {
            panic!("legacy store host must not be required")
        })
    })
    .await
    .unwrap();
    assert_eq!(html.as_deref(), Some("Full regional description"));
    assert_eq!(calls, [true]);
}

#[tokio::test]
async fn legacy_store_is_only_bounded_recovery_and_never_bypasses_restrictions() {
    for primary_error in [
        AboutError::Invalid("Invalid content".into()),
        AboutError::Unavailable("Unavailable".into()),
        AboutError::Network(app_network::NetworkError::Status {
            status: reqwest::StatusCode::TOO_MANY_REQUESTS,
            origin: "https://api.steamchina.com".into(),
        }),
    ] {
        let expected_recovery = primary_error.permits_fallback();
        let mut error = Some(primary_error);
        let mut calls = Vec::new();
        let result = fetch_providers(Duration::from_secs(16), |primary, budget| {
            calls.push(primary);
            assert!(budget <= Duration::from_secs(16));
            std::future::ready(if primary {
                Err(error.take().unwrap())
            } else {
                Ok(Some("Recovered".to_owned()))
            })
        })
        .await;
        assert_eq!(result.is_ok(), expected_recovery);
        assert_eq!(calls.len(), if expected_recovery { 2 } else { 1 });
    }
}

#[test]
fn matches_game_identity_when_live_envelope_uses_another_id() {
    for (requested, key) in [
        (322330, "1338540"),
        (1621690, "3398360"),
        (252490, "2511860"),
        (1623730, "2771110"),
    ] {
        let payload = json!({key: description(requested, "<p>Official game description.</p>")});
        // Official responses can use an envelope key different from the requested
        // app ID; the embedded app ID identifies the matching game's body.
        assert!(payload.get(requested.to_string()).is_none());
        assert_eq!(
            parse_about(&payload, requested, false).unwrap().as_deref(),
            Some("<p>Official game description.</p>")
        );
    }
}

#[test]
fn normal_envelope_preserves_about_and_localizes_review_heading() {
    let payload = json!({"322330": {"success": true, "data": {
        "steam_appid": 322330, "reviews": "<p>Review</p>", "about_the_game": " <p>Story</p> "
    }}});
    assert_eq!(
        parse_about(&payload, 322330, false).unwrap().unwrap(),
        "<h2>Reviews</h2><div class=\"steam-review-copy\"><p>Review</p></div><hr /><p>Story</p>"
    );
    assert!(
        parse_about(&payload, 322330, true)
            .unwrap()
            .unwrap()
            .starts_with("<h2>媒体评价</h2>")
    );
}

#[test]
fn rejects_wrong_missing_or_ambiguous_game_identity() {
    for payload in [
        json!({"322330": description(252490, "Wrong game")}),
        json!({"322330": {"success": true, "data": {"about_the_game": "Missing ID"}}}),
        json!({"322330": description(322330, "One"), "other": description(322330, "Two")}),
        json!({"other": {"success": true, "data": {"steam_appid": "322330", "about_the_game": "Invalid ID"}}}),
        json!([]),
        json!({}),
    ] {
        assert!(parse_about(&payload, 322330, false).is_err());
    }
}

#[test]
fn selects_matching_game_without_using_first_entry() {
    let payload =
        json!({"0": description(252490, "Wrong game"), "1": description(322330, "Correct game")});
    assert_eq!(
        parse_about(&payload, 322330, false).unwrap().as_deref(),
        Some("Correct game")
    );
}

#[test]
fn distinguishes_explicit_unavailable_and_empty_copy_from_invalid_identity() {
    assert_eq!(
        parse_about(&json!({"322330": {"success": false}}), 322330, false).unwrap(),
        None
    );
    assert_eq!(
        parse_about(
            &json!({"322330": description(322330, " \n ")}),
            322330,
            false
        )
        .unwrap(),
        None
    );
    assert!(parse_about(&json!({"other": {"success": false}}), 322330, false).is_err());
}

#[tokio::test]
async fn localized_failure_recovers_with_valid_alternative_under_one_budget() {
    let mut calls = Vec::new();
    let mut replies = std::collections::VecDeque::from([
        Err(AboutError::Invalid(invalid_identity(322330))),
        Ok(Some("<p>Verified alternative</p>".to_owned())),
    ]);
    let html = fetch_languages(
        Some("zh-CN"),
        Duration::from_secs(16),
        |cc, language, budget| {
            calls.push((cc, language, budget));
            std::future::ready(replies.pop_front().unwrap())
        },
    )
    .await
    .unwrap();
    assert_eq!(html.as_deref(), Some("<p>Verified alternative</p>"));
    assert_eq!(
        calls
            .iter()
            .map(|(cc, language, _)| (*cc, *language))
            .collect::<Vec<_>>(),
        [("cn", "schinese"), ("us", "english")]
    );
    assert!(
        calls
            .iter()
            .all(|(_, _, budget)| !budget.is_zero() && *budget <= Duration::from_secs(8))
    );
}

#[tokio::test]
async fn transport_failure_can_recover_without_caching_an_error_as_unavailable() {
    let mut calls = 0;
    let html = fetch_languages(Some("en-US"), Duration::from_secs(16), |_, language, _| {
        calls += 1;
        std::future::ready(if calls == 1 {
            assert_eq!(language, "english");
            Err(AboutError::Network(app_network::NetworkError::Deadline {
                attempts: 2,
                origin: "https://store.steampowered.com".into(),
            }))
        } else {
            assert_eq!(language, "schinese");
            Ok(Some("<p>恢复后的简介</p>".into()))
        })
    })
    .await
    .unwrap();
    assert_eq!(calls, 2);
    assert_eq!(html.as_deref(), Some("<p>恢复后的简介</p>"));
    let mut calls = 0;
    let result = fetch_languages(Some("en-US"), Duration::from_secs(16), |_, _, _| {
        calls += 1;
        std::future::ready(if calls == 1 {
            Err(AboutError::Invalid("invalid identity".into()))
        } else {
            Ok(None)
        })
    })
    .await;
    assert_eq!(result.unwrap_err(), "invalid identity");
}

#[tokio::test]
async fn authorization_throttling_and_server_deferral_never_try_another_language() {
    use app_network::NetworkError;
    use reqwest::StatusCode;
    for error in [
        NetworkError::Status {
            status: StatusCode::UNAUTHORIZED,
            origin: "https://store.steampowered.com".into(),
        },
        NetworkError::Status {
            status: StatusCode::FORBIDDEN,
            origin: "https://store.steampowered.com".into(),
        },
        NetworkError::Status {
            status: StatusCode::TOO_MANY_REQUESTS,
            origin: "https://store.steampowered.com".into(),
        },
        NetworkError::RetryDeferred {
            status: StatusCode::SERVICE_UNAVAILABLE,
            origin: "https://store.steampowered.com".into(),
            retry_after: Some(Duration::from_secs(120)),
        },
        NetworkError::BodyTooLarge {
            max_bytes: 8 * 1024 * 1024,
            origin: "https://store.steampowered.com".into(),
        },
    ] {
        let mut error = Some(error);
        let mut calls = 0;
        assert!(
            fetch_languages(Some("zh-CN"), Duration::from_secs(16), |_, _, _| {
                calls += 1;
                std::future::ready(Err(AboutError::Network(
                    error.take().expect("must not retry"),
                )))
            })
            .await
            .is_err()
        );
        assert_eq!(calls, 1);
    }
}

#[tokio::test]
async fn expired_budget_starts_no_requests_and_valid_empty_results_remain_empty() {
    assert!(
        fetch_languages(Some("zh-CN"), Duration::ZERO, |_, _, _| {
            panic!("expired budget issued a network request");
            #[allow(unreachable_code)]
            std::future::ready(Ok(None))
        })
        .await
        .is_err()
    );
    assert_eq!(
        fetch_languages(Some("zh-CN"), Duration::from_secs(16), |_, _, _| {
            std::future::ready(Ok(None))
        })
        .await
        .unwrap(),
        None
    );
}

#[tokio::test]
#[ignore = "Read-only official Steam network probe, run explicitly on the affected computer"]
async fn live_descriptions_resolve_in_both_languages() {
    for locale in ["zh-CN", "en-US"] {
        let html = fetch(322330, Some(locale))
            .await
            .unwrap()
            .expect("official description");
        assert!(!html.trim().is_empty());
        println!(
            "Steam description app=322330 locale={locale} html_bytes={}",
            html.len()
        );
    }
}
