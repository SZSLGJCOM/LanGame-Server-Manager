use super::*;
use serde_json::json;

pub(super) const API: &str =
    "https://enshrouded.zendesk.com/api/v2/help_center/en-us/articles/16055441447709.json";
const CANONICAL: &str = "https://enshrouded.zendesk.com/hc/en-us/articles/16055441447709-Dedicated-Server-Configuration";

pub(super) fn source() -> Source {
    Source {
        id: "keen-server-help".into(),
        title: "Dedicated server help".into(),
        authority: "Keen Games".into(),
        kind: "official".into(),
        seeds: vec![API.into()],
        allowed_prefixes: Vec::new(),
        discover_links: false,
        reference_only: false,
        max_pages: 1,
        content_selector: None,
        discovery_selector: None,
        sitemaps: Vec::new(),
        authority_evidence: CANONICAL.into(),
        license_note: "Public publisher documentation".into(),
        license_url: None,
        reviewed_on: "2026-09-28".into(),
    }
}

pub(super) fn response() -> Value {
    json!({"article": {
        "id": 16055441447709_u64,
        "locale": "en-us",
        "draft": false,
        "title": "Dedicated Server Configuration",
        "body": "<p>Publisher instructions for running a dedicated server.</p>",
        "html_url": CANONICAL,
        "user_segment_id": null
    }})
}

fn decode(value: &Value) -> Result<PublicArticle> {
    parse(
        &source(),
        &Url::parse(API).unwrap(),
        "application/json; charset=utf-8",
        &serde_json::to_vec(value).unwrap(),
    )
}

#[test]
fn published_anonymous_article_preserves_official_citation_and_html() {
    let value = response();
    let decoded = decode(&value).unwrap();
    assert_eq!(decoded.title, "Dedicated Server Configuration");
    assert_eq!(decoded.html, value["article"]["body"].as_str().unwrap());
    assert_eq!(decoded.canonical_url.as_str(), CANONICAL);
    let mut enterprise = value;
    enterprise["article"]
        .as_object_mut()
        .unwrap()
        .remove("user_segment_id");
    enterprise["article"]["user_segment_ids"] = json!([]);
    assert!(decode(&enterprise).is_ok());
}

#[test]
fn citation_uses_the_stable_official_route_only_for_reviewed_api_seeds() {
    assert_eq!(
        citation_url(&source(), &Url::parse(API).unwrap())
            .unwrap()
            .as_str(),
        "https://enshrouded.zendesk.com/hc/en-us/articles/16055441447709"
    );
    for unreviewed in [
        API.replace("16055441447709", "16055441447710"),
        API.replace("enshrouded.zendesk.com", "other.zendesk.com"),
        format!("{API}?extra=1"),
        CANONICAL.to_owned(),
    ] {
        assert!(citation_url(&source(), &Url::parse(&unreviewed).unwrap()).is_none());
    }
}

#[test]
fn extraction_cleans_html_preserves_technical_text_and_never_discovers_links() {
    let mut value = response();
    value["article"]["body"] = json!(
        r#"
        <h1>Configuration details</h1>
        <p>Set the query port before starting the server.</p>
        <pre><code>{"queryPort":15637,"slotCount":16}</code></pre>
        <p>Keep values &lt; 65536. <a href="https://example.com/unreviewed">Related topic</a></p>
        <script>script_noise()</script><style>.style_noise{color:red}</style>
        <nav>navigation_noise</nav><button>button_noise</button>
        <textarea>textarea_noise</textarea><input value="input_noise">
    "#
    );
    let extracted = crate::extract::extract(
        &source(),
        &Url::parse(API).unwrap(),
        "application/json",
        &serde_json::to_vec(&value).unwrap(),
    )
    .unwrap();
    assert_eq!(extracted.title, "Dedicated Server Configuration");
    assert!(
        extracted
            .body
            .contains("{\"queryPort\":15637,\"slotCount\":16}")
    );
    assert!(extracted.body.contains("Keep values < 65536."));
    assert!(extracted.body.contains("Configuration details"));
    assert!(extracted.body.contains("Related topic"));
    assert!(!extracted.body.contains("_noise"));
    assert!(extracted.links.is_empty());
}

#[test]
fn article_body_robots_metadata_still_prevents_indexing() {
    let mut value = response();
    value["article"]["body"] = json!(
        "<meta name=\"ROBOTS\" content=\"noindex,follow\"><p>Publisher-only configuration instructions.</p>"
    );
    assert!(matches!(
        crate::extract::extract(
            &source(),
            &Url::parse(API).unwrap(),
            "application/json",
            &serde_json::to_vec(&value).unwrap(),
        ),
        Err(KnowledgeError::Policy(_))
    ));
}

#[test]
fn cancelled_article_extraction_stops_before_parsing() {
    assert!(matches!(
        extract_article(
            &source(),
            &Url::parse(API).unwrap(),
            "application/json",
            b"invalid JSON",
            &std::sync::atomic::AtomicBool::new(true),
            std::time::Instant::now(),
        ),
        Err(KnowledgeError::Cancelled)
    ));
}

#[test]
fn api_admission_requires_an_exact_reviewed_publisher_article() {
    let mut source = source();
    source.allowed_prefixes = vec!["https://enshrouded.zendesk.com/".into()];
    for url in [
        API.replace("16055441447709", "16055441447710"),
        API.replace("en-us", "de"),
        API.replace("enshrouded.zendesk.com", "other.zendesk.com"),
        API.replace("/articles/", "/sections/"),
        API.replace("16055441447709", "016055441447709"),
        API.replace(
            "16055441447709",
            "%31%36%30%35%35%34%34%31%34%34%37%37%30%39",
        ),
        format!("{API}?include=drafts"),
        format!("{API}#fragment"),
        CANONICAL.into(),
    ] {
        let parsed = Url::parse(&url).unwrap();
        assert!(!is_article_url(&source, &parsed), "{url}");
        // Even an otherwise reviewed seed cannot authorize another API kind.
        if !url.contains("16055441447710") {
            let mut changed = source.clone();
            changed.seeds.push(url.clone());
            assert!(!is_article_url(&changed, &parsed), "{url}");
        }
    }
    source.reference_only = true;
    assert!(!is_article_url(&source, &Url::parse(API).unwrap()));
}

#[test]
fn draft_private_and_missing_public_visibility_metadata_are_rejected() {
    for patch in [
        json!({"draft": true}),
        json!({"user_segment_id": 42}),
        json!({"user_segment_ids": [42]}),
        json!({"user_segment_id": "public"}),
        json!({"user_segment_ids": {}}),
    ] {
        let mut value = response();
        value["article"]
            .as_object_mut()
            .unwrap()
            .extend(patch.as_object().unwrap().clone());
        assert!(matches!(decode(&value), Err(KnowledgeError::Policy(_))));
    }
    let mut value = response();
    value["article"]
        .as_object_mut()
        .unwrap()
        .remove("user_segment_id");
    assert!(matches!(decode(&value), Err(KnowledgeError::Policy(_))));
    value["article"]["user_segment_ids"] = Value::Null;
    assert!(matches!(decode(&value), Err(KnowledgeError::Policy(_))));
}

#[test]
fn identity_locale_and_canonical_cannot_escape_the_reviewed_article() {
    for patch in [
        json!({"id": 16055441447710_u64}),
        json!({"locale": "de"}),
        json!({"html_url": CANONICAL.replace("enshrouded.zendesk.com", "other.zendesk.com")}),
        json!({"html_url": CANONICAL.replace("16055441447709", "160554414477090")}),
        json!({"html_url": CANONICAL.replace("/en-us/", "/de/")}),
        json!({"html_url": format!("{CANONICAL}/extra")}),
        json!({"html_url": format!("{CANONICAL}?redirect=https://example.com/")}),
        json!({"html_url": format!("{CANONICAL}#fragment")}),
        json!({"html_url": CANONICAL.replace("-Dedicated", "%2fDedicated")}),
    ] {
        let mut value = response();
        value["article"]
            .as_object_mut()
            .unwrap()
            .extend(patch.as_object().unwrap().clone());
        assert!(decode(&value).is_err(), "{patch}");
    }
}

#[test]
fn malformed_missing_and_oversized_bodies_cannot_become_documents() {
    for field in ["id", "locale", "draft", "title", "body", "html_url"] {
        let mut value = response();
        value["article"].as_object_mut().unwrap().remove(field);
        assert!(decode(&value).is_err(), "{field}");
    }
    for body in [Value::Null, json!(" \n\t"), json!({"text": "body"})] {
        let mut value = response();
        value["article"]["body"] = body;
        assert!(decode(&value).is_err());
    }
    let mut value = response();
    value["article"]["body"] = json!("x".repeat(crate::extract::MAX_TEXT));
    assert!(decode(&value).is_err());
    for bytes in [b"not JSON".as_slice(), b"{\"article\":null}", b"[]"] {
        assert!(
            parse(
                &source(),
                &Url::parse(API).unwrap(),
                "application/json",
                bytes
            )
            .is_err()
        );
    }
    assert!(parse(&source(), &Url::parse(API).unwrap(), "text/html", b"{}").is_err());
}

#[test]
fn duplicate_permission_fields_are_not_a_last_wins_public_override() {
    let value = serde_json::to_string(&response()).unwrap().replace(
        "\"user_segment_id\":null",
        "\"user_segment_id\":42,\"user_segment_id\":null",
    );
    assert!(
        parse(
            &source(),
            &Url::parse(API).unwrap(),
            "application/json",
            value.as_bytes()
        )
        .is_err()
    );
}
