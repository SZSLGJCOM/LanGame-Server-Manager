use super::*;

fn document(file: &str, content: &str) -> app_storage::InstanceWorkspaceDocument {
    app_storage::InstanceWorkspaceDocument {
        entry: app_storage::InstanceWorkspaceEntry {
            file: file.into(),
            byte_length: content.len() as u64,
            editable: true,
            protection_reason: None,
        },
        source_sha256: "a".repeat(64),
        content: content.into(),
    }
}

#[test]
fn workspace_validation_reports_syntax_and_explicit_unsupported_formats() {
    for (file, content, status) in [
        ("data/plugins/valid.json", "{\"limit\": 4}", "valid"),
        ("data/plugins/invalid.json", "{\"limit\": }", "invalid"),
        ("data/plugins/valid.toml", "limit = 4\n", "valid"),
        ("data/plugins/invalid.toml", "limit =\n", "invalid"),
        ("data/plugins/code.lua", "this is not Lua", "unsupported"),
    ] {
        let result = assistant_workspace_validation_evidence(document(file, content)).unwrap();
        assert_eq!(result["status"], status);
        assert_eq!(result["scope"], "syntax_only");
        assert_eq!(result["sourceSha256"], "a".repeat(64));
        assert_eq!(
            result["issues"].as_array().unwrap().len(),
            usize::from(status == "invalid")
        );
    }
}

#[test]
fn workspace_validation_never_returns_source_or_parser_error_text() {
    let secret = "synthetic-fixture";
    for (file, content) in [
        (
            "data/plugins/options.json",
            format!("{{\"password\": \"{secret}\", \"broken\": }}"),
        ),
        (
            "data/plugins/options.toml",
            format!("password = '{secret}'\nvalue =\n"),
        ),
        (
            "data/plugins/options.json",
            format!("{{\"{secret}\": invalid}}"),
        ),
    ] {
        let result = assistant_workspace_validation_evidence(document(file, &content)).unwrap();
        assert_eq!(result["status"], "invalid");
        assert!(!result.to_string().contains(secret));
        assert!(result["issues"][0]["line"].as_u64().is_some());
        assert!(result["issues"][0]["column"].as_u64().is_some());
    }
}
