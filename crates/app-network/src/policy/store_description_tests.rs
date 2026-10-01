use super::*;

const PAYLOAD: &str = r#"{"ids":[{"appid":322330}],"context":{"language":"schinese","country_code":"CN"},"data_request":{"include_full_description":true}}"#;

fn description_url(payload: &str) -> String {
    let mut url =
        Url::parse("https://api.steampowered.com/IStoreBrowseService/GetItems/v1/").unwrap();
    url.query_pairs_mut().append_pair("input_json", payload);
    url.to_string()
}

#[test]
fn store_description_mapping_preserves_public_identity_language_and_request() {
    for payload in [
        PAYLOAD.to_string(),
        PAYLOAD.replace("schinese", "english").replace("CN", "US"),
    ] {
        let original = description_url(&payload);
        let china = original.replace("api.steampowered.com", "api.steamchina.com");
        let (key, values) = candidates(policy(), &original).unwrap();
        assert_eq!(key, "steam-store-description");
        assert_eq!(values, [original.clone(), china.clone()]);
        assert_eq!(
            ordered_candidates(
                policy(),
                &original,
                SourcePreference::ChinaFirst,
                &[],
                Instant::now()
            ),
            [china.clone(), original.clone()]
        );
        assert_eq!(
            ordered_candidates(
                policy(),
                &china,
                SourcePreference::InternationalFirst,
                &[],
                Instant::now()
            ),
            [original, china]
        );
    }
}

#[test]
fn store_description_rejects_extra_duplicate_private_and_invalid_json_fields() {
    for payload in [
        "{}".to_string(),
        format!("{PAYLOAD} trailing"),
        PAYLOAD.replace("322330", "0"),
        PAYLOAD.replace("322330", "-1"),
        PAYLOAD.replace("322330", "1.5"),
        PAYLOAD.replace("322330", "4294967296"),
        PAYLOAD.replace("322330", "\"322330\""),
        PAYLOAD.replace(r#"[{"appid":322330}]"#, "[]"),
        PAYLOAD.replace(
            r#"[{"appid":322330}]"#,
            r#"[{"appid":322330},{"appid":892970}]"#,
        ),
        PAYLOAD.replace(r#""appid":322330"#, r#""appid":322330,"appid":892970"#),
        PAYLOAD.replace(r#""appid":322330"#, r#""appid":322330,"packageid":1"#),
        PAYLOAD.replace("schinese", "japanese"),
        PAYLOAD.replace("CN", "cn"),
        PAYLOAD.replace(
            r#""language":"schinese""#,
            r#""language":"schinese","language":"english""#,
        ),
        PAYLOAD.replace(
            r#""country_code":"CN""#,
            r#""country_code":"CN","steamid":"76561198000000000""#,
        ),
        PAYLOAD.replace(
            r#""include_full_description":true"#,
            r#""include_full_description":false"#,
        ),
        PAYLOAD.replace(
            r#""include_full_description":true"#,
            r#""include_full_description":true,"include_assets":true"#,
        ),
        PAYLOAD.replace(
            r#""include_full_description":true"#,
            r#""include_full_description":true,"include_full_description":true"#,
        ),
        PAYLOAD.replacen('{', r#"{"access_token":"secret","#, 1),
        PAYLOAD.replacen('{', r#"{"ids":[{"appid":1}],"#, 1),
        format!("{}{PAYLOAD}", " ".repeat(4096)),
    ] {
        assert!(
            candidates(policy(), &description_url(&payload)).is_none(),
            "{payload}"
        );
    }
}

#[test]
fn store_description_rejects_unverified_paths_queries_credentials_and_normalization() {
    let original = description_url(PAYLOAD);
    for input in [
        original.replace("api.steampowered.com", "example.com"),
        original.replace("https://", "http://"),
        original.replace("api.steampowered.com", "user:pass@api.steampowered.com"),
        original.replace("api.steampowered.com", "api.steampowered.com:8443"),
        original.replace("/GetItems/v1/", "/GetItems/v2/"),
        original.replace("/GetItems/v1/", "/GetItems/v1"),
        original.replace("/IStoreBrowseService/", "/a/../IStoreBrowseService/"),
        original.replace("/IStoreBrowseService/", "/%2e/IStoreBrowseService/"),
        original.replace("?input_json=", "?%69nput_json="),
        format!("{original}&input_json=%7B%7D"),
        format!("{original}&access_token=secret"),
        format!("{original}&key=secret"),
        format!("{original}&X-Amz-Signature=secret"),
        format!("{original}&"),
        format!("{original}#fragment"),
        "https://api.steampowered.com/IStoreBrowseService/GetItems/v1/".to_string(),
        "https://api.steampowered.com/IStoreBrowseService/GetItems/v1/?input_json=".to_string(),
    ] {
        assert!(candidates(policy(), &input).is_none(), "{input}");
    }
}

#[test]
fn store_description_health_is_independent_from_other_steam_metadata() {
    let original = description_url(PAYLOAD);
    let china = original.replace("api.steampowered.com", "api.steamchina.com");
    let now = Instant::now();
    let mut failure = Health {
        key: "steam-public-metadata".into(),
        origin: "https://api.steamchina.com".into(),
        at: now,
        success: false,
    };
    assert_eq!(
        ordered_candidates(
            policy(),
            &original,
            SourcePreference::ChinaFirst,
            std::slice::from_ref(&failure),
            now
        ),
        [china.clone(), original.clone()]
    );
    failure.key = "steam-store-description".into();
    assert_eq!(
        ordered_candidates(
            policy(),
            &original,
            SourcePreference::ChinaFirst,
            &[failure],
            now
        ),
        [original, china]
    );
}
