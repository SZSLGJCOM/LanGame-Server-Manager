use super::*;

#[test]
fn external_url_rejects_invalid_schemes_authorities_and_ambiguous_input() {
    for input in [
        "",
        "   ",
        "https://",
        "https:example.com",
        "/relative",
        "//example.com",
        "file:///C:/fixture",
        "javascript:alert(1)",
        "https://user:password@example.com",
        "https://user@example.com",
        "https://example.com\\file",
        "https://exam\nple.com",
        "https://example.com/\0payload",
        "\thttps://example.com",
        "https://[::1",
        "https://example.com:70000",
        "https:///example.com",
    ] {
        assert!(parse_external_url(input).is_err(), "accepted {input:?}");
    }
    assert!(
        parse_external_url(&format!(
            "https://example.com/{}",
            "a".repeat(MAX_URL_BYTES)
        ))
        .is_err()
    );
}

#[test]
fn external_url_preserves_http_links_and_normalizes_the_authority() {
    for (input, expected) in [
        (
            "  HTTPS://Example.COM/news?id=42#details  ",
            "https://example.com/news?id=42#details",
        ),
        ("http://127.0.0.1:9088/", "http://127.0.0.1:9088/"),
        ("https://example.com/a b", "https://example.com/a%20b"),
        ("http://[::1]/", "http://[::1]/"),
    ] {
        assert_eq!(parse_external_url(input).unwrap().as_str(), expected);
    }
}
