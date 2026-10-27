//! Explicit native SML/FRM proof, independent of the generic smoke probes.
#![cfg(test)]
use std::collections::BTreeSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use app_core::{
    InstanceDetails, InstanceStatus, ModulePlayerListSource, RuntimeLivePlayerSnapshot,
    RuntimeLivePlayerStatus,
};
use app_platform_win::WindowsPlatform;

use super::runtime::NativeRuntime;

#[path = "commands_native_satisfactory_frm_stage.rs"]
mod staging;

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
const WEB_PORT: u16 = 18080;

pub(super) struct Fixture {
    mods: Vec<staging::VerifiedMod>,
}

impl Fixture {
    pub(super) fn from_env(module_id: &str, source: &Path) -> Result<Option<Self>> {
        if std::env::var("LANGAME_NATIVE_SATISFACTORY_FRM").as_deref() != Ok("true") {
            return Ok(None);
        }
        if module_id != "satisfactory"
            || std::env::var("LANGAME_NATIVE_MANUAL_SAVE").as_deref() != Ok("true")
            || std::env::var("LANGAME_NATIVE_MANUAL_SAVE_RESTART").as_deref() != Ok("true")
        {
            return Err("Satisfactory FRM native acceptance requires its module, native manual-save world fixture and restart".into());
        }
        let archives = PathBuf::from(
            std::env::var_os("LANGAME_NATIVE_SATISFACTORY_MOD_ARCHIVES")
                .ok_or("Supply the reviewed Satisfactory WindowsServer Mod archives")?,
        );
        Ok(Some(Self {
            mods: staging::load(source, &staging::plain(&archives)?)?,
        }))
    }

    pub(super) fn stage(
        &self,
        details: &InstanceDetails,
        runtime: &Path,
        fixture: &Path,
    ) -> Result<()> {
        let fixture = staging::plain(fixture)?;
        let runtime = staging::plain(runtime)?;
        let config = staging::plain(Path::new(&details.config_file_path))?;
        let instance_settings: serde_json::Value = serde_json::from_str(&details.settings_json)?;
        if details.summary.module_id != "satisfactory"
            || !matches!(details.summary.status, InstanceStatus::Stopped)
            || details.active_run.is_some()
            || !runtime.starts_with(&fixture)
            || runtime == fixture
            || !config.starts_with(&fixture)
            || staging::plain(&super::creation::program_root(details)?)? != runtime
            || instance_settings["allow_insecure_local_api"].as_bool() == Some(true)
        {
            return Err("Satisfactory FRM staging requires this disposable stopped instance and actual owned runtime".into());
        }
        staging::stage(&self.mods, &runtime)?;
        let settings = config
            .parent()
            .and_then(Path::parent)
            .ok_or("Satisfactory fixture has no instance root")?
            .join("data/Saved/Config/WindowsServer/GameUserSettings.ini");
        let existing = match fs::symlink_metadata(&settings) {
            Ok(_) => {
                let mut text = String::new();
                fs::File::open(staging::plain(&settings)?)?
                    .take(256 * 1024 + 1)
                    .read_to_string(&mut text)?;
                text
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(error.into()),
        };
        let replacement = staging::settings(&existing, WEB_PORT)?;
        fs::create_dir_all(
            settings
                .parent()
                .ok_or("Satisfactory fixture native settings have no parent")?,
        )?;
        staging::plain(
            settings
                .parent()
                .ok_or("Satisfactory fixture native settings have no parent")?,
        )?;
        fs::write(settings, replacement)?;
        Ok(())
    }

    pub(super) async fn verify<R: tauri::Runtime>(
        &self,
        runtime: &NativeRuntime<'_, '_, R>,
        running: &InstanceDetails,
        cycle: &str,
    ) -> Result<()> {
        owned(runtime, running)?;
        let process = running
            .active_run
            .as_ref()
            .and_then(|run| run.processes.iter().find(|process| process.is_primary))
            .ok_or("Satisfactory FRM fixture has no primary process")?;
        let log = process
            .log_path
            .as_deref()
            .ok_or("Satisfactory FRM fixture has no current console log")?;
        // NativeWorld has already loaded the fresh owned world. Only closed
        // startup/loading results are observed again, with no world mutation.
        tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                owned(runtime, running)?;
                if !load_evidence(Path::new(log))?.complete() {
                    tokio::time::sleep(Duration::from_millis(250)).await;
                    continue;
                }
                let owners = WindowsPlatform::loopback_tcp_listener_owners(WEB_PORT)?;
                if owners.is_empty() { tokio::time::sleep(Duration::from_millis(250)).await; continue; }
                let pid = process.pid.ok_or("Satisfactory FRM fixture has no process ID")?;
                if owners != BTreeSet::from([pid]) { return Err("FRM loopback listener does not belong exclusively to this native run".into()); }
                let request_id = uuid::Uuid::new_v4().to_string();
                let observed_at = u64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())?;
                match crate::live_players::satisfactory::collect_satisfactory(running, &running.summary.id, &request_id, observed_at).await {
                    Ok(collected) => {
                        owned_web(runtime, running)?;
                        let snapshot = &collected.public_snapshot;
                        if snapshot.instance_id != running.summary.id || snapshot.snapshot_id != request_id || snapshot.observed_at_unix_ms != Some(observed_at) {
                            return Err("Production FRM response belongs to another observation".into());
                        }
                        empty_roster(snapshot)?;
                        break;
                    },
                    Err(failure) if failure.issue.as_ref().is_some_and(|issue| issue.code == app_core::RuntimeLivePlayerIssueCode::ProcessUnavailable) => { tokio::time::sleep(Duration::from_millis(250)).await; },
                    Err(_) => return Err("Production FRM getPlayer query did not return a complete successful native response".into()),
                }
            }
            Ok::<(), Box<dyn std::error::Error>>(())
        }).await.map_err(|_| "Satisfactory FRM endpoint did not finish bounded native startup observation")??;
        owned_web(runtime, running)?;
        let raw = native_empty_array().await?;
        owned_web(runtime, running)?;
        println!(
            "NATIVE_SATISFACTORY_FRM cycle={cycle} sml=3.12.0 frm=1.5.4 loaded_versions=current_run_log getPlayer=production_collector_ready response_bytes={raw} response_empty_array=true complete=true stale=false online_players=0 source=http_api listener=owned_native_process fixture_only=true client_join=not_tested"
        );
        Ok(())
    }
}

fn owned<R: tauri::Runtime>(
    runtime: &NativeRuntime<'_, '_, R>,
    running: &InstanceDetails,
) -> Result<()> {
    let run = running
        .active_run
        .as_ref()
        .ok_or("Satisfactory FRM fixture has no run")?;
    if running.summary.module_id != "satisfactory" || run.processes.len() != 1 {
        return Err("Satisfactory FRM fixture run is ambiguous".into());
    }
    let process = &run.processes[0];
    let pid = process.pid.ok_or("Satisfactory FRM fixture has no PID")?;
    let identity = process
        .process_identity
        .as_ref()
        .ok_or("Satisfactory FRM fixture has no process identity")?;
    if app_runtime::inspect_process_identity(pid)?.as_ref() != Some(identity)
        || !Path::new(&identity.image_path)
            .canonicalize()?
            .starts_with(staging::plain(&runtime.package.root)?)
        || !runtime
            .state
            .runtime_supervisor
            .lock()
            .map_err(|_| "Satisfactory FRM fixture supervisor lock failed")?
            .matches_running_process(&running.summary.id, run.run_id, &process.process_key, pid)?
    {
        return Err("Satisfactory FRM fixture changed its owned process generation".into());
    }
    Ok(())
}

fn owned_web<R: tauri::Runtime>(
    runtime: &NativeRuntime<'_, '_, R>,
    running: &InstanceDetails,
) -> Result<()> {
    owned(runtime, running)?;
    let pid = running
        .active_run
        .as_ref()
        .and_then(|run| run.processes.first())
        .and_then(|process| process.pid)
        .ok_or("Satisfactory FRM fixture has no PID")?;
    if WindowsPlatform::loopback_tcp_listener_owners(WEB_PORT)? != BTreeSet::from([pid]) {
        return Err("FRM listener changed its native owner during the observation".into());
    }
    Ok(())
}

fn empty_roster(snapshot: &RuntimeLivePlayerSnapshot) -> Result<()> {
    if snapshot.status != RuntimeLivePlayerStatus::Ready
        || snapshot.source != Some(ModulePlayerListSource::HttpApi)
        || !snapshot.complete
        || snapshot.stale
        || snapshot.truncated
        || snapshot.current_players != Some(0)
        || !snapshot.entries.is_empty()
        || snapshot.issue.is_some()
    {
        return Err("FRM must report a successful complete empty online roster; failure or unknown is not zero players".into());
    }
    Ok(())
}

async fn native_empty_array() -> Result<usize> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .connect_timeout(Duration::from_secs(1))
        .timeout(Duration::from_secs(3))
        .build()?;
    let mut response = client
        .get(format!("http://127.0.0.1:{WEB_PORT}/api/getPlayer"))
        .send()
        .await
        .map_err(|_| "Native FRM response observation failed")?;
    if !response.status().is_success() {
        return Err("Native FRM response observation was not successful".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "Native FRM response observation was incomplete")?
    {
        if bytes.len().saturating_add(chunk.len()) > 1024 * 1024 {
            return Err("Native FRM response exceeds its fixture byte limit".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    require_empty_array(&bytes)?;
    Ok(bytes.len())
}

fn require_empty_array(bytes: &[u8]) -> Result<()> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| "Native FRM did not return JSON")?;
    if !value.as_array().is_some_and(Vec::is_empty) {
        return Err(
            "The new no-client FRM world must return an actual empty JSON player array".into(),
        );
    }
    Ok(())
}

#[derive(Default)]
struct LoadEvidence {
    preinit: bool,
    initialized: bool,
    sml: bool,
    frm: bool,
    player_endpoint: bool,
}
impl LoadEvidence {
    fn observe(&mut self, line: &str) {
        // SML v3.12.0 SatisfactoryModLoader.cpp and ModLoadingLibrary.cpp;
        // FRM tag 1.5.4-515 FicsitRemoteMonitoring.cpp, InitAPIRegistry().
        if let Some((_, message)) = line.split_once("LogSatisfactoryModLoader: Display:") {
            let message = message.trim();
            self.preinit |= message
                .strip_prefix("Satisfactory Mod Loader v.")
                .and_then(|value| value.split_whitespace().next())
                .is_some_and(|value| version(value, "3.12.0"))
                && message.ends_with("pre-initializing...");
            self.initialized |= message == "Initialization finished!";
            self.sml |= message
                .strip_prefix("SML:")
                .is_some_and(|value| version(value.trim(), "3.12.0"));
            self.frm |= message
                .strip_prefix("FicsitRemoteMonitoring:")
                .is_some_and(|value| version(value.trim(), "1.5.4"));
        }
        self.player_endpoint |= line
            .split_once("LogHttpServer:")
            .is_some_and(|(_, message)| {
                message.trim().starts_with(
                    "Registered API Endpoint: getPlayer - Current number of endpoints registered:",
                )
            });
    }
    fn complete(&self) -> bool {
        self.preinit && self.initialized && self.sml && self.frm && self.player_endpoint
    }
}
fn version(actual: &str, expected: &str) -> bool {
    actual == expected
        || actual
            .strip_prefix(expected)
            .is_some_and(|tail| tail.starts_with('+'))
}

fn load_evidence(path: &Path) -> Result<LoadEvidence> {
    let mut tail = crate::runtime_log_stream::RuntimeLogTailState::default();
    let mut evidence = LoadEvidence::default();
    for _ in 0..16 {
        let delta = crate::runtime_log_stream::read_runtime_log_delta_bounded(
            path,
            &mut tail,
            1024 * 1024,
            64 * 1024,
        )?;
        if delta.stream_error.is_some() {
            return Err("Satisfactory FRM current-run loading evidence is incomplete".into());
        }
        for line in &delta.lines {
            evidence.observe(line);
        }
        if !delta.limit_exhausted {
            return Ok(evidence);
        }
    }
    Err("Satisfactory FRM current-run loading evidence exceeds its byte limit".into())
}

#[cfg(test)]
#[path = "commands_native_satisfactory_frm_tests.rs"]
mod tests;
