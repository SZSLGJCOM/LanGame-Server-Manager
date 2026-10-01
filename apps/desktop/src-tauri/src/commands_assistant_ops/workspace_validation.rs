pub(super) async fn validate_assistant_workspace_file(
    state: &DesktopState,
    storage: &StorageBootstrap,
    instance: &InstanceDetails,
    file: &str,
) -> Result<Value, String> {
    let _lease = state.begin_storage_context_operation("assistant workspace validation")?;
    ensure_storage_context_snapshot_current(state, storage, "assistant workspace validation")?;
    let workspace = app_storage::open_instance_workspace(&storage.paths, &instance.summary.id)
        .await
        .map_err(|error| redact_assistant_provider_text(&error.to_string()))?;
    let file = file.to_owned();
    run_assistant_evidence_read(state, move || {
        let document = workspace
            .read_file(&file)
            .map_err(|error| redact_assistant_provider_text(&error.to_string()))?;
        assistant_workspace_validation_evidence(document)
    })
    .await
}

fn assistant_workspace_validation_evidence(
    document: app_storage::InstanceWorkspaceDocument,
) -> Result<Value, String> {
    // Redaction is evaluated over the full document before any evidence leaves
    // this worker. Parsing uses original bytes; parser errors never expose text.
    let redacted =
        redact_assistant_file_content(Path::new(&document.entry.file), &document.content)?;
    let format = Path::new(&document.entry.file)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let mut issues = Vec::new();
    let status = match format.as_str() {
        "json" => match serde_json::from_str::<Value>(&document.content) {
            Ok(_) => "valid",
            Err(error) => {
                issues.push(
                    json!({"code":"json_syntax", "message":"JSON syntax is invalid.",
                    "line":error.line(), "column":error.column()}),
                );
                "invalid"
            }
        },
        "toml" => match toml::from_str::<toml::Value>(&document.content) {
            Ok(_) => "valid",
            Err(error) => {
                let location = error.span().map(|span| {
                    let offset = span.start.min(document.content.len());
                    let prefix = &document.content.as_bytes()[..offset];
                    let line = prefix.iter().filter(|byte| **byte == b'\n').count() + 1;
                    let column = prefix
                        .iter()
                        .rposition(|byte| *byte == b'\n')
                        .map_or(offset + 1, |newline| offset - newline);
                    (line, column)
                });
                issues.push(json!({"code":"toml_syntax", "message":"TOML syntax is invalid.",
                    "line":location.map(|position| position.0), "column":location.map(|position| position.1)}));
                "invalid"
            }
        },
        _ => "unsupported",
    };
    Ok(json!({
        "file":document.entry.file, "sourceSha256":document.source_sha256,
        "byteLength":document.entry.byte_length, "editable":document.entry.editable,
        "protectionReason":document.entry.protection_reason, "format":format,
        "status":status, "scope":"syntax_only", "issues":issues,
        "redacted":redacted != document.content, "locationBasis":"original_utf8_bytes_1_based",
        "guidance": if status == "unsupported" {
            "No validator is available for this file format. No syntax or runtime result was established."
        } else {
            "Syntax validation does not establish game schema compatibility, Mod compatibility or runtime recovery."
        },
    }))
}

#[cfg(test)]
#[path = "workspace_validation_tests.rs"]
mod workspace_validation_tests;
