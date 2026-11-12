//! Verify the same bounded desktop log used by the normal application.
use super::*;

pub(super) fn verify(
    root: &Path,
    main_ipc: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let text = std::fs::read_to_string(root.join("logs/desktop-ui/active.jsonl"))
        .map_err(|error| error.to_string())?;
    if [
        "fixture-client-secret",
        "https://",
        "http://",
        FIXTURE_TITLE,
        "<h1>",
    ]
    .iter()
    .any(|value| text.contains(value))
    {
        return Err("Nexus normal diagnostics leaked website content or a raw URL".into());
    }
    let entries = text
        .lines()
        .map(serde_json::from_str::<serde_json::Value>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let entries = entries
        .iter()
        .filter(|entry| {
            entry["action"]
                .as_str()
                .is_some_and(|action| action.starts_with("mod.nexus.website."))
        })
        .collect::<Vec<_>>();
    let no_controls_session = main_ipc["no_controls"]["sessionId"]
        .as_str()
        .ok_or("Main IPC missing-controls session was not reported")?;
    let missing = entries
        .iter()
        .filter(|entry| entry["context"]["session_id"] == no_controls_session)
        .collect::<Vec<_>>();
    let actions = missing
        .iter()
        .filter_map(|entry| entry["action"].as_str())
        .collect::<Vec<_>>();
    if actions
        != [
            "mod.nexus.website.started",
            "mod.nexus.website.stage",
            "mod.nexus.website.stage",
            "mod.nexus.website.blocked",
            "mod.nexus.website.failed",
        ]
    {
        return Err(format!(
            "Missing-controls IPC did not persist its complete normal log lifecycle: {actions:?}"
        ));
    }
    for entry in missing.iter().rev().take(2) {
        let context = &entry["context"];
        let page = &context["page"];
        let summary = &page["diagnostics"];
        if context["code"] != "nexus_website_controls_unavailable"
            || context["target"]["game_domain"] != "7daystodie"
            || context["target"]["mod_id"] != "2264"
            || page["metadata_state"] != "ready"
            || page["has_content"] != true
            || page["matches_target"] != true
            || page["rendered_current"] != true
            || page["current"]["mod_id"] != "2264"
            || summary["readyState"] != "complete"
            || summary["canonicalCount"] != 1
            || summary["mainHeadingCount"] != 0
            || summary["manualControlCount"] != 0
            || summary["slowControlCount"] != 0
            || summary["matchingManualCount"] != 0
            || summary["matchingSlowCount"] != 0
            || summary["loginNoticeCount"] != 0
            || summary["visibleDialogCount"] != 0
            || summary["controls"] != json!([])
        {
            return Err(format!(
                "Missing-controls log lost its safe page and diagnostic summary: {entry}"
            ));
        }
    }
    let mut previous = std::collections::HashMap::new();
    let mut stages = 0;
    for entry in &entries {
        if entry["action"] != "mod.nexus.website.stage" {
            continue;
        }
        let session = entry["context"]["session_id"]
            .as_str()
            .ok_or("Stage missing session")?;
        let phase = entry["context"]["phase"]
            .as_str()
            .ok_or("Stage missing phase")?;
        if previous.insert(session, phase) == Some(phase) {
            return Err(format!("Repeated Nexus stage was logged: {phase}"));
        }
        stages += 1;
    }
    for case in ["no_runtime", "missing_instance", "cancelled"] {
        let result = &main_ipc[case];
        if !entries.iter().any(|entry| {
            entry["action"] == "mod.nexus.website.failed"
                && entry["context"]["session_id"] == result["sessionId"]
                && entry["context"]["code"] == result["error"]["code"]
        }) {
            return Err(format!(
                "Main IPC {case} did not persist its fixed failure code"
            ));
        }
    }
    Ok(
        json!({ "normal_desktop_log": "logs/desktop-ui/active.jsonl", "records": entries.len(),
        "stage_records": stages, "stages_deduplicated": true, "no_controls_blocked_and_failed": true,
        "metadata_and_zero_controls_recorded": true, "signed_query_and_raw_content_absent": true }),
    )
}
