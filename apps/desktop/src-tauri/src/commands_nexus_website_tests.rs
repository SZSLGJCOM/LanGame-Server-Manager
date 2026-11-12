use super::super::commands_mod_staging::{ONLINE_MOD_MANIFEST, manage_package};
use super::super::commands_mods::mod_inventory::read_inventory;
use super::*;
use std::io::Write;

// Synthetic official-origin URL; the test never requests this image.
const ICON: &str =
    "https://staticdelivery.nexusmods.com/mods/1059/images/9559/9559-1770000000-123456789.png";

#[test]
fn nexus_download_progress_payload_has_only_isolated_counters_and_stages() {
    use crate::commands::commands_mods::mod_download::ModDownloadProgress;
    let operation_id = uuid::Uuid::new_v4().to_string();
    let mut state = WebsiteProgress::new("fixture-instance".into(), operation_id.clone());
    let first = state.observe(download::WebsiteDownloadProgress::Downloading(
        ModDownloadProgress {
            attempt: 1,
            received_bytes: 4,
            total_bytes: Some(10),
        },
    ));
    let value = serde_json::to_value(&first).unwrap();
    assert_eq!(
        value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        [
            "attempt",
            "elapsedSeconds",
            "instanceId",
            "operationId",
            "phase",
            "receivedBytes",
            "sequence",
            "totalBytes"
        ]
    );
    assert_eq!(value["instanceId"], "fixture-instance");
    assert_eq!(value["operationId"], operation_id);
    assert_eq!(value["phase"], "downloading");
    let retry = state.observe(download::WebsiteDownloadProgress::Downloading(
        ModDownloadProgress {
            attempt: 2,
            received_bytes: 0,
            total_bytes: None,
        },
    ));
    assert!(retry.sequence > first.sequence);
    assert_eq!(retry.attempt, 2);
    assert_eq!(retry.received_bytes, 0);
    assert_eq!(retry.total_bytes, None);
    let verifying = state.observe(download::WebsiteDownloadProgress::Verifying);
    assert_eq!(verifying.phase, "verifying");
    assert_eq!(verifying.attempt, 2);
    let installing = state.snapshot("installing");
    assert_eq!(installing.phase, "installing");
    assert!(installing.sequence > verifying.sequence);
}

fn source() -> app_core::ModuleModSourceSpec {
    serde_json::from_value(json!({
        "provider": "nexus", "label": "Nexus Mods",
        "url": "https://www.nexusmods.com/enshrouded"
    }))
    .unwrap()
}

#[test]
fn nexus_website_preparation_canonicalizes_identity_without_forwarding_authorization() {
    let cases = [
        (
            "00012",
            "https://www.nexusmods.com/enshrouded/mods/12?tab=files",
        ),
        (
            "https://www.nexusmods.com/enshrouded/mods/12/files",
            "https://www.nexusmods.com/enshrouded/mods/12?tab=files",
        ),
        (
            "https://www.nexusmods.com/games/enshrouded/mods/12/files",
            "https://www.nexusmods.com/enshrouded/mods/12?tab=files",
        ),
        (
            "https://nexusmods.com/games/enshrouded/mods/12/files/88",
            "https://www.nexusmods.com/enshrouded/mods/12?tab=files&file_id=88",
        ),
        (
            "https://www.nexusmods.com/enshrouded/mods/12?tab=description&file_id=88#details",
            "https://www.nexusmods.com/enshrouded/mods/12?tab=files&file_id=88",
        ),
        (
            "nxm://enshrouded/mods/12/files/88?key=fixture-secret&expires=1&user_id=42",
            "https://www.nexusmods.com/enshrouded/mods/12?tab=files&file_id=88",
        ),
    ];
    for (input, expected) in cases {
        let target = website_target(&source(), vec!["zip".into(), "dll".into()], input).unwrap();
        assert_eq!(target.page_url, expected);
        assert_eq!(target.game_domain, "enshrouded");
        assert_eq!(target.mod_id, "12");
        assert_eq!(target.accepts, ["zip", "dll"]);
        let serialized = serde_json::to_string(&target).unwrap();
        for secret in ["fixture-secret", "expires", "user_id", "nxm:"] {
            assert!(!serialized.contains(secret), "{secret}");
        }
        let reloaded: NexusWebsiteTarget = serde_json::from_str(&serialized).unwrap();
        assert_eq!(reloaded.page_url, expected);
    }
}

#[test]
fn nexus_website_preparation_rejects_other_games_and_untrusted_or_ambiguous_references() {
    for input in [
        "https://www.nexusmods.com/skyrimspecialedition/mods/12",
        "https://www.nexusmods.com/skyrimspecialedition/mods/12/files",
        "https://www.nexusmods.com/games/enshrouded/mods/not-an-id/files",
        "https://www.nexusmods.com.evil.example/enshrouded/mods/12",
        "https://user@www.nexusmods.com/enshrouded/mods/12",
        "https://www.nexusmods.com/enshrouded/mods/12?file_id=88&file_id=89",
        "nxm://enshrouded/mods/12/files/88?key=fixture-secret",
        "12\n13",
    ] {
        let error = website_target(&source(), vec!["zip".into()], input)
            .err()
            .unwrap();
        assert!(error.contains("nexus_reference_invalid") || error.contains("nexus_game_mismatch"));
        assert!(!error.contains("fixture-secret"));
    }
    let mut wrong_source = source();
    wrong_source.provider = "thunderstore".into();
    assert!(website_target(&wrong_source, vec!["zip".into()], "12").is_err());
}

#[test]
fn nexus_website_commands_are_runtime_local_and_installation_is_owned() {
    use crate::command_registry::{Completion, Execution, LanAccess, find};
    for (name, completion) in [
        ("prepare_nexus_website_install", Completion::Bounded),
        ("install_nexus_website_download", Completion::Owned),
    ] {
        let command = find(name).expect("Nexus website command must be registered");
        assert_eq!(command.execution, Execution::Runtime);
        assert_eq!(command.lan, LanAccess::Denied);
        assert_eq!(command.completion, completion);
    }
}

#[test]
fn nexus_website_local_failures_keep_safe_installation_details() {
    for message in [
        "Stop instance fixture before changing mods.",
        "runtime supervisor lock poisoned",
        "Nexus website download was cancelled before publication.",
        "The Nexus website installation worker did not complete.",
    ] {
        let error: Value = serde_json::from_str(&installation_error(message.into())).unwrap();
        assert_eq!(error["code"], "nexus_installation_failed");
        assert_eq!(error["message"], message);
    }
    let error: Value = serde_json::from_str(&preparation_error(
        "Stop instance fixture before changing mods.".into(),
    ))
    .unwrap();
    assert_eq!(error["code"], "nexus_preparation_failed");
    assert_eq!(
        error["message"],
        "Stop instance fixture before changing mods."
    );
}

#[test]
fn nexus_website_existing_download_errors_are_not_reclassified_as_local_failures() {
    for code in [
        "nexus_reference_invalid",
        "nexus_network_error",
        "nexus_file_unavailable",
        "nexus_format_unsupported",
        "nexus_installation_failed",
    ] {
        let original = json!({"code": code, "message": "Fixed Nexus failure."}).to_string();
        assert_eq!(preparation_error(original.clone()), original);
        assert_eq!(installation_error(original.clone()), original);
        assert_eq!(
            installation_context_error(original.clone(), "Local preparation failed."),
            original
        );
    }
}

#[test]
fn nexus_website_configuration_failures_never_expose_arbitrary_source_text() {
    let context = "Could not read this instance's local Nexus installation configuration.";
    for original in [
        "https://cf-files.nexusmods.com/payload.zip?token=synthetic-test-only".to_owned(),
        "TOML parse error: Cookie = synthetic-test-only".to_owned(),
        json!({"code": "upstream_error", "message": "synthetic-test-only"}).to_string(),
    ] {
        let wrapped = installation_context_error(original, context);
        let error: Value = serde_json::from_str(&wrapped).unwrap();
        assert_eq!(error["code"], "nexus_installation_failed");
        assert_eq!(error["message"], context);
        assert!(!wrapped.contains("synthetic-test-only"));
        assert!(!wrapped.contains("https://"));
        assert!(!wrapped.contains("Cookie"));
    }
}

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "langame-nexus-website-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }

    fn target(&self) -> ResolvedManualModTarget {
        ResolvedManualModTarget {
            instance_id: "synthetic-nexus-website".into(),
            module_id: "enshrouded".into(),
            source_label: "Nexus Mods".into(),
            target_label: "mods".into(),
            target_path: self.0.join("install/mods"),
            accepts: vec!["folder".into(), "zip".into(), "dll".into()],
            id_strategy: None,
        }
    }

    fn archive(&self, name: &str, entries: &[(&str, &[u8])]) -> PathBuf {
        let path = self.0.join(name);
        let mut archive = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        for (name, contents) in entries {
            archive
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            archive.write_all(contents).unwrap();
        }
        archive.finish().unwrap();
        path
    }

    fn stage(
        &self,
        path: PathBuf,
        title: Option<&str>,
        icon: Option<&str>,
    ) -> Result<ManualModStageResult, String> {
        stage_website_mod_source(
            self.target(),
            path,
            ManualModImportPresentation {
                display_name: super::super::mod_presentations::normalize_display_name(title),
                icon_url: super::super::mod_presentations::verified_icon_url(icon),
            },
            None,
        )
    }

    fn manifest(&self) -> Value {
        serde_json::from_slice(
            &fs::read(self.target().target_path.join(ONLINE_MOD_MANIFEST)).unwrap(),
        )
        .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn nexus_website_cancelled_publication_returns_failure_without_installing_files() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let fixture = Fixture::new();
    let source = fixture.archive("Cancelled.zip", &[("ActualMod/payload.dll", b"payload")]);
    let cancellation = AtomicBool::new(true);
    let presentation = || ManualModImportPresentation {
        display_name: Some("Readable Nexus Mod".into()),
        icon_url: Some(ICON.into()),
    };
    let error = publish_website_download(
        fixture.target(),
        source.clone(),
        presentation(),
        None,
        &cancellation,
    )
    .unwrap_err();
    let error: Value = serde_json::from_str(&error).unwrap();
    assert_eq!(error["code"], "nexus_installation_failed");
    assert_eq!(
        error["message"],
        "Nexus website installation was cancelled before publication."
    );
    assert!(!fixture.target().target_path.exists());
    assert!(source.is_file());

    cancellation.store(false, Ordering::Release);
    let result = publish_website_download(
        fixture.target(),
        source,
        presentation(),
        None,
        &cancellation,
    )
    .unwrap();
    assert_eq!(result.copied_file_count, 1);
    assert_eq!(
        fs::read(fixture.target().target_path.join("ActualMod/payload.dll")).unwrap(),
        b"payload"
    );
}

fn assert_presentation(inventory: &ManualModInventoryResult, name: &str, enabled: bool) {
    let item = inventory
        .items
        .iter()
        .find(|item| item.name == name)
        .unwrap();
    assert_eq!(item.display_name.as_deref(), Some("Readable Nexus Mod"));
    assert_eq!(item.icon_url.as_deref(), Some(ICON));
    let management = item.management.as_ref().unwrap();
    assert_eq!(management.package_id, format!("local-{name}"));
    assert_eq!(management.enabled, enabled);
}

#[test]
fn nexus_website_presentation_follows_actual_roots_through_reload_toggle_and_local_replacement() {
    let fixture = Fixture::new();
    let target_path = fixture.target().target_path;
    fs::create_dir_all(target_path.join("Unrelated")).unwrap();
    fs::write(
        target_path.join("Unrelated/other.dll"),
        b"unrelated payload",
    )
    .unwrap();
    let archive = fixture.archive(
        "Upstream-archive-name.zip",
        &[
            ("ActualMod/payload.dll", b"first payload"),
            ("SecondMod/second.dll", b"second payload"),
            ("config/website.cfg", b"sample configuration"),
        ],
    );
    fixture
        .stage(archive, Some(" Readable Nexus Mod "), Some(ICON))
        .unwrap();
    let manifest = fixture.manifest();
    let packages = manifest["packages"].as_object().unwrap();
    assert_eq!(packages.len(), 2);
    for name in ["ActualMod", "SecondMod"] {
        let record = &packages[&format!("local-{name}")];
        assert_eq!(record["target_name"], name);
        assert_eq!(record["display_name"], "Readable Nexus Mod");
        assert_eq!(record["icon_url"], ICON);
        assert!(record.get("thunderstore_intent").is_none());
        assert!(record["version"].is_null());
        assert!(record["dependencies"].is_null());
        assert_presentation(&read_inventory(fixture.target()).unwrap(), name, true);
    }
    assert!(!packages.contains_key("local-Upstream-archive-name"));
    assert!(!packages.contains_key("local-config"));
    let inventory = read_inventory(fixture.target()).unwrap();
    let unrelated = inventory
        .items
        .iter()
        .find(|item| item.name == "Unrelated")
        .unwrap();
    assert!(unrelated.display_name.is_none());
    assert!(unrelated.icon_url.is_none());
    assert!(unrelated.management.is_none());
    for (action, enabled) in [("disable", false), ("enable", true)] {
        let result = manage_package(fixture.target(), "local-ActualMod", action).unwrap();
        assert_presentation(&result.inventory, "ActualMod", enabled);
        assert_presentation(
            &read_inventory(fixture.target()).unwrap(),
            "ActualMod",
            enabled,
        );
    }
    let replacement = fixture.archive(
        "Manual-replacement.zip",
        &[("ActualMod/payload.dll", b"updated payload")],
    );
    super::super::commands_mod_staging::stage_manual_mod_sources(
        fixture.target(),
        vec![replacement],
    )
    .unwrap();
    assert_presentation(
        &read_inventory(fixture.target()).unwrap(),
        "ActualMod",
        true,
    );
    assert_eq!(
        fs::read(target_path.join("ActualMod/payload.dll")).unwrap(),
        b"updated payload"
    );
    assert_eq!(
        fs::read(target_path.join("Unrelated/other.dll")).unwrap(),
        b"unrelated payload"
    );
    assert_eq!(
        fs::read(target_path.join("config/website.cfg")).unwrap(),
        b"sample configuration"
    );
}

#[test]
fn nexus_website_missing_or_rejected_presentation_does_not_prevent_loose_file_installation() {
    for icon in [None, Some("https://untrusted.example/avatar.png")] {
        let fixture = Fixture::new();
        let source = fixture.0.join("OriginalFilename.dll");
        fs::write(&source, b"payload").unwrap();
        fixture.stage(source, None, icon).unwrap();
        let inventory = read_inventory(fixture.target()).unwrap();
        let item = inventory
            .items
            .iter()
            .find(|item| item.name == "OriginalFilename.dll")
            .unwrap();
        assert!(item.display_name.is_none());
        assert!(item.icon_url.is_none());
        assert_eq!(
            item.management.as_ref().unwrap().package_id,
            "local-OriginalFilename.dll"
        );
        assert_eq!(
            fs::read(fixture.target().target_path.join(&item.name)).unwrap(),
            b"payload"
        );
    }
}

#[cfg(windows)]
#[test]
fn nexus_website_failed_publication_rolls_back_display_data_and_payload_together() {
    use std::os::windows::fs::OpenOptionsExt;
    let fixture = Fixture::new();
    let initial = fixture.archive("Initial.zip", &[("ActualMod/z.dll", b"old payload")]);
    fixture
        .stage(initial, Some("Readable Nexus Mod"), Some(ICON))
        .unwrap();
    let destination = fixture.target().target_path;
    let manifest = destination.join(ONLINE_MOD_MANIFEST);
    let before = fs::read(&manifest).unwrap();
    fs::write(
        destination.join("ActualMod/operator.cfg"),
        b"operator settings",
    )
    .unwrap();
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(destination.join("ActualMod/z.dll"))
        .unwrap();
    let replacement = fixture.archive(
        "Replacement.zip",
        &[
            ("ActualMod/a.dll", b"new file"),
            ("ActualMod/z.dll", b"new payload"),
        ],
    );
    let error = fixture
        .stage(replacement, Some("Uncommitted new title"), None)
        .unwrap_err();
    drop(held);
    assert!(error.contains("rolled back"), "{error}");
    assert_eq!(fs::read(&manifest).unwrap(), before);
    assert_eq!(
        fs::read(destination.join("ActualMod/z.dll")).unwrap(),
        b"old payload"
    );
    assert_eq!(
        fs::read(destination.join("ActualMod/operator.cfg")).unwrap(),
        b"operator settings"
    );
    assert!(!destination.join("ActualMod/a.dll").exists());
    assert_presentation(
        &read_inventory(fixture.target()).unwrap(),
        "ActualMod",
        true,
    );
}
