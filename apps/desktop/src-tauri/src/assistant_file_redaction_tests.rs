use super::*;

#[test]
fn file_redaction_rejects_duplicate_sensitive_keys_before_value_collapse() {
    let credential = ["synthetic-test-only", "duplicate", "value"].join("-");
    let masked = serde_json::to_string(ASSISTANT_REDACTED_VALUE).unwrap();
    for text in [
        format!(r#"{{"password":"{credential}","password":{masked}}}"#),
        format!(r#"{{"nested":[{{"token":"{credential}","token":{masked}}}]}}"#),
        format!(r#"{{"pass\u0077ord":"{credential}","password":{masked}}}"#),
    ] {
        let redacted = redact_assistant_file_text("options", &text);
        assert!(!redacted.contains(&credential));
        assert!(redacted.contains(ASSISTANT_REDACTED_VALUE));
        serde_json::from_str::<Value>(&redacted).unwrap();
    }
}

#[test]
fn file_redaction_discards_shadowed_ordinary_values_with_credentials() {
    let credential = ["synthetic-test-only", "shadowed", "value"].join("-");
    for text in [
        format!(r#"{{"description":"password={credential}","description":"public"}}"#),
        format!(r#"{{"nested":[{{"value":"token={credential}","value":"public"}}]}}"#),
        format!(r#"{{"value":"token={credential}","v\u0061lue":"public"}}"#),
    ] {
        let redacted = redact_assistant_file_text("options", &text);
        assert!(!redacted.contains(&credential));
        assert!(redacted.contains("public"));
    }
}

#[test]
fn file_redaction_preserves_safe_json_key_order_spacing_and_numeric_spelling() {
    let text = " {\r\n  \"z\": [null, true, false, -1, 18446744073709551615, 1.20e2],\r\n  \"a\": \"quoted \\\"text\\\"\",\r\n  \"nested\": {\"z\": 2, \"a\": 1}\r\n}\r\n";
    assert_eq!(redact_assistant_file_text("options", text), text);
    assert_eq!(
        redact_assistant_file_text("options", &format!("\u{feff}{text}")),
        format!("\u{feff}{text}")
    );
}

#[test]
fn file_redaction_parses_bom_json_before_decoding_sensitive_escaped_keys() {
    let credential = ["synthetic-test-only", "bom", "value"].join("-");
    let text = format!("\u{feff}{{\"pass\\u0077ord\":\"{credential}\",\"ordinary\":1}}\r\n");
    let redacted = redact_assistant_file_text("options", &text);
    assert!(!redacted.contains(&credential));
    assert!(redacted.contains(ASSISTANT_REDACTED_VALUE));
    assert_eq!(
        serde_json::from_str::<Value>(&redacted).unwrap()["ordinary"],
        1
    );
}

#[test]
fn file_redaction_sensitive_file_name_masks_the_complete_document() {
    let credential = ["synthetic-test-only", "file", "value"].join("-");
    assert_eq!(
        redact_assistant_file_text("cluster_token", &credential),
        ASSISTANT_REDACTED_VALUE
    );
}

#[test]
fn file_redaction_preserves_safe_lua_and_mixed_line_endings() {
    for text in [
        "local value = 1\r\nreturn value\r\n",
        "first\r\nsecond\nthird",
        "\r\n \t\n",
    ] {
        assert_eq!(redact_assistant_file_text("modmain", text), text);
    }
}

#[test]
fn file_redaction_masks_yaml_blocks_without_normalizing_unrelated_line_endings() {
    let credential = ["synthetic-test-only", "yaml", "value"].join("-");
    let text =
        format!("name: public\r\ntoken: |\r\n  {credential}\n  \r\n  {credential}\r\nvalue: 1\n");
    let redacted = redact_assistant_file_text("options", &text);
    assert!(!redacted.contains(&credential));
    assert!(redacted.starts_with("name: public\r\n"));
    assert!(redacted.ends_with("value: 1\n"));
    assert!(redacted.contains(&format!(
        "  {ASSISTANT_REDACTED_VALUE}\n  \r\n  {ASSISTANT_REDACTED_VALUE}\r\n"
    )));
}

#[test]
fn duplicate_json_fallback_does_not_expose_a_credential_on_any_page() {
    let credential = ["synthetic-test-only", "paged", "value"].join("-");
    let masked = serde_json::to_string(ASSISTANT_REDACTED_VALUE).unwrap();
    let original = serde_json::to_string(&format!("{}{credential}", " ".repeat(4096))).unwrap();
    let text = format!(r#"{{"password":{original},"password":{masked}}}"#);
    let redacted = redact_assistant_file_text("options", &text);
    for page in redacted.as_bytes().chunks(4096) {
        assert!(!String::from_utf8_lossy(page).contains(&credential));
    }
    assert!(!redacted.contains(&credential));
}

#[test]
fn assignment_redaction_retains_no_suffix_after_escaped_quotes() {
    for source in [
        r#"password="prefix\"synthetic-secret-suffix""#,
        r#"password='prefix\'synthetic-secret-suffix'"#,
        r#"password="prefix""synthetic-secret-suffix""#,
        r#"password='prefix''synthetic-secret-suffix'"#,
        r#"password="prefix\"synthetic-secret-suffix"#,
    ] {
        for redacted in [
            redact_assistant_provider_text(source),
            redact_assistant_file_text("options", source),
        ] {
            assert_eq!(redacted, format!("password={ASSISTANT_REDACTED_VALUE}"));
        }
    }
}

#[test]
fn assignment_redaction_does_not_treat_an_escaped_backslash_as_an_escaped_quote() {
    let source = format!(r#"{}="prefix\\",ordinary=value"#, "password");
    assert_eq!(
        redact_assistant_provider_text(&source),
        format!("password={ASSISTANT_REDACTED_VALUE},ordinary=value")
    );
}

#[test]
fn yaml_redaction_masks_multiline_values_with_comments_tags_and_escaped_quotes() {
    for prefix in [
        "token: | # credential follows",
        "token: !tag &credential >- # credential follows",
        r#"token: "prefix\"unfinished"#,
        "token: 'prefix''unfinished",
    ] {
        let source = format!("{prefix}\r\n  synthetic-secret-suffix\r\npublic: value\n");
        for redacted in [
            redact_assistant_provider_text(&source),
            redact_assistant_file_text("options", &source),
        ] {
            assert!(!redacted.contains("synthetic-secret-suffix"));
            assert!(redacted.ends_with("public: value\n"));
        }
    }
}

#[test]
fn path_redaction_handles_many_matches_without_recursive_stack_growth() {
    let source = r#""C:\private\a","/private/b","\\host\private\c","#.repeat(4096);
    assert!(source.len() < 256 * 1024);
    let redacted = std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(move || redact_assistant_file_text("options", &source))
        .unwrap()
        .join()
        .unwrap();
    assert!(!redacted.contains("private"));
    assert_eq!(redacted.matches("[REDACTED_PATH]").count(), 3 * 4096);
}
