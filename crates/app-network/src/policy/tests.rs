use super::*;

fn fixture() -> Policy {
    serde_json::from_str(r#"{
      "schemaVersion":1,
      "chinaOrigins":[],
      "groups":[{"id":"image","origins":["https://a.example","https://b.example"],"pathPrefixes":["/media/"],"allowedQueryKeys":["t","size"]}],
      "exactResources":[{"id":"installer","urls":["https://a.example/installer.zip","https://b.example/download/tool.zip"]}]
    }"#).unwrap()
}

#[test]
fn equivalent_urls_preserve_resource_path_and_query() {
    let (_, urls) = candidates(
        &fixture(),
        "https://a.example/media/screenshot.jpg?t=123&size=400",
    )
    .unwrap();
    assert_eq!(
        urls,
        [
            "https://a.example/media/screenshot.jpg?t=123&size=400",
            "https://b.example/media/screenshot.jpg?t=123&size=400"
        ]
    );
    assert_eq!(
        candidates(&fixture(), "https://b.example/installer.zip"),
        None
    );
    assert_eq!(
        candidates(&fixture(), "https://a.example/installer.zip")
            .unwrap()
            .1,
        [
            "https://a.example/installer.zip",
            "https://b.example/download/tool.zip"
        ]
    );
}

#[test]
fn unknown_private_signed_and_unrelated_urls_are_not_rewritten() {
    for url in [
        "https://unknown.example/media/a.png",
        "https://a.example/account/profile",
        "http://a.example/media/a.png",
        "https://user:pass@a.example/media/a.png",
        "https://a.example:8443/media/a.png",
        "https://a.example/media/a.png#fragment",
        "https://a.example/media/a.png?X-Amz-Signature=secret",
        "https://a.example/media/a.png?token=secret",
        "https://a.example/media/a.png?key=secret",
        "https://a.example/media/a.png?expires=123",
    ] {
        assert!(candidates(&fixture(), url).is_none(), "{url}");
    }
}

#[test]
fn bundled_policy_has_bounded_official_groups_and_primary_evidence() {
    let document: serde_json::Value =
        serde_json::from_str(include_str!("../../official-sources.json")).unwrap();
    let policy = policy();
    assert_eq!(policy.schema_version, 1);
    assert!(!policy.groups.is_empty());
    assert!(!policy.china_origins.is_empty());
    let mut china_origins = std::collections::HashSet::new();
    for origin in &policy.china_origins {
        assert!(china_origins.insert(origin));
        let url = Url::parse(origin).unwrap();
        assert!(public_https(&url));
        assert_eq!(url.origin().ascii_serialization(), *origin);
        assert!(
            policy
                .groups
                .iter()
                .any(|group| group.origins.contains(origin))
        );
    }
    let mut ids = std::collections::HashSet::new();
    for group in &policy.groups {
        assert!(ids.insert(&group.id));
        assert!((2..=8).contains(&group.origins.len()));
        assert!(!group.path_prefixes.is_empty() || !group.exact_paths.is_empty());
        assert!(
            group
                .path_prefixes
                .iter()
                .all(|prefix| prefix.starts_with('/') && prefix.ends_with('/') && prefix.len() > 1)
        );
        for origin in &group.origins {
            let url = Url::parse(origin).unwrap();
            assert!(public_https(&url));
            assert_eq!(url.origin().ascii_serialization(), *origin);
        }
        if !group.base_paths.is_empty() {
            assert_eq!(group.base_paths.len(), group.origins.len());
            assert!(group.path_prefixes.is_empty());
            for origin in &group.origins {
                let base = &group.base_paths[origin];
                assert!(base.starts_with('/') && base.ends_with('/'));
                assert!(!base.contains("..") && !base.contains('%'));
            }
        }
        if !group.filename_patterns.is_empty() {
            assert!(!group.base_paths.is_empty());
            assert!(group.filename_patterns.iter().all(|pattern| {
                !pattern.prefix.is_empty() && pattern.extension.starts_with('.')
            }));
        }
    }
    for resource in &policy.exact_resources {
        assert!(ids.insert(&resource.id));
        assert!((2..=8).contains(&resource.urls.len()));
        assert!(
            resource
                .urls
                .iter()
                .all(|value| public_https(&Url::parse(value).unwrap()))
        );
    }
    for entry in document["groups"]
        .as_array()
        .unwrap()
        .iter()
        .chain(document["exactResources"].as_array().unwrap())
    {
        assert!(!entry["purposes"].as_array().unwrap().is_empty());
        assert!(!entry["evidence"].as_array().unwrap().is_empty());
    }
}

#[test]
fn authentication_and_store_apis_never_use_the_public_metadata_alternate() {
    for url in [
        "https://api.steampowered.com/ISteamUserAuth/AuthenticateUser/v1/",
        "https://store.steampowered.com/api/appdetails?appids=322330",
        "https://api.steampowered.com/ISteamRemoteStorage/GetPublishedFileDetails/v1/private",
        "https://api.steampowered.com/ISteamNews/GetNewsForApp/v2/?api_key=secret",
        "https://api.steampowered.com/ISteamRemoteStorage/GetPublishedFileDetails/v1/?key=secret",
    ] {
        for preference in [
            SourcePreference::ChinaFirst,
            SourcePreference::InternationalFirst,
        ] {
            assert_eq!(official_url_candidates(url, preference), [url]);
        }
    }
}

#[test]
fn a_verified_success_is_preferred_and_a_failure_has_a_bounded_cooldown() {
    let initial = "https://piston-data.mojang.com/v1/objects/network-policy-test/server.jar";
    let alternate = "https://launcher.mojang.com/v1/objects/network-policy-test/server.jar";
    record_success(alternate);
    assert_eq!(
        official_url_candidates(initial, SourcePreference::InternationalFirst)
            .first()
            .map(String::as_str),
        Some(alternate)
    );
    record_failure(alternate);
    assert_eq!(
        official_url_candidates(initial, SourcePreference::InternationalFirst)
            .first()
            .map(String::as_str),
        Some(initial)
    );
}

fn regional_fixture() -> Policy {
    let mut policy = fixture();
    policy
        .china_origins
        .push(String::from("https://cn.example"));
    policy.groups[0]
        .origins
        .push(String::from("https://cn.example"));
    policy
}

fn source_health(origin: &str, at: Instant, success: bool) -> Health {
    Health {
        key: String::from("image"),
        origin: origin.to_string(),
        at,
        success,
    }
}

#[test]
fn locale_preference_only_recognizes_the_chinese_language_tag() {
    for locale in ["zh", "ZH", "zh-CN", "Zh-TW", "ZH-hans-CN"] {
        assert_eq!(
            SourcePreference::from_locale(Some(locale)),
            SourcePreference::ChinaFirst
        );
    }
    for locale in [
        None,
        Some(""),
        Some("en"),
        Some("EN-US"),
        Some("ja"),
        Some("zh_CN"),
        Some("zhuang"),
    ] {
        assert_eq!(
            SourcePreference::from_locale(locale),
            SourcePreference::InternationalFirst
        );
    }
}

#[test]
fn request_preference_selects_the_region_even_when_the_original_url_is_opposite() {
    let policy = regional_fixture();
    let now = Instant::now();
    let input = "https://a.example/media/screenshot.jpg?t=123&size=400";
    let china = "https://cn.example/media/screenshot.jpg?t=123&size=400";
    let alternate = "https://b.example/media/screenshot.jpg?t=123&size=400";
    assert_eq!(
        ordered_candidates(&policy, input, SourcePreference::ChinaFirst, &[], now),
        [china, input, alternate]
    );
    assert_eq!(
        ordered_candidates(
            &policy,
            china,
            SourcePreference::InternationalFirst,
            &[],
            now
        ),
        [input, alternate, china]
    );
    // Equal-ranking international candidates retain their original stable order.
    assert_eq!(
        ordered_candidates(
            &policy,
            alternate,
            SourcePreference::InternationalFirst,
            &[],
            now
        ),
        [alternate, input, china]
    );
}

#[test]
fn success_from_another_region_never_overrides_this_requests_language() {
    let policy = regional_fixture();
    let now = Instant::now();
    let input = "https://a.example/media/a.jpg";
    let alternate = "https://b.example/media/a.jpg";
    let china = "https://cn.example/media/a.jpg";
    let international_success = [source_health("https://b.example", now, true)];
    assert_eq!(
        ordered_candidates(
            &policy,
            input,
            SourcePreference::ChinaFirst,
            &international_success,
            now
        ),
        [china, alternate, input]
    );
    let china_success = [source_health("https://cn.example", now, true)];
    assert_eq!(
        ordered_candidates(
            &policy,
            china,
            SourcePreference::InternationalFirst,
            &china_success,
            now
        ),
        [input, alternate, china]
    );
}

#[test]
fn a_failed_preferred_region_is_a_backup_until_the_exact_cooldown_boundary() {
    let policy = regional_fixture();
    let at = Instant::now();
    let input = "https://a.example/media/a.jpg";
    let alternate = "https://b.example/media/a.jpg";
    let china = "https://cn.example/media/a.jpg";
    let entries = [
        source_health("https://cn.example", at, false),
        source_health("https://b.example", at, true),
    ];
    assert_eq!(
        ordered_candidates(
            &policy,
            input,
            SourcePreference::ChinaFirst,
            &entries,
            at + FAILURE_TTL - Duration::from_nanos(1)
        ),
        [alternate, input, china]
    );
    assert_eq!(
        ordered_candidates(
            &policy,
            input,
            SourcePreference::ChinaFirst,
            &entries,
            at + FAILURE_TTL
        ),
        [china, alternate, input]
    );
    // International preference is symmetric when both international origins fail.
    let entries = [
        source_health("https://a.example", at, false),
        source_health("https://b.example", at, false),
    ];
    assert_eq!(
        ordered_candidates(
            &policy,
            input,
            SourcePreference::InternationalFirst,
            &entries,
            at
        ),
        [china, input, alternate]
    );
    assert_eq!(
        ordered_candidates(
            &policy,
            input,
            SourcePreference::InternationalFirst,
            &entries,
            at + FAILURE_TTL
        ),
        [input, alternate, china]
    );
}

#[test]
fn same_region_success_expires_without_permanently_reordering_sources() {
    let policy = regional_fixture();
    let at = Instant::now();
    let input = "https://a.example/media/a.jpg";
    let alternate = "https://b.example/media/a.jpg";
    let china = "https://cn.example/media/a.jpg";
    let entries = [source_health("https://b.example", at, true)];
    assert_eq!(
        ordered_candidates(
            &policy,
            input,
            SourcePreference::InternationalFirst,
            &entries,
            at + SUCCESS_TTL - Duration::from_nanos(1)
        ),
        [alternate, input, china]
    );
    assert_eq!(
        ordered_candidates(
            &policy,
            input,
            SourcePreference::InternationalFirst,
            &entries,
            at + SUCCESS_TTL
        ),
        [input, alternate, china]
    );
}

#[test]
fn groups_without_china_sources_keep_existing_sources_and_health_order() {
    let policy = fixture();
    let now = Instant::now();
    let input = "https://a.example/media/a.jpg";
    let alternate = "https://b.example/media/a.jpg";
    let entries = [source_health("https://b.example", now, true)];
    for preference in [
        SourcePreference::ChinaFirst,
        SourcePreference::InternationalFirst,
    ] {
        assert_eq!(
            ordered_candidates(&policy, input, preference, &[], now),
            [input, alternate]
        );
        assert_eq!(
            ordered_candidates(&policy, input, preference, &entries, now),
            [alternate, input]
        );
        assert_eq!(
            ordered_candidates(
                &policy,
                "https://a.example/installer.zip",
                preference,
                &[],
                now
            ),
            [
                "https://a.example/installer.zip",
                "https://b.example/download/tool.zip"
            ]
        );
    }
}

#[test]
fn region_preference_does_not_expand_unverified_or_credential_bearing_urls() {
    let policy = regional_fixture();
    for input in [
        "https://unknown.example/media/a.jpg",
        "https://a.example/private/a.jpg",
        "https://a.example/media/a.jpg?token=secret",
        "https://a.example/media/a.jpg?X-Amz-Signature=secret",
        concat!("https://user:", "password@", "a.example/media/a.jpg"),
        "https://a.example:8443/media/a.jpg",
    ] {
        for preference in [
            SourcePreference::ChinaFirst,
            SourcePreference::InternationalFirst,
        ] {
            assert_eq!(
                ordered_candidates(&policy, input, preference, &[], Instant::now()),
                [input]
            );
        }
    }
}

#[test]
fn steamcmd_update_candidates_translate_each_official_base_path() {
    let bases = [
        "https://client-update.steamstatic.com/",
        "https://cdn.fastly.steamstatic.com/client/",
        "https://steamcdn-a.akamaihd.net/client/",
        "https://media.steampowered.com/client/",
    ];
    for name in [
        "steam_cmd_win64",
        "steamcmd_public_all.zip",
        "steamcmd_bins_win64.zip.0123456789abcdef",
    ] {
        for base in bases {
            let input = format!("{base}{name}");
            let (key, actual) = candidates(policy(), &input).unwrap();
            let mut expected = vec![input.clone()];
            expected.extend(
                bases
                    .iter()
                    .filter(|value| **value != base)
                    .map(|value| format!("{value}{name}")),
            );
            assert_eq!(key, "steamcmd-native-update");
            assert_eq!(actual, expected);
        }
    }
}

#[test]
fn steamcmd_update_mapping_rejects_unrelated_encoded_signed_and_traversal_paths() {
    for input in [
        "https://client-update.steamstatic.com/steam_cmd_win32",
        "https://client-update.steamstatic.com/client/steam_cmd_win64",
        "https://cdn.fastly.steamstatic.com/steam_cmd_win64",
        "https://cdn.fastly.steamstatic.com/client/config.vdf",
        "https://client-update.steamstatic.com/steam_cmd_win64?",
        "https://client-update.steamstatic.com/steam_cmd_win64?token=secret",
        "https://client-update.steamstatic.com/steamcmd_public_all.zip#fragment",
        "https://client-update.steamstatic.com/steamcmd_public_all.zip?x=1",
        "https://client-update.steamstatic.com/a/../steam_cmd_win64",
        "https://client-update.steamstatic.com/./steam_cmd_win64",
        "https://cdn.fastly.steamstatic.com/client/%2e%2e/client/steam_cmd_win64",
        "https://client-update.steamstatic.com/%73team_cmd_win64",
        "https://client-update.steamstatic.com/steamcmd_foo%2ezip",
        "https://client-update.steamstatic.com/steamcmd_foo/../steamcmd_bar.zip",
        "https://client-update.steamstatic.com/steamcmd_foo\\..\\steamcmd_bar.zip",
        "https://client-update.steamstatic.com/steamcmd_foo.zip/extra",
        "https://client-update.steamstatic.com/steamcmd_foo..zip",
        "https://client-update.steamstatic.com/steamcmd_foo.zip.",
        "https://client-update.steamstatic.com/steamcmd_foo.zipbad",
        "https://client-update.steamstatic.com/steamcmd_.zip",
        "https://client-update.steamstatic.com/steamcmd_foo.exe",
        "https://unknown.example/steam_cmd_win64",
        "https://user:pass@client-update.steamstatic.com/steam_cmd_win64",
        "https://client-update.steamstatic.com:8443/steam_cmd_win64",
    ] {
        assert!(candidates(policy(), input).is_none(), "{input}");
    }
}
