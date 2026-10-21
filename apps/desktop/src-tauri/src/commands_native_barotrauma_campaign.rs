//! Real console campaign setup, restricted to the disposable manual-save test.
//! No game state, save contents, native fields or production protocol is mocked.
use super::*;
use app_core::ProcessIdentity;
use serde::Deserialize;
use std::io::{Read, Seek, SeekFrom};
use std::process::{Child, Command, Stdio};

const SAVE_NAME: &str = "lgsm_native_manual_save";
const MAX_LOG: u64 = 1024 * 1024;
const EMPTY_KNOWN_FOLDER: &str = "Daedalic Entertainment GmbH/Barotrauma/Multiplayer";
const FOLDER_PROBE: &str = r#"
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
$stage='open'; $f=$null; $r=$null
try {
  $s=[Xml.XmlReaderSettings]::new()
  $s.DtdProcessing=[Xml.DtdProcessing]::Prohibit; $s.XmlResolver=$null
  $f=[IO.File]::OpenRead($env:LGSM_BARO_PROBE_CONFIG)
  $stage='parse'; $r=[Xml.XmlReader]::Create($f,$s)
  $d=[Xml.XmlDocument]::new(); $d.XmlResolver=$null; $d.Load($r)
  $stage='read_config'
  if($d.DocumentElement.Name -ne 'config'){throw 'invalid root'}
  $savepath=$d.DocumentElement.GetAttribute('savepath')
  $stage='known_folder'
  $local=[Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData)
  $default=[IO.Path]::Combine($local,'Daedalic Entertainment GmbH','Barotrauma','Multiplayer')
  @{savepath=$savepath;default_multiplayer=$default;local_app_data_empty=($local.Length -eq 0)}|ConvertTo-Json -Compress
} catch {
  [Console]::Error.WriteLine('LGSM_BARO_PROBE_FAILURE stage='+$stage+' type='+$_.Exception.GetBaseException().GetType().FullName)
  exit 1
} finally {
  if($null -ne $r){$r.Dispose()}; if($null -ne $f){$f.Dispose()}
}
"#;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeRoots {
    savepath: PathBuf,
    default_multiplayer: PathBuf,
    local_app_data_empty: bool,
}

struct Owner {
    pid: u32,
    identity: ProcessIdentity,
    run_id: i64,
    log: PathBuf,
}

pub(super) async fn prepare<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    running: &InstanceDetails,
    cycle: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if running.summary.module_id != "barotrauma"
        || std::env::var("LANGAME_NATIVE_MANUAL_SAVE").as_deref() != Ok("true")
    {
        return Ok(());
    }
    if !matches!(cycle, "initial" | "restart") {
        return Err("Unsupported native campaign fixture cycle".into());
    }
    let _operation = runtime
        .state
        .begin_storage_context_operation("isolated native Barotrauma campaign setup")?;
    let fixture = runtime.package.root.canonicalize()?;
    let config = Path::new(&running.config_file_path)
        .parent()
        .ok_or("Campaign fixture has no configuration directory")?
        .canonicalize()?;
    let saves = Path::new(&running.saves_path).canonicalize()?;
    if config == fixture || !config.starts_with(&fixture) || saves != config.join("Multiplayer") {
        return Err("Campaign setup requires the isolated native save directory".into());
    }
    let player_config = config.join("config_player.xml");
    ordinary_file(&player_config, 1024 * 1024)?;
    let native = read_native_roots(&player_config).await?;
    if !native.savepath.is_absolute() || native.savepath.canonicalize()? != config {
        return Err("Native config_player.xml does not force the fixture save root".into());
    }
    let (default_multiplayer, must_exist) = default_multiplayer(&native, &config)?;
    // GetSaveFiles also enumerates its default despite a custom savepath. An
    // external KnownFolder must already exist and contain no user campaigns.
    // Only the verified relative default may be created inside this fixture.
    require_empty_fallback(&default_multiplayer, must_exist)?;
    require_empty_fallback(&config.join("Data/Saves/Multiplayer"), false)?;
    let save = saves.join(format!("{SAVE_NAME}.save"));
    validate_campaign_files(&saves, &save, cycle == "initial")?;
    let run = running
        .active_run
        .as_ref()
        .ok_or("Campaign has no managed run")?;
    let primary = run
        .processes
        .iter()
        .find(|process| process.is_primary)
        .ok_or("Campaign has no primary process")?;
    let owner = Owner {
        pid: primary.pid.ok_or("Campaign has no native PID")?,
        identity: primary
            .process_identity
            .clone()
            .ok_or("Campaign has no native process identity")?,
        run_id: run.run_id,
        log: PathBuf::from(
            primary
                .log_path
                .as_ref()
                .ok_or("Campaign has no console log")?,
        )
        .canonicalize()?,
    };
    let image = Path::new(&owner.identity.image_path).canonicalize()?;
    if !owner.log.starts_with(&fixture) || image.parent() != Some(config.as_path()) {
        return Err("Campaign process or console escaped its fixture".into());
    }
    let offset = send(runtime, running, &owner, "gamemode campaign").await?;
    wait(
        runtime,
        running,
        &owner,
        offset,
        "Do you want to start a new campaign? Y/N",
    )
    .await?;
    let round_offset = if cycle == "initial" {
        let offset = send(runtime, running, &owner, "y").await?;
        wait(
            runtime,
            running,
            &owner,
            offset,
            "Enter a save name for the campaign:",
        )
        .await?;
        // Recheck absence immediately before the only fresh-world creation.
        validate_campaign_files(&saves, &save, true)?;
        let offset = send(runtime, running, &owner, SAVE_NAME).await?;
        wait(runtime, running, &owner, offset, "Campaign started!").await?;
        offset
    } else {
        let offset = send(runtime, running, &owner, "n").await?;
        let text = wait(
            runtime,
            running,
            &owner,
            offset,
            "Select a save file to load (0 - 0):",
        )
        .await?;
        validate_selector(&text, &save)?;
        let offset = send(runtime, running, &owner, "0").await?;
        wait(runtime, running, &owner, offset, "Campaign loaded!").await?;
        offset
    };
    // Both native creation and loading start a round. EndGame synchronously
    // clears GameSession.IsRunning and Server.GameStarted before logging this
    // completion; the formal save's owner-thread preflight rechecks both flags.
    // This native round teardown is not attributed to the manual-save action.
    wait(runtime, running, &owner, round_offset, "Round started").await?;
    let offset = send(runtime, running, &owner, "endgame").await?;
    wait(runtime, running, &owner, offset, "Round ended.").await?;
    validate_campaign_files(&saves, &save, false)?;
    println!(
        "NATIVE_BAROTRAUMA_CAMPAIGN cycle={cycle} phase=lobby_ready source=native_console native_lifecycle_saves=not_attributed_to_manual_action"
    );
    Ok(())
}

fn default_multiplayer(
    native: &NativeRoots,
    config: &Path,
) -> Result<(PathBuf, bool), Box<dyn std::error::Error>> {
    if native.default_multiplayer.is_absolute() && !native.local_app_data_empty {
        return Ok((native.default_multiplayer.clone(), true));
    }
    if native.local_app_data_empty && native.default_multiplayer == Path::new(EMPTY_KNOWN_FOLDER) {
        // In the reviewed .NET 8 package, an unavailable KnownFolder yields
        // exactly this relative default. Native Program.Main sets cwd to the
        // entry assembly directory; the primary image must share this config
        // directory before any command is sent. The native selector may create
        // this empty fixture directory, without creating a user-profile folder.
        return Ok((config.join(EMPTY_KNOWN_FOLDER), false));
    }
    Err("Native default folder is not an absolute or verified empty-KnownFolder location".into())
}

fn reparse(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

fn ordinary_file(path: &Path, maximum: u64) -> Result<(), Box<dyn std::error::Error>> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || reparse(&metadata) || metadata.len() == 0 || metadata.len() > maximum
    {
        return Err("Native campaign probe requires an ordinary bounded file".into());
    }
    Ok(())
}

fn campaign_files(directory: &Path) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let mut files = Vec::new();
    for (index, entry) in fs::read_dir(directory)?.enumerate() {
        if index >= 1024 {
            return Err("Campaign directory exceeds bounded discovery".into());
        }
        let entry = entry?;
        if reparse(&fs::symlink_metadata(entry.path())?) {
            return Err("Campaign directory contains a redirected entry".into());
        }
        if entry
            .path()
            .extension()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|ext| ext.eq_ignore_ascii_case("save"))
        {
            ordinary_file(&entry.path(), 64 * 1024 * 1024)?;
            files.push(entry.path());
        }
    }
    Ok(files)
}

fn require_empty_fallback(path: &Path, must_exist: bool) -> Result<(), Box<dyn std::error::Error>> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && !must_exist => return Ok(()),
        Err(_) => return Err("Native fallback save directory cannot be safely inspected".into()),
        Ok(metadata) if !metadata.is_dir() || reparse(&metadata) => {
            return Err("Native fallback save directory is not an ordinary directory".into());
        }
        Ok(_) => {}
    }
    // Only directory metadata/counts are inspected here, never user-save bytes.
    for (index, entry) in fs::read_dir(path)?.enumerate() {
        if index >= 1024 {
            return Err("Native fallback save directory exceeds bounded discovery".into());
        }
        if entry?
            .path()
            .extension()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|ext| ext.eq_ignore_ascii_case("save"))
        {
            return Err("Native fallback contains campaigns; isolated selector refused".into());
        }
    }
    Ok(())
}

fn validate_campaign_files(
    directory: &Path,
    expected: &Path,
    initial: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let files = campaign_files(directory)?;
    if initial {
        if !files.is_empty() {
            return Err("Fresh native campaign would reuse existing world data".into());
        }
    } else if files.len() != 1 || files[0] != expected {
        return Err("Native campaign fixture does not contain its unique expected save".into());
    }
    Ok(())
}

fn validate_selector(text: &str, expected: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let choices = text
        .lines()
        .filter_map(|line| {
            let (index, path) = line.trim().split_once(". ")?;
            let index = index.parse::<usize>().ok()?;
            path.to_ascii_lowercase()
                .ends_with(".save")
                .then_some((index, path))
        })
        .collect::<Vec<_>>();
    let expected_text = selector_path_spelling(&expected.to_string_lossy());
    if choices.len() != 1
        || choices[0].0 != 0
        || selector_path_spelling(choices[0].1) != expected_text
        || Path::new(choices[0].1).canonicalize()? != expected.canonicalize()?
    {
        return Err("Native campaign selector is not exclusively the current fixture save".into());
    }
    Ok(())
}

fn selector_path_spelling(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    normalized
        .strip_prefix("//?/")
        .unwrap_or(&normalized)
        .to_ascii_lowercase()
}

struct OwnedProbeChild(Child);

impl Drop for OwnedProbeChild {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn probe_failure_detail(bytes: &[u8]) -> Option<String> {
    if bytes.len() > 4096 {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?;
    let line = text
        .lines()
        .find_map(|line| line.strip_prefix("LGSM_BARO_PROBE_FAILURE stage="))?;
    let (stage, exception) = line.split_once(" type=")?;
    if !matches!(stage, "open" | "parse" | "read_config" | "known_folder")
        || exception.is_empty()
        || exception.len() > 128
        || !exception
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.')
    {
        return None;
    }
    Some(format!("stage={stage} type={exception}"))
}

async fn read_native_roots(config: &Path) -> Result<NativeRoots, Box<dyn std::error::Error>> {
    let config = config.to_owned();
    tokio::task::spawn_blocking(move || {
        // Windows KnownFolder must come from the same native API used by .NET,
        // not LOCALAPPDATA/USERPROFILE environment overrides. XML DTD/entity
        // resolution is disabled. Open a file stream: XmlReader's string overload
        // treats canonical \\?\ paths as URIs. Only closed failure metadata logs.
        let mut command = Command::new("powershell.exe");
        command
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                FOLDER_PROBE,
            ])
            .env("LGSM_BARO_PROBE_CONFIG", config)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        let mut child = OwnedProbeChild(
            command
                .spawn()
                .map_err(|_| "Cannot start bounded native-folder probe")?,
        );
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            match child
                .0
                .try_wait()
                .map_err(|_| "Cannot inspect native-folder probe")?
            {
                Some(status) => {
                    if !status.success() {
                        let mut bytes = Vec::new();
                        if let Some(stderr) = child.0.stderr.take() {
                            let _ = stderr.take(4097).read_to_end(&mut bytes);
                        }
                        return Err(match probe_failure_detail(&bytes) {
                            Some(detail) => {
                                format!("Native folder/configuration probe failed: {detail}")
                            }
                            None => "Native folder/configuration probe failed".to_owned(),
                        });
                    }
                    let mut bytes = Vec::new();
                    child
                        .0
                        .stdout
                        .take()
                        .ok_or("Native folder probe has no output")?
                        .take(64 * 1024 + 1)
                        .read_to_end(&mut bytes)
                        .map_err(|_| "Native folder output unavailable")?;
                    if bytes.len() > 64 * 1024 {
                        return Err("Native folder output exceeded its bound".to_owned());
                    }
                    return serde_json::from_slice(&bytes)
                        .map_err(|_| "Native folder output is invalid".to_owned());
                }
                None if Instant::now() >= until => {
                    return Err("Native folder/configuration probe exceeded ten seconds".to_owned());
                }
                None => std::thread::sleep(Duration::from_millis(25)),
            }
        }
    })
    .await
    .map_err(|_| "Native folder inspection worker failed")?
    .map_err(Into::into)
}

async fn verify_owner<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    running: &InstanceDetails,
    owner: &Owner,
) -> Result<(), Box<dyn std::error::Error>> {
    let current = read_active_instance_run(&runtime.storage.paths, &running.summary.id)
        .await?
        .ok_or("Native campaign run stopped")?;
    if current.run_id != owner.run_id
        || !current.processes.iter().any(|process| {
            process.pid == Some(owner.pid)
                && process.process_identity.as_ref() == Some(&owner.identity)
        })
        || !app_runtime::process_matches_identity(owner.pid, &owner.identity)?
    {
        return Err("Native campaign process generation changed".into());
    }
    Ok(())
}

async fn send<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    running: &InstanceDetails,
    owner: &Owner,
    command: &str,
) -> Result<u64, Box<dyn std::error::Error>> {
    verify_owner(runtime, running, owner).await?;
    let offset = fs::metadata(&owner.log)?.len();
    let response = crate::commands::send_instance_runtime_command(
        runtime.state.clone(),
        serde_json::from_value(json!({"instanceId":running.summary.id,"command":command}))?,
    )
    .await?;
    if response.instance_id != running.summary.id
        || response.pid != owner.pid
        || response.process_key != "main"
    {
        return Err("Native campaign setup command reached another process".into());
    }
    Ok(offset)
}

async fn wait<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    running: &InstanceDetails,
    owner: &Owner,
    offset: u64,
    marker: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let until = Instant::now() + Duration::from_secs(90);
    loop {
        verify_owner(runtime, running, owner).await?;
        let mut file = fs::File::open(&owner.log)?;
        if file.metadata()?.len() < offset {
            return Err("Native campaign log was replaced".into());
        }
        file.seek(SeekFrom::Start(offset))?;
        let mut bytes = Vec::new();
        file.take(MAX_LOG + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_LOG {
            return Err("Native campaign setup exceeded bounded output".into());
        }
        let text = String::from_utf8_lossy(&bytes);
        if text.contains(marker) {
            return Ok(text.into_owned());
        }
        if Instant::now() >= until {
            return Err("Native campaign setup did not reach its expected state".into());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[cfg(test)]
#[path = "commands_native_barotrauma_campaign_tests.rs"]
mod tests;
