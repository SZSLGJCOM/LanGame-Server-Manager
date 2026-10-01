use std::path::Path;

use app_core::{InstanceDetails, LogTailSnapshot};

pub(super) fn report_astroneer_startup(instance: &InstanceDetails, install_root: &Path) {
    if instance.summary.module_id != "astroneer"
        || std::env::var("LANGAME_NATIVE_LOG_SUCCESS").as_deref() != Ok("true")
    {
        return;
    }
    let semaphore = install_root.join("Astro/Saved/EXITREQUEST");
    let markers = ["running", "exitrequest", "pending", "shutdown"]
        .map(|name| format!("{name}={}", semaphore.join(name).is_file()));
    let processes = instance
        .active_run
        .as_ref()
        .map(|run| {
            run.processes
                .iter()
                .map(|process| format!("{}:{:?}", process.process_key, process.pid))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    eprintln!(
        "NATIVE_ASTRONEER_STARTUP processes=[{}] semaphore=[{}]",
        processes.join(","),
        markers.join(",")
    );
}

pub(super) async fn report_failed_readiness(
    descriptor: &app_modules::ModuleDescriptor,
    instance: &InstanceDetails,
    install_root: &Path,
    fixture_root: &Path,
) {
    if instance.summary.module_id == "squad" {
        super::squad_probe::report(descriptor, instance, install_root, fixture_root).await;
    }
}

pub(super) fn query_player_count(protocol: &str, port: u16) -> Result<(), String> {
    if protocol == "a2s_info" {
        return app_storage::query_a2s_player_count(([127, 0, 0, 1], port).into())
            .map(|_| ())
            .map_err(|error| error.to_string());
    }
    app_storage::query_live_player_count(protocol, "127.0.0.1", port)
        .map(|_| ())
        .ok_or_else(|| "owned endpoint without an accepted reply".into())
}

pub(super) fn contains_private_runtime_identity(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    if [
        "public ip",
        "session with info",
        "gameinfo path",
        "starting a new world :",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
    {
        return true;
    }
    line.split(|character: char| {
        character.is_ascii_whitespace()
            || matches!(
                character,
                ';' | ',' | '(' | ')' | '=' | '[' | ']' | '"' | '\''
            )
    })
    .any(|part| {
        part.parse::<std::net::IpAddr>()
            .or_else(|_| part.split(':').next().unwrap_or_default().parse())
            .is_ok_and(|address| !address.is_loopback() && !address.is_unspecified())
    })
}

pub(super) fn redact_fixture_paths(mut text: String, root: &Path) -> String {
    for path in std::iter::once(root.to_path_buf()).chain(root.canonicalize().ok()) {
        let path = path.to_string_lossy();
        let path = path.strip_prefix(r"\\?\").unwrap_or(&path);
        text = text.replace(path, "<fixture>");
        text = text.replace(&path.replace('\\', "/"), "<fixture>");
    }
    text
}

#[test]
fn native_diagnostics_omit_runtime_addresses_and_session_identity() {
    for line in [
        "StandaloneNetworkingSubset.SetPublicIP: Received public ip 192.0.2.10",
        "192.0.2.10;27015;disposable-game;disposable-password",
        "Started session with info: disposable-game",
        "GameInfo path: E:/fixture/GameInfo.txt",
        "starting a new world : ephemeral-world-id",
        "Remote endpoint 198.51.100.4:27015",
        "Remote endpoint [2001:db8::1]",
    ] {
        assert!(contains_private_runtime_identity(line), "{line}");
    }
    assert!(!contains_private_runtime_identity("timescale = 0"));
    assert!(!contains_private_runtime_identity(
        "Listening on ip:0.0.0.0:27015"
    ));
}

#[test]
fn native_diagnostics_redact_both_fixture_path_separators() {
    let root = Path::new(r"D:\fixture\instance");
    assert_eq!(
        redact_fixture_paths(
            "D:/fixture/instance/save; D:\\fixture\\instance\\log".into(),
            root,
        ),
        "<fixture>/save; <fixture>\\log"
    );
}

pub(super) fn failed_run_logs(
    instance: &InstanceDetails,
    install_root: &Path,
    fixture_root: &Path,
) -> Result<Vec<LogTailSnapshot>, &'static str> {
    // Failed starts have no database run record. This freshly created fixture
    // has exactly one launch attempt, so its bounded console directory contains
    // only that attempt; never discover logs outside this disposable instance.
    let root = fixture_root
        .canonicalize()
        .map_err(|_| "fixture root unavailable")?;
    let instance_root = Path::new(&instance.config_file_path)
        .parent()
        .and_then(Path::parent)
        .ok_or("invalid fixture config path")?;
    let console = instance_root.join("logs").join("managed-console");
    if !console.exists() {
        return Ok(Vec::new());
    }
    let console = console
        .canonicalize()
        .map_err(|_| "fixture console unavailable")?;
    if !console.starts_with(&root) {
        return Err("native console escaped this fixture");
    }
    let entries = std::fs::read_dir(console).map_err(|_| "failed to list fixture console")?;
    let mut logs = Vec::new();
    for (index, entry) in entries.enumerate() {
        if index >= 32 {
            return Err("fixture console exceeds bounded log discovery");
        }
        let entry = entry.map_err(|_| "failed to inspect fixture console entry")?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !name.starts_with("run-") || !name.ends_with(".log") {
            continue;
        }
        let path = entry
            .path()
            .canonicalize()
            .map_err(|_| "native console log unavailable")?;
        if !path.starts_with(&root) {
            return Err("native console log escaped this fixture");
        }
        if path.is_file() {
            logs.push(app_storage::read_log_path_snapshot(
                path.to_string_lossy().into_owned(),
                80,
            ));
        }
    }
    let native_log = match instance.summary.module_id.as_str() {
        "corekeeper" => Some(instance_root.join("logs/CoreKeeperServer.log")),
        "astroneer" => Some(install_root.join("Astro/Saved/Logs/Astro.log")),
        "runescapedragonwilds" => {
            Some(install_root.join("RSDragonwilds/Saved/Logs/RSDragonwilds.log"))
        }
        "rust" => Some(instance_root.join("logs/rust.log")),
        "satisfactory" => Some(instance_root.join("data/Saved/Logs/FactoryGame.log")),
        _ => None,
    };
    if let Some(path) = native_log.filter(|path| path.is_file()) {
        let path = path
            .canonicalize()
            .map_err(|_| "native game log unavailable")?;
        if !path.starts_with(&root) {
            return Err("native game log escaped this fixture");
        }
        logs.push(app_storage::read_log_path_snapshot(
            path.to_string_lossy().into_owned(),
            160,
        ));
    }
    Ok(logs)
}

pub(super) fn observation_seconds() -> Result<u64, &'static str> {
    let seconds = std::env::var("LANGAME_NATIVE_OBSERVE_SECONDS")
        .ok()
        .map(|value| value.parse::<u64>())
        .transpose()
        .map_err(|_| "invalid native diagnostic observation duration")?
        .unwrap_or(0);
    if seconds > 120 {
        return Err("native diagnostic observation exceeds 120 seconds");
    }
    Ok(seconds)
}

// Applied only to the disposable instance before the production settings
// validator/materializer. Never print the payload: it may contain credentials.
pub(super) fn fixture_settings_override(
    json: Option<&str>,
) -> Result<serde_json::Map<String, serde_json::Value>, &'static str> {
    let Some(json) = json else {
        return Ok(serde_json::Map::new());
    };
    if json.len() > 64 * 1024 {
        return Err("native fixture settings exceed 64 KiB");
    }
    serde_json::from_str(json).map_err(|_| "native fixture settings must be a JSON object")
}

#[test]
fn native_fixture_settings_require_bounded_object() {
    assert!(fixture_settings_override(None).unwrap().is_empty());
    assert_eq!(
        fixture_settings_override(Some(r#"{"lan_only":false}"#)).unwrap()["lan_only"],
        serde_json::json!(false)
    );
    for invalid in ["null", "[]", "true", "{bad}"] {
        assert!(fixture_settings_override(Some(invalid)).is_err());
    }
    assert!(fixture_settings_override(Some(&" ".repeat(64 * 1024 + 1))).is_err());
}

pub(super) async fn report_success(
    phase: &str,
    instance: &InstanceDetails,
    install_root: &Path,
    root: &Path,
    settings: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), &'static str> {
    if std::env::var("LANGAME_NATIVE_LOG_SUCCESS").as_deref() == Ok("true") {
        let logs = failed_run_logs(instance, install_root, root)?;
        super::print_native_failure(
            phase,
            "bounded native diagnostic observation",
            &logs,
            settings,
            root,
        );
        let files = super::save_inventory(Path::new(&instance.saves_path))
            .map_err(|_| "failed to observe this fixture's save inventory")?;
        for (path, (bytes, _)) in files.iter().take(16) {
            eprintln!(
                "NATIVE_DIAGNOSTIC phase={phase} save_file={} bytes={bytes}",
                path.display()
            );
        }
        if instance.summary.module_id == "astroneer" && phase != "stopped" {
            match super::astroneer_console::inspect(instance).await {
                Ok(observations) => {
                    for observation in observations {
                        eprintln!("NATIVE_DIAGNOSTIC phase={phase} console={observation}");
                    }
                }
                Err(category) => {
                    eprintln!("NATIVE_DIAGNOSTIC phase={phase} console_error={category}");
                }
            }
        }
    }
    Ok(())
}
