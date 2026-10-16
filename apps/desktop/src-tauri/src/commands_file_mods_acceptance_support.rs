use super::*;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub(super) type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

pub(super) fn plain(path: &Path) -> Result<PathBuf> {
    for ancestor in path.ancestors() {
        let metadata = fs::symlink_metadata(ancestor)?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err("acceptance paths cannot cross reparse points".into());
            }
        }
        if metadata.file_type().is_symlink() {
            return Err("acceptance paths cannot cross symbolic links".into());
        }
    }
    Ok(path.canonicalize()?)
}

pub(super) fn input(name: &str) -> Result<PathBuf> {
    plain(Path::new(
        &std::env::var_os(name).ok_or_else(|| format!("supply {name}"))?,
    ))
}

pub(super) fn write_new(path: &Path, bytes: &[u8]) -> Result {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

pub(super) fn hash(path: &Path) -> Result<String> {
    let mut file = fs::File::open(plain(path)?)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

pub(super) fn hashes(root: &Path) -> Result<BTreeMap<String, String>> {
    if !root.exists() {
        return Ok(BTreeMap::new());
    }
    let mut pending = vec![plain(root)?];
    let mut files = BTreeMap::new();
    let mut entries = 0usize;
    while let Some(path) = pending.pop() {
        entries += 1;
        if entries > 20_000 {
            return Err("acceptance hash tree exceeds its bound".into());
        }
        let path = plain(&path)?;
        if path.is_dir() {
            for entry in fs::read_dir(path)? {
                pending.push(entry?.path());
            }
        } else {
            let relative = path
                .strip_prefix(plain(root)?)?
                .to_string_lossy()
                .replace('\\', "/");
            files.insert(relative, hash(&path)?);
        }
    }
    Ok(files)
}

pub(super) fn source_hashes(source: &Path) -> Result<BTreeMap<String, String>> {
    hashes(source)
}

pub(super) fn ports(details: &InstanceDetails) -> Result<Vec<PortBinding>> {
    for _ in 0..32 {
        let game = UdpSocket::bind("0.0.0.0:0")?;
        let port = game.local_addr()?.port();
        let Some(next) = port.checked_add(1) else {
            continue;
        };
        let Ok(_query) = UdpSocket::bind(("0.0.0.0", next)) else {
            continue;
        };
        let mut result = details.ports.clone();
        if result.len() != 2 {
            return Err("review changed Valheim port schema".into());
        }
        for binding in &mut result {
            binding.port = match binding.name.as_str() {
                "game" => port,
                "query" => next,
                _ => return Err("review changed Valheim port name".into()),
            };
        }
        return Ok(result);
    }
    Err("no independent UDP port pair available".into())
}

fn read_log(path: &Path, details: &InstanceDetails) -> Result<String> {
    let mut bytes = Vec::new();
    fs::File::open(plain(path)?)?
        .take(8 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err("native log exceeds its evidence bound".into());
    }
    let mut text = String::from_utf8_lossy(&bytes).into_owned();
    if let Some(settings) = serde_json::from_str::<Value>(&details.settings_json)?.as_object() {
        for (key, value) in settings {
            if ["password", "secret", "token"]
                .iter()
                .any(|needle| key.to_ascii_lowercase().contains(needle))
                && let Some(secret) = value.as_str().filter(|secret| !secret.is_empty())
            {
                text = text.replace(secret, "[redacted fixture secret]");
            }
        }
    }
    Ok(text)
}

pub(super) struct Cycle<'a, 's> {
    pub state: &'a tauri::State<'s, DesktopState>,
    pub storage: &'a StorageBootstrap,
    pub descriptor: &'a ModuleDescriptor,
    pub smoke: &'a readiness::SmokeManifest,
    pub package: &'a package::NativePackage,
    pub runtime: &'a Path,
    pub evidence: &'a Path,
}

// These exact rendering messages were also present in the unmodded native
// 1.0.17 run. BepInEx mirrors Unity's headless graphics output into its log;
// neither a different channel nor an unknown Unity error is exempted.
fn observed_native_render_error(line: &str) -> bool {
    let Some(message) = line.strip_prefix("[Error  : Unity Log] ") else {
        return false;
    };
    if [
        "AsyncResourceUpload failed.",
        "This custom render path shader needs to have at least 1 passes.",
        "Could not find material Hidden/VideoDecode. Make sure the Video shaders are included in your build, in the Built-in Shader Settings section of the Graphics Settings.",
        "Could not find material Hidden/VideoComposite. Make sure the Video shaders are included in your build, in the Built-in Shader Settings section of the Graphics Settings.",
        "Could not find video decode shader pass YCbCr_To_RGB1 in shader <not found>",
        "Could not find video decode shader pass YCbCrA_To_RGBAFull in shader <not found>",
        "Could not find video decode shader pass YCbCrA_To_RGBA in shader <not found>",
        "Could not find video decode shader pass Flip_RGBA_To_RGBA in shader <not found>",
        "Could not find video decode shader pass Flip_RGBASplit_To_RGBA in shader <not found>",
        "Could not find video decode shader pass Flip_NV12_To_RGB1 in shader <not found>",
        "Could not find video decode shader pass Flip_NV12_To_RGBA in shader <not found>",
        "Could not find video decode shader pass Default in shader <not found>",
    ].contains(&message) {
        return true;
    }
    message
        .strip_suffix(": Failed to play intro cinematic")
        .is_some_and(|stamp| {
            stamp.len() == 19
                && stamp.bytes().enumerate().all(|(index, byte)| match index {
                    2 | 5 => byte == b'/',
                    10 => byte == b' ',
                    13 | 16 => byte == b':',
                    _ => byte.is_ascii_digit(),
                })
        })
}

fn unexpected_loader_error(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    (lower.contains("[error")
        || lower.contains("[fatal")
        || lower.contains("error loading")
        || lower.contains("harmonyexception")
        || lower.contains("patching exception")
        || lower.contains("exception in awake"))
        && !observed_native_render_error(line)
}

#[test]
fn native_render_baseline_does_not_hide_loader_failures() {
    assert!(!unexpected_loader_error(
        "[Error  : Unity Log] AsyncResourceUpload failed."
    ));
    assert!(!unexpected_loader_error(
        "[Error  : Unity Log] 10/16/2026 14:32:04: Failed to play intro cinematic"
    ));
    for line in [
        "[Error  : BepInEx] AsyncResourceUpload failed.",
        "[Error  : Unity Log] NullReferenceException in plugin Awake",
        "[Error  : Unity Log] Could not find video decode shader pass UNKNOWN in shader <not found>",
        "[Fatal  : BepInEx] Could not run preloader!",
        "HarmonyException: patch failed",
    ] {
        assert!(unexpected_loader_error(line), "{line}");
    }
}

// Fresh production run identities and a fresh chainloader log establish absence
// only after native readiness, never merely because a process or file exists.
pub(super) async fn run_cycle(
    cycle: &Cycle<'_, '_>,
    details: &InstanceDetails,
    phase: &str,
    should_load: bool,
) -> Result<Value> {
    let core_before_start = hashes(&cycle.runtime.join("BepInEx/core"))?;
    if !core_before_start.contains_key("BepInEx.Preloader.dll") {
        return Err(format!("{phase}: installed preloader is missing before startup").into());
    }
    for relative in hashes(&cycle.runtime.join("valheim_server_Data/Managed"))?.keys() {
        let path = cycle
            .runtime
            .join("valheim_server_Data/Managed")
            .join(relative);
        // Mono/Cecil may report DirectoryNotFound for an existing long path.
        if path.to_string_lossy().encode_utf16().count() >= 260 {
            return Err(format!(
                "{phase}: isolated managed assembly path exceeds Mono MAX_PATH: {}",
                path.display()
            )
            .into());
        }
    }
    write_new(
        &cycle.evidence.join(format!("{phase}-before-start.json")),
        &serde_json::to_vec_pretty(
            &json!({"runtime":cycle.runtime,"loader_core_sha256":core_before_start,
            "winhttp_sha256":hash(&cycle.runtime.join("winhttp.dll"))?,
            "doorstop_config_sha256":hash(&cycle.runtime.join("doorstop_config.ini"))?}),
        )?,
    )?;
    let loader_log = cycle.runtime.join("BepInEx/LogOutput.log");
    if loader_log.exists() {
        let destination = cycle.evidence.join(format!("{phase}-previous-loader.log"));
        // Evidence and managed scratch may reside on different volumes. Only
        // remove the stopped fixture's log after its evidence is flushed.
        let previous = read_log(&loader_log, details)?;
        write_new(&destination, previous.as_bytes())?;
        fs::remove_file(plain(&loader_log)?)?;
    }
    let baselines = cycle.smoke.log_baselines(details, cycle.runtime)?;
    cycle.package.cleanup_allowed.store(false, Ordering::SeqCst);
    let started = start_instance_process_after_reconcile(
        None,
        cycle.state,
        cycle.storage,
        details.summary.id.clone(),
        "manual",
        None,
    )
    .await?;
    let running = read_instance_details(&cycle.storage.paths, &details.summary.id).await?;
    let observation = cycle
        .smoke
        .wait_ready(
            cycle.descriptor,
            &running,
            cycle.runtime,
            &cycle.package.root,
            baselines,
        )
        .await;
    // Stop even on a readiness failure. The outer NativeCleanup owns error paths
    // that return before this point and never targets another DesktopState.
    let stopped = stop_instance_process(cycle.state.clone(), details.summary.id.clone()).await;
    stopped?;
    if read_active_instance_run(&cycle.storage.paths, &details.summary.id)
        .await?
        .is_some()
        || cycle
            .state
            .runtime_supervisor
            .lock()
            .map_err(|_| "runtime supervisor lock poisoned")?
            .is_tracked(&details.summary.id)
        || !cycle.state.pending_runtime_start_instance_ids()?.is_empty()
    {
        return Err("owned native shutdown left an active or pending process".into());
    }
    cycle.package.cleanup_allowed.store(true, Ordering::SeqCst);
    for (index, process) in started.processes.iter().enumerate() {
        if Path::new(&process.log_path).is_file() {
            let text = read_log(Path::new(&process.log_path), details)?;
            write_new(
                &cycle.evidence.join(format!("{phase}-process-{index}.log")),
                text.as_bytes(),
            )?;
        }
    }
    for entry in fs::read_dir(cycle.runtime)? {
        let path = entry?.path();
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("preloader_") && name.ends_with(".log"))
        {
            let text = read_log(&path, details)?;
            write_new(
                &cycle.evidence.join(format!(
                    "{phase}-{}",
                    path.file_name()
                        .ok_or("preloader log has no name")?
                        .to_string_lossy()
                )),
                text.as_bytes(),
            )?;
        }
    }
    let text = read_log(&loader_log, details).map_err(|error| format!("{phase}: fresh BepInEx loader log unavailable: {error}; inspect retained process/preloader evidence"))?;
    write_new(
        &cycle.evidence.join(format!("{phase}-loader.log")),
        text.as_bytes(),
    )?;
    observation?;
    let loaded = text
        .lines()
        .any(|line| line.contains("Loading [Server Devcommands "));
    if !text.contains("Chainloader startup complete") || loaded != should_load {
        return Err(format!("{phase}: expected Mod loaded={should_load}, observed={loaded}; inspect fresh loader evidence").into());
    }
    if should_load
        && fs::metadata(plain(
            &cycle.runtime.join("BepInEx/config/server_devcommands.cfg"),
        )?)?
        .len()
            == 0
    {
        return Err(format!("{phase}: plugin did not initialize its real configuration").into());
    }
    let failures = text
        .lines()
        .filter(|line| unexpected_loader_error(line))
        .take(20)
        .collect::<Vec<_>>();
    if !failures.is_empty() {
        return Err(format!(
            "{phase}: native loader/plugin errors observed: {}",
            failures.join(" | ")
        )
        .into());
    }
    let run_id = running
        .active_run
        .as_ref()
        .ok_or("native ready process has no run identity")?
        .run_id;
    Ok(
        json!({"phase":phase,"run_id":run_id,"native_ready":true,"chainloader_complete":true,
        "server_devcommands_loaded":loaded,"processes_stopped":true,"scope":"production_start_stop",
        "native_render_messages_also_observed_without_mods":text.lines().filter(|line| observed_native_render_error(line)).count(),
        "unexpected_loader_plugin_errors":0}),
    )
}
