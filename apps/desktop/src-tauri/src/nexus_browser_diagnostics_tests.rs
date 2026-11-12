use super::*;

const PROJECT: &str = "https://www.nexusmods.com/7daystodie/mods/2264";

fn summary() -> Value {
    json!({
        "readyState": "complete", "fileId": "00088", "canonicalCount": 1,
        "mainHeadingCount": 1, "manualControlCount": 1, "matchingManualCount": 1,
        "slowControlCount": 0, "matchingSlowCount": 0, "loginNoticeCount": 0,
        "visibleDialogCount": 0, "challengeFramePresent": false,
        "controls": [{ "kind": "manual", "tag": "a", "displayed": true,
            "disabled": false, "matchesProject": true, "fileId": "00088" }],
    })
}

fn observation(diagnostics: Value) -> PageObservation {
    serde_json::from_value(json!({
        "page": { "url": PROJECT, "canonicalUrl": PROJECT,
            "gameDomain": "7daystodie", "modId": "2264", "title": "private-title",
            "iconUrl": "https://private.invalid/avatar?token=fixture-client-secret", "state": "ready" },
        "rendered": { "url": PROJECT, "hasContent": true },
        "automation": { "state": "blocked", "reason": "website_controls" },
        "diagnostics": diagnostics,
    }))
    .unwrap()
}

#[test]
fn nexus_diagnostics_drop_malformed_fields_without_losing_the_page() {
    for value in [Value::Null, json!("untrusted"), json!({}), {
        let mut value = summary();
        value["readyState"] = json!("private-page-text");
        value
    }] {
        let page = observation(value);
        assert_eq!(page.page.state, "ready");
        assert!(page.diagnostics.is_none());
    }
}

#[test]
fn nexus_diagnostics_cap_controls_and_drop_unknown_text_and_invalid_ids() {
    let mut value = summary();
    let mut control = value["controls"][0].clone();
    control["label"] = json!("private-control-text");
    control["href"] = json!("https://private.invalid/?token=fixture-client-secret");
    control["fileId"] = json!("18446744073709551616");
    value["controls"] = json!(vec![control; 12]);
    value["cookie"] = json!("private-cookie");
    value["html"] = json!("private-html");
    let page = observation(value);
    let value = serde_json::to_value(page.diagnostics.unwrap()).unwrap();
    assert_eq!(value["fileId"], "88");
    assert_eq!(value["controls"].as_array().unwrap().len(), 8);
    assert!(
        value["controls"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["fileId"].is_null())
    );
    assert!(!value.to_string().contains("private-"));
    assert!(!value.to_string().contains("fixture-client-secret"));
    for id in ["", "0", "18446744073709551616", "88&token=private", "-88"] {
        assert!(public_id(id).is_none());
    }
}

#[test]
fn nexus_diagnostic_identity_never_contains_raw_urls_or_private_parameters() {
    let expected = json!({ "kind": "project", "game_domain": "7daystodie", "mod_id": "2264", "file_id": "88" });
    assert_eq!(
        page_identity(&format!("{PROJECT}?file_id=88&token=fixture-client-secret")),
        Some(expected.clone())
    );
    assert_eq!(
        page_identity("https://nexusmods.com/games/7daystodie/mods/2264/files/88?token=private"),
        Some(expected)
    );
    assert_eq!(
        page_identity("https://users.nexusmods.com/auth/sign_in?return=private"),
        Some(json!({ "kind": "login" }))
    );
    for value in [
        "https://private.invalid/7daystodie/mods/2264",
        "https://user:password@www.nexusmods.com/7daystodie/mods/2264",
        "https://www.nexusmods.com/7daystodie/mods/2264/private",
        "https://cf-files.nexusmods.com/file.zip?token=private",
        "http://www.nexusmods.com/7daystodie/mods/2264",
    ] {
        assert!(page_identity(value).is_none());
    }
    for query in ["file_id=88&file_id=99", "file_id=private"] {
        assert!(page_identity(&format!("{PROJECT}?{query}")).unwrap()["file_id"].is_null());
    }
}

struct LogDirectory(PathBuf);

impl LogDirectory {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("langame-nexus-log-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for LogDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn nexus_attempt_log_persists_safe_context_and_deduplicates_stages() {
    let directory = LogDirectory::new();
    let path = directory.0.join("desktop-ui/active.jsonl");
    let log = AttemptLog::new("session-1", Some(path.clone()));
    log.start(
        "instance-1",
        &format!("{PROJECT}?file_id=88&token=fixture-client-secret"),
    );
    log.stage("opening");
    log.stage("opening");
    log.stage("private-phase");
    let target = NexusWebsiteTarget {
        page_url: PROJECT.into(),
        game_domain: "7daystodie".into(),
        mod_id: "2264".into(),
        accepts: vec!["zip".into()],
    };
    log.observe(
        &observation(summary()),
        &format!("{PROJECT}?file_id=88&token=fixture-client-secret")
            .parse()
            .unwrap(),
        &target,
    );
    log.stage("selecting");
    log.stage("selecting");
    let failure =
        json!({"code":"nexus_website_controls_unavailable","message":"private-runtime-error"})
            .to_string();
    log.blocked(&failure);
    log.finish(Some(&failure));
    let text = std::fs::read_to_string(path).unwrap();
    assert!(!text.contains("private-"));
    assert!(!text.contains("fixture-client-secret"));
    assert!(!text.contains("https://"));
    let entries = text
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|entry| {
            entry["action"]
                .as_str()
                .is_some_and(|action| action.starts_with("mod.nexus.website."))
        })
        .collect::<Vec<_>>();
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry["action"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "mod.nexus.website.started",
            "mod.nexus.website.stage",
            "mod.nexus.website.stage",
            "mod.nexus.website.blocked",
            "mod.nexus.website.failed",
        ]
    );
    let context = &entries.last().unwrap()["context"];
    assert_eq!(context["session_id"], "session-1");
    assert_eq!(context["instance_id"], "instance-1");
    assert_eq!(context["code"], "nexus_website_controls_unavailable");
    assert_eq!(context["page"]["metadata_state"], "ready");
    assert_eq!(context["page"]["has_content"], true);
    assert_eq!(context["page"]["current"]["file_id"], "88");
    assert_eq!(context["page"]["diagnostics"]["manualControlCount"], 1);
}

#[test]
fn nexus_quarantined_file_retains_its_fixed_code_in_blocked_and_failure_logs() {
    let directory = LogDirectory::new();
    let path = directory.0.join("desktop-ui/active.jsonl");
    let log = AttemptLog::new("quarantined-file", Some(path.clone()));
    let failure = super::super::blocked_error(Some("file_quarantined"));
    log.blocked(&failure);
    log.finish(Some(&failure));
    let entries = std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    let (header, entries) = entries.split_first().unwrap();
    assert_eq!(header["action"], "desktop.log.segment");
    assert_eq!(entries.len(), 2);
    for (entry, action) in entries
        .iter()
        .zip(["mod.nexus.website.blocked", "mod.nexus.website.failed"])
    {
        assert_eq!(entry["action"], action);
        assert_eq!(entry["context"]["code"], "nexus_website_file_quarantined");
    }
}

#[test]
fn nexus_attempt_log_accepts_only_fixed_error_codes() {
    assert_eq!(
        fixed_error_code(r#"{"code":"nexus_preparation_failed","message":"private"}"#),
        "nexus_preparation_failed"
    );
    for value in [
        "private-error",
        r#"{"code":"nexus_private-value"}"#,
        r#"{"message":"private"}"#,
    ] {
        assert_eq!(fixed_error_code(value), "nexus_operation_failed");
    }
    let directory = LogDirectory::new();
    // An ordinary file cannot be a log directory. Logging failure stays local.
    std::fs::write(directory.0.join("occupied"), b"unchanged").unwrap();
    let log = AttemptLog::new("session", Some(directory.0.join("occupied/active.jsonl")));
    log.finish(Some("private-error"));
    assert_eq!(
        std::fs::read(directory.0.join("occupied")).unwrap(),
        b"unchanged"
    );
}
