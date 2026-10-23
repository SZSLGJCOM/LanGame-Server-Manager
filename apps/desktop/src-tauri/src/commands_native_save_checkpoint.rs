//! Test-only checkpoint between the formal save and normal shutdown.
//! Restoring this point prevents shutdown saves from paying the manual-save debt.
use super::*;
use crate::commands::commands_managed_save_control::{guard_pin_directories, guard_read_regular};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::{Read, Write};

use super::super::native_logs as log_cursor;
#[path = "commands_native_dragon_save_observation.rs"]
pub(super) mod dragon;
#[path = "commands_native_moria_save_observation.rs"]
pub(super) mod moria;
#[path = "commands_native_necesse_checkpoint.rs"]
pub(super) mod necesse;
#[path = "commands_native_save_checkpoint_profiles.rs"]
mod profiles;
#[path = "commands_native_requested_save_observation.rs"]
pub(super) mod requested;
#[path = "commands_native_restored_world.rs"]
pub(super) mod restored_world;
#[path = "commands_native_save_checkpoint_sqlite.rs"]
mod sqlite;
#[path = "commands_native_terraria_checkpoint.rs"]
pub(super) mod terraria;

const MAX_FILES: usize = 20_000;
const MAX_BYTES: u64 = 1024 * 1024 * 1024;
type Manifest = BTreeMap<PathBuf, (u64, String)>;
static CHECKPOINTS: OnceLock<Mutex<BTreeMap<String, Checkpoint>>> = OnceLock::new();

struct Checkpoint {
    fixture: PathBuf,
    saves: PathBuf,
    copy: PathBuf,
    manifest: Manifest,
    run_id: i64,
    native_before: String,
    capture_logs: Vec<PathBuf>,
    logs: Vec<(PathBuf, log_cursor::LogBaseline)>,
    restored: bool,
    read_back: bool,
}

pub(super) fn enabled() -> Result<bool, String> {
    match std::env::var("LANGAME_NATIVE_EXACT_SAVE_READBACK") {
        Err(std::env::VarError::NotPresent) => Ok(false),
        Ok(value)
            if value == "true"
                && std::env::var("LANGAME_NATIVE_MANUAL_SAVE").as_deref() == Ok("true")
                && std::env::var("LANGAME_NATIVE_MANUAL_SAVE_RESTART").as_deref() == Ok("true") =>
        {
            Ok(true)
        }
        _ => Err("Exact checkpoint readback requires manual save and restart opt-ins".into()),
    }
}

fn checkpoints() -> Result<std::sync::MutexGuard<'static, BTreeMap<String, Checkpoint>>, String> {
    CHECKPOINTS
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .map_err(|_| "Native checkpoint ownership lock failed".into())
}

fn input<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    details: &InstanceDetails,
) -> Result<(PathBuf, InstanceDetails), String> {
    if details.summary.module_id != runtime.descriptor.summary.id {
        return Err("Checkpoint module differs from its disposable fixture".into());
    }
    Ok((runtime.package.root.clone(), details.clone()))
}

async fn worker(
    work: impl FnOnce() -> Result<(), String> + Send + 'static,
) -> Result<(), Box<dyn std::error::Error>> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| format!("Native checkpoint worker failed: {error}"))?
        .map_err(Into::into)
}

fn binding(
    fixture: &Path,
    details: &InstanceDetails,
) -> Result<(PathBuf, PathBuf, PathBuf), String> {
    let _fixture_pins = guard_pin_directories(fixture)?;
    let _save_pins = guard_pin_directories(Path::new(&details.saves_path))?;
    let _config_pins = guard_pin_directories(
        Path::new(&details.config_file_path)
            .parent()
            .ok_or("Checkpoint configuration has no parent")?,
    )?;
    let fixture = dunce::canonicalize(fixture).map_err(|_| "Checkpoint fixture unavailable")?;
    let saves =
        dunce::canonicalize(&details.saves_path).map_err(|_| "Checkpoint saves unavailable")?;
    let program = dunce::canonicalize(creation::program_root(details).map_err(|e| e.to_string())?)
        .map_err(|_| "Checkpoint program unavailable")?;
    let config = dunce::canonicalize(&details.config_file_path)
        .map_err(|_| "Checkpoint config unavailable")?;
    if saves == fixture
        || !saves.starts_with(&fixture)
        || !program.starts_with(&fixture)
        || !config.starts_with(&fixture)
    {
        return Err("Exact checkpoint requires the disposable owned instance".into());
    }
    let _pins = guard_pin_directories(&saves)?;
    Ok((fixture, saves, program))
}

fn relative(root: &Path, path: &Path) -> Result<PathBuf, String> {
    let path = path
        .strip_prefix(root)
        .map_err(|_| "Checkpoint path escaped its save binding")?;
    if path
        .components()
        .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return Err("Checkpoint paths must be ordinary relative paths".into());
    }
    Ok(path.to_path_buf())
}

fn ordinary(path: &Path) -> Result<fs::Metadata, String> {
    let metadata = fs::symlink_metadata(path).map_err(|_| "Checkpoint entry unavailable")?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("Checkpoint entries cannot be redirected".into());
        }
    }
    if metadata.file_type().is_symlink() || (!metadata.is_dir() && !metadata.is_file()) {
        return Err("Checkpoint entries must be ordinary files or directories".into());
    }
    Ok(metadata)
}

fn files(saves: &Path, roots: &[PathBuf]) -> Result<Manifest, String> {
    let manifest = inventory(saves, roots)?;
    if manifest.values().all(|(bytes, _)| *bytes == 0) {
        return Err("The formal save produced no native checkpoint".into());
    }
    Ok(manifest)
}

fn inventory(saves: &Path, roots: &[PathBuf]) -> Result<Manifest, String> {
    if roots.iter().any(|path| path == saves) {
        return Err("Checkpoint roots must retain the bound save directory".into());
    }
    let mut pending = roots
        .iter()
        .cloned()
        .map(|path| (path, 0_u32))
        .collect::<Vec<_>>();
    let mut manifest = Manifest::new();
    let mut total = 0_u64;
    while let Some((path, depth)) = pending.pop() {
        if depth > 32 || manifest.len() + pending.len() > MAX_FILES {
            return Err("Checkpoint inventory exceeds its depth or entry bound".into());
        }
        let name = relative(saves, &path)?;
        let metadata = ordinary(&path)?;
        if metadata.is_dir() {
            let _pins = guard_pin_directories(&path)?;
            for entry in fs::read_dir(&path).map_err(|_| "Checkpoint directory unreadable")? {
                pending.push((
                    entry.map_err(|_| "Checkpoint enumeration failed")?.path(),
                    depth + 1,
                ));
                if pending.len() > MAX_FILES {
                    return Err("Checkpoint entry bound exceeded".into());
                }
            }
        } else {
            total = total
                .checked_add(metadata.len())
                .ok_or("Checkpoint size overflow")?;
            if total > MAX_BYTES {
                return Err("Checkpoint byte bound exceeded".into());
            }
            if manifest.insert(name, digest(&path)?).is_some() {
                return Err("Checkpoint native file families overlap".into());
            }
        }
    }
    Ok(manifest)
}

fn digest(path: &Path) -> Result<(u64, String), String> {
    let _pins = guard_pin_directories(path.parent().ok_or("Checkpoint file has no parent")?)?;
    let metadata = ordinary(path)?;
    if !metadata.is_file() || metadata.len() > MAX_BYTES {
        return Err("Invalid checkpoint file".into());
    }
    let mut file = fs::File::open(path).map_err(|_| "Checkpoint file unreadable")?;
    let mut hash = Sha256::new();
    let mut bytes = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| "Checkpoint read failed")?;
        if count == 0 {
            break;
        }
        bytes += count as u64;
        if bytes > MAX_BYTES {
            return Err("Checkpoint grew beyond its byte bound".into());
        }
        hash.update(&buffer[..count]);
    }
    if bytes != metadata.len() {
        return Err("Checkpoint changed while being read".into());
    }
    Ok((
        bytes,
        hash.finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    ))
}

fn copy_new(source: &Path, target: &Path) -> Result<(), String> {
    let parent = target.parent().ok_or("Checkpoint target has no parent")?;
    fs::create_dir_all(parent).map_err(|_| "Checkpoint target directory unavailable")?;
    let _pins = guard_pin_directories(parent)?;
    let mut input = fs::File::open(source).map_err(|_| "Checkpoint source unavailable")?;
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)
        .map_err(|_| "Checkpoint target already exists or cannot be created")?;
    let bytes = std::io::copy(
        &mut Read::by_ref(&mut input).take(MAX_BYTES + 1),
        &mut output,
    )
    .map_err(|_| "Checkpoint copy failed")?;
    if bytes > MAX_BYTES {
        return Err("Checkpoint copy exceeded its bound".into());
    }
    output.flush().map_err(|_| "Checkpoint flush failed")?;
    output.sync_all().map_err(|_| "Checkpoint sync failed")?;
    Ok(())
}

fn log_parent(path: &Path) -> Result<Vec<fs::File>, String> {
    let mut parent = path.parent().ok_or("Checkpoint log has no parent")?;
    loop {
        match fs::symlink_metadata(parent) {
            Ok(_) => return guard_pin_directories(parent),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                parent = parent
                    .parent()
                    .ok_or("Checkpoint log has no existing ancestor")?;
            }
            Err(_) => return Err("Checkpoint log parent is unavailable".into()),
        }
    }
}

fn log_paths(fixture: &Path, paths: Vec<PathBuf>) -> Result<Vec<PathBuf>, String> {
    for path in &paths {
        relative(fixture, path)?;
        let _pins = log_parent(path)?;
        if path.exists() && !ordinary(path)?.is_file() {
            return Err("Checkpoint log is not an ordinary fixture file".into());
        }
    }
    Ok(paths)
}

fn native_text(paths: &[PathBuf]) -> Result<String, String> {
    let mut text = String::new();
    for path in paths {
        let _pins = log_parent(path)?;
        if !path.exists() {
            continue;
        }
        let bytes = guard_read_regular(path, 8 * 1024 * 1024)?;
        text.push_str(&String::from_utf8_lossy(&bytes));
        text.push('\n');
    }
    Ok(text)
}

pub(super) async fn capture<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    running: &InstanceDetails,
    cycle: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if !enabled()? {
        return Ok(());
    }
    let (fixture, running) = input(runtime, running)?;
    let cycle = cycle.to_owned();
    let handle = tokio::runtime::Handle::current();
    worker(move || handle.block_on(capture_owned(&fixture, &running, &cycle))).await
}

async fn capture_owned(
    fixture: &Path,
    running: &InstanceDetails,
    cycle: &str,
) -> Result<(), String> {
    // This checkpoint belongs to the initial-save/restart pair. Later restore
    // cycles still verify their native save receipts without replacing it.
    if cycle != "initial" {
        return Ok(());
    }
    let (fixture, saves, program) = binding(fixture, running)?;
    let _pins = guard_pin_directories(&saves)?;
    let id = &running.summary.id;
    if checkpoints()?.contains_key(id) {
        return Err("Manual checkpoint was already captured; not replaced".into());
    }
    let roots = profiles::roots(&running.summary.module_id, &saves)?;
    let source_manifest = files(&saves, &roots)?;
    let copy = fixture.join("manual-checkpoint");
    fs::create_dir(&copy).map_err(|error| format!("Create manual checkpoint: {error}"))?;
    // Only game-specific durable families enter the snapshot. Logs, IPC and
    // configuration stay in the stopped fixture and cannot supply stale proof.
    let mut manifest = Manifest::new();
    for (name, expected) in &source_manifest {
        let source = saves.join(name);
        let target = copy.join(name);
        if profiles::is_sqlite(&running.summary.module_id, name) {
            fs::create_dir_all(target.parent().ok_or("Database checkpoint has no parent")?)
                .map_err(|error| format!("Create database checkpoint parent: {error}"))?;
            sqlite::backup(&source, &target).await?;
            manifest.insert(name.clone(), digest(&target)?);
        } else {
            copy_new(&source, &target)?;
            let copied = digest(&target)?;
            if copied != *expected {
                return Err("Native checkpoint changed during capture".into());
            }
            manifest.insert(name.clone(), copied);
        }
    }
    // SQLite owns a read transaction across its backup; its main/WAL bytes may
    // legitimately advance. Other native multi-file families must remain stable.
    let after = files(&saves, &roots)?;
    for (name, expected) in &source_manifest {
        if !profiles::is_sqlite(&running.summary.module_id, name)
            && after.get(name) != Some(expected)
        {
            return Err("Native checkpoint changed before normal shutdown; not retried".into());
        }
    }
    if after.keys().ne(source_manifest.keys()) {
        return Err("Native checkpoint file family changed during capture".into());
    }
    terraria::verify_capture(running)?;
    moria::verify_capture(running)?;
    necesse::verify_capture(running)?;
    let capture_logs = log_paths(
        &fixture,
        profiles::native_logs(&running.summary.module_id, &program, running),
    )?;
    let native_before = native_text(&capture_logs)?;
    let run_id = running
        .active_run
        .as_ref()
        .ok_or("Checkpoint has no active run")?
        .run_id;
    println!(
        "NATIVE_SAVE_CHECKPOINT module={} phase=captured run_id={run_id} files={} captured_before_stop=true scope=isolated_world",
        running.summary.module_id,
        manifest.len()
    );
    for (name, (bytes, sha)) in &manifest {
        println!(
            "NATIVE_SAVE_CHECKPOINT_FILE relative={} bytes={bytes} sha256={sha}",
            name.display()
        );
    }
    checkpoints()?.insert(
        id.clone(),
        Checkpoint {
            fixture,
            saves,
            copy,
            manifest,
            run_id,
            native_before,
            capture_logs,
            logs: Vec::new(),
            restored: false,
            read_back: false,
        },
    );
    Ok(())
}

pub(super) async fn after_stop<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    running: &InstanceDetails,
    cycle: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if !enabled()? || cycle != "initial" {
        return Ok(());
    }
    if read_active_instance_run(&runtime.storage.paths, &running.summary.id)
        .await?
        .is_some()
    {
        return Err("Cannot restore a checkpoint while its managed run exists".into());
    }
    let (fixture, running) = input(runtime, running)?;
    worker(move || restore_owned(&fixture, &running)).await
}

fn restore_owned(fixture: &Path, running: &InstanceDetails) -> Result<(), String> {
    let (fixture, saves, _) = binding(fixture, running)?;
    let _pins = guard_pin_directories(&saves)?;
    let mut owned = checkpoints()?;
    let checkpoint = owned
        .get_mut(&running.summary.id)
        .ok_or("Formal manual checkpoint is absent")?;
    if checkpoint.restored || checkpoint.fixture != fixture || checkpoint.saves != saves {
        return Err("Checkpoint ownership or lifecycle changed".into());
    }
    restore_snapshot(checkpoint, &running.summary.module_id)?;
    println!(
        "NATIVE_SAVE_CHECKPOINT module={} phase=restored after_normal_stop=true shutdown_snapshot_excluded=true sha256_verified=true",
        running.summary.module_id
    );
    Ok(())
}

fn restore_snapshot(checkpoint: &mut Checkpoint, module: &str) -> Result<(), String> {
    let saves = &checkpoint.saves;
    let _pins = guard_pin_directories(saves)?;
    // Reject corrupted or redirected captured data before moving any stopped
    // native files. A failed acceptance retains the original stopped state.
    for (name, expected) in &checkpoint.manifest {
        if digest(&checkpoint.copy.join(name))? != *expected {
            return Err("Captured manual snapshot changed before restore".into());
        }
    }
    let backup = checkpoint.fixture.join("shutdown-checkpoint");
    fs::create_dir(&backup).map_err(|error| format!("Create shutdown checkpoint: {error}"))?;
    // Preserve every post-stop native file instead of deleting it. A distinct
    // directory makes the shutdown result unavailable to the next game start.
    // Enumerate again: stop may create a new numbered generation or recovery
    // member that did not exist at capture. Leaving it would let the loader
    // select the shutdown save instead of the manual point.
    let roots = profiles::roots(module, saves)?;
    files(saves, &roots)?;
    let mut displaced = roots.clone();
    for root in &roots {
        if profiles::is_sqlite(module, &relative(saves, root)?) {
            for suffix in ["-wal", "-shm", "-journal"] {
                let sidecar = PathBuf::from(format!("{}{suffix}", root.display()));
                if sidecar.exists() {
                    displaced.push(sidecar);
                }
            }
        }
    }
    displaced.sort();
    displaced.dedup();
    for source in &displaced {
        let name = relative(saves, source)?;
        ordinary(source)?;
        let target = backup.join(name);
        let parent = target
            .parent()
            .ok_or("Displaced checkpoint has no parent")?;
        fs::create_dir_all(parent)
            .map_err(|error| format!("Create shutdown checkpoint parent: {error}"))?;
        let _target_pins = guard_pin_directories(parent)?;
        fs::rename(source, target)
            .map_err(|error| format!("Preserve shutdown checkpoint: {error}"))?;
    }
    for (name, expected) in &checkpoint.manifest {
        let source = checkpoint.copy.join(name);
        if digest(&source)? != *expected {
            return Err("Captured manual snapshot changed during restore".into());
        }
        copy_new(&source, &saves.join(name))?;
    }
    if files(saves, &profiles::roots(module, saves)?)? != checkpoint.manifest {
        return Err("Restored files differ from the exact manual checkpoint".into());
    }
    checkpoint.restored = true;
    Ok(())
}

pub(super) async fn before_start<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    details: &InstanceDetails,
    cycle: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if !enabled()? || cycle != "restart" {
        return Ok(());
    }
    let (fixture, details) = input(runtime, details)?;
    worker(move || before_start_owned(&fixture, &details)).await
}

fn before_start_owned(fixture: &Path, details: &InstanceDetails) -> Result<(), String> {
    let (fixture, saves, program) = binding(fixture, details)?;
    let _pins = guard_pin_directories(&saves)?;
    let mut owned = checkpoints()?;
    let checkpoint = owned
        .get_mut(&details.summary.id)
        .ok_or("Restart has no formal manual checkpoint")?;
    if !checkpoint.restored
        || checkpoint.read_back
        || checkpoint.fixture != fixture
        || checkpoint.saves != saves
        || files(
            &saves,
            &profiles::roots(&details.summary.module_id, &saves)?,
        )? != checkpoint.manifest
    {
        return Err("Restart did not begin with the exact manual checkpoint".into());
    }
    checkpoint.logs = log_paths(
        &fixture,
        profiles::native_logs(&details.summary.module_id, &program, details),
    )?
    .into_iter()
    .map(|path| {
        let _pins = log_parent(&path)?;
        log_cursor::LogBaseline::capture(&path).map(|baseline| (path, baseline))
    })
    .collect::<Result<_, _>>()?;
    Ok(())
}

pub(super) async fn verify<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    running: &InstanceDetails,
) -> Result<(), Box<dyn std::error::Error>> {
    terraria::verify_loaded(runtime, running, "restart").await?;
    necesse::verify_loaded(runtime, running, "restart").await?;
    let (fixture, running) = input(runtime, running)?;
    worker(move || verify_owned(&fixture, &running)).await
}

fn verify_owned(fixture: &Path, running: &InstanceDetails) -> Result<(), String> {
    let (fixture, saves, _) = binding(fixture, running)?;
    let mut owned = checkpoints()?;
    let checkpoint = owned
        .get_mut(&running.summary.id)
        .ok_or("Restart has no saved checkpoint")?;
    let run = running
        .active_run
        .as_ref()
        .ok_or("Readback has no managed run")?;
    let run_id = run.run_id;
    if !checkpoint.restored
        || checkpoint.read_back
        || checkpoint.fixture != fixture
        || checkpoint.saves != saves
        || run_id == checkpoint.run_id
    {
        return Err(
            "Readback requires a new owned process generation and restored manual checkpoint"
                .into(),
        );
    }
    // Console and Core Keeper native paths are allocated after before_start.
    // Only a different run's new primary logs may start at byte zero.
    for process in &run.processes {
        if !process.is_primary
            || process.run_id != run_id
            || process.session_id != run.session_id
            || process.status != "running"
        {
            continue;
        }
        if let Some(path) = &process.log_path {
            let mut paths = vec![PathBuf::from(path)];
            if running.summary.module_id == "corekeeper" {
                paths.extend(app_core::runtime_logs::corekeeper_native_log_path(
                    Path::new(path),
                ));
            }
            for path in log_paths(&fixture, paths)? {
                if !checkpoint.logs.iter().any(|(known, _)| known == &path) {
                    if checkpoint.capture_logs.contains(&path) {
                        return Err(
                            "Restart reused a managed log without a pre-start baseline".into()
                        );
                    }
                    checkpoint
                        .logs
                        .push((path, log_cursor::LogBaseline::default()));
                }
            }
        }
    }
    if checkpoint.logs.is_empty() {
        return Err("Restart has no bound native load evidence source".into());
    }
    let mut fresh = String::new();
    for (path, baseline) in &mut checkpoint.logs {
        // Check the original binding again after startup, including parents
        // created since the absent-file baseline. Never follow an external log.
        log_paths(&fixture, vec![path.clone()])?;
        let _pins = log_parent(path)?;
        // Traverse every fresh line using the existing rotation-aware bounded
        // cursor. Returning false prevents `any` from stopping at the first line.
        log_cursor::new_log_line_matches(path, baseline, |line| {
            fresh.push_str(line);
            fresh.push('\n');
            false
        })?;
    }
    if running.summary.module_id == "returntomoria" {
        moria::verify_loaded_owned(&fixture, running, "restart")?;
    } else {
        profiles::require_loaded(
            &running.summary.module_id,
            &checkpoint.native_before,
            &fresh,
        )?;
    }
    checkpoint.read_back = true;
    println!(
        "NATIVE_SAVE_READBACK module={} phase=verified initial_run={} restart_run={run_id} exact_manual_checkpoint=true native_load=passed second_save_sent=false scope=isolated_world",
        running.summary.module_id, checkpoint.run_id
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Checkpoint {
        let fixture = crate::commands::tests::temp_test_dir("save-checkpoint");
        let saves = fixture.join("saves");
        let copy = fixture.join("manual-checkpoint");
        fs::create_dir(&saves).unwrap();
        fs::create_dir(&copy).unwrap();
        fs::write(saves.join("world0.ksd"), b"manual A").unwrap();
        copy_new(&saves.join("world0.ksd"), &copy.join("world0.ksd")).unwrap();
        let manifest = files(&saves, &profiles::roots("enshrouded", &saves).unwrap()).unwrap();
        Checkpoint {
            fixture,
            saves,
            copy,
            manifest,
            run_id: 1,
            native_before: String::new(),
            capture_logs: Vec::new(),
            logs: Vec::new(),
            restored: false,
            read_back: false,
        }
    }

    #[tokio::test]
    async fn corekeeper_checkpoint_loads_from_the_native_log_created_by_restart() {
        let root = crate::commands::tests::temp_test_dir("core-checkpoint-restart");
        let root = dunce::canonicalize(&root).unwrap();
        for child in ["config", "runtime", "saves", "logs/managed-console"] {
            fs::create_dir_all(root.join(child)).unwrap();
        }
        fs::write(root.join("runtime/.langame-private-runtime"), b"managed\n").unwrap();
        let config = root.join("config/instance.json");
        let saves = root.join("saves");
        let world = saves.join("world.gzip");
        fs::write(&config, b"{}").unwrap();
        fs::write(&world, b"manual world A").unwrap();
        let loaded = "Decompressed brotli compressed world file:\nDeserialize world finishied\nsuccessfully loaded world file into SerializeWorld\n";
        let console = root.join("logs/managed-console/run-42-main.log");
        let native = app_core::runtime_logs::corekeeper_native_log_path(&console).unwrap();
        fs::write(&console, "previous console\n").unwrap();
        fs::write(&native, loaded).unwrap();
        let legacy = root.join("logs/CoreKeeperServer.log");
        fs::write(&legacy, loaded).unwrap();
        let mut running =
            crate::commands::tests::corekeeper_run_log_tests::running_instance(&console);
        running.summary.id = format!("checkpoint-{}", uuid::Uuid::new_v4());
        running.config_file_path = config.to_string_lossy().into_owned();
        running.saves_path = saves.to_string_lossy().into_owned();
        capture_owned(&root, &running, "initial").await.unwrap();
        fs::write(&world, b"shutdown world B").unwrap();
        restore_owned(&root, &running).unwrap();
        assert_eq!(fs::read(&world).unwrap(), b"manual world A");
        let mut stopped = running.clone();
        stopped.active_run = None;
        before_start_owned(&root, &stopped).unwrap();

        let console = root.join("logs/managed-console/run-43-main.log");
        let native = app_core::runtime_logs::corekeeper_native_log_path(&console).unwrap();
        let run = running.active_run.as_mut().unwrap();
        run.run_id = 43;
        run.session_id = Some("core-restart".into());
        run.log_path = Some(console.to_string_lossy().into_owned());
        run.processes[0].run_id = run.run_id;
        run.processes[0].session_id = run.session_id.clone();
        run.processes[0].log_path = run.log_path.clone();
        fs::write(&console, "current console\n").unwrap();
        fs::write(&native, "Loading world\n").unwrap();
        assert!(
            verify_owned(&root, &running)
                .unwrap_err()
                .contains("fresh native logs do not prove")
        );
        let mut output = fs::OpenOptions::new().append(true).open(&native).unwrap();
        output.write_all(loaded.as_bytes()).unwrap();
        drop(output);
        verify_owned(&root, &running).unwrap();
        assert!(
            checkpoints()
                .unwrap()
                .remove(&running.summary.id)
                .unwrap()
                .read_back
        );
        assert_eq!(fs::read_to_string(legacy).unwrap(), loaded);
        assert_eq!(fs::read(&world).unwrap(), b"manual world A");
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn native_checkpoint_survives_later_lifecycle_save_cycles() {
        let root = crate::commands::tests::temp_test_dir("save-checkpoint-cycles");
        let root = dunce::canonicalize(&root).unwrap();
        for child in ["config", "runtime", "saves", "logs"] {
            fs::create_dir(root.join(child)).unwrap();
        }
        fs::write(root.join("runtime/.langame-private-runtime"), b"managed\n").unwrap();
        let config = root.join("config/instance.json");
        let saves = root.join("saves");
        let world = saves.join("world0.ksd");
        let log = root.join("logs/enshrouded_server.log");
        fs::write(&config, b"{}").unwrap();
        fs::write(&world, b"manual A").unwrap();
        fs::write(&log, b"").unwrap();
        let mut running: InstanceDetails = serde_json::from_value(json!({
            "summary": { "id": format!("checkpoint-{}", uuid::Uuid::new_v4()),
                "name": "fixture", "module_id": "enshrouded", "status": "Running",
                "bind_ip": "127.0.0.1", "port_count": 0, "autostart": false },
            "config_file_path": config, "saves_path": saves,
            "auto_backup_on_stop": false, "backup_retention_count": 0,
            "settings_json": "{}", "ports": [],
            "active_run": { "run_id": 1, "session_id": "first", "pid": null,
                "log_path": null, "process_count": 0, "processes": [] }
        }))
        .unwrap();
        capture_owned(&root, &running, "initial").await.unwrap();
        fs::write(&world, b"shutdown B").unwrap();
        restore_owned(&root, &running).unwrap();
        assert_eq!(fs::read(&world).unwrap(), b"manual A");
        running.active_run.as_mut().unwrap().run_id = 2;
        before_start_owned(&root, &running).unwrap();
        fs::write(&log, b"Found file 'world0.ksd'\nSavegame deserialization took 1ms\nSwitching state from LoadEcsScene to Run\n").unwrap();
        verify_owned(&root, &running).unwrap();
        for (run, cycle, bytes) in [
            (3, "after_restore", b"later C"),
            (4, "after_program_maintenance", b"later D"),
            (5, "after_archive_restore", b"later E"),
        ] {
            running.active_run.as_mut().unwrap().run_id = run;
            fs::write(&world, bytes).unwrap();
            capture_owned(&root, &running, cycle).await.unwrap();
            assert_eq!(fs::read(&world).unwrap(), bytes);
            assert_eq!(
                fs::read(root.join("manual-checkpoint/world0.ksd")).unwrap(),
                b"manual A"
            );
        }
        assert!(
            capture_owned(&root, &running, "initial")
                .await
                .unwrap_err()
                .contains("already captured")
        );
        let checkpoint = checkpoints().unwrap().remove(&running.summary.id).unwrap();
        assert!(checkpoint.restored && checkpoint.read_back);
        assert_eq!(checkpoint.run_id, 1);
        assert_eq!(
            files(
                &checkpoint.copy,
                &profiles::roots("enshrouded", &checkpoint.copy).unwrap()
            )
            .unwrap(),
            checkpoint.manifest
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn native_checkpoint_excludes_shutdown_overwrite_and_new_generation() {
        let mut checkpoint = fixture();
        fs::write(checkpoint.saves.join("world0.ksd"), b"shutdown B").unwrap();
        fs::write(checkpoint.saves.join("world1.ksd"), b"new generation C").unwrap();
        restore_snapshot(&mut checkpoint, "enshrouded").unwrap();
        assert!(checkpoint.restored);
        assert_eq!(
            fs::read(checkpoint.saves.join("world0.ksd")).unwrap(),
            b"manual A"
        );
        assert!(!checkpoint.saves.join("world1.ksd").exists());
        let preserved = checkpoint.fixture.join("shutdown-checkpoint");
        assert_eq!(
            fs::read(preserved.join("world0.ksd")).unwrap(),
            b"shutdown B"
        );
        assert_eq!(
            fs::read(preserved.join("world1.ksd")).unwrap(),
            b"new generation C"
        );
        assert_eq!(
            files(
                &checkpoint.saves,
                &profiles::roots("enshrouded", &checkpoint.saves).unwrap()
            )
            .unwrap(),
            checkpoint.manifest
        );
        fs::remove_dir_all(checkpoint.fixture).unwrap();
    }

    #[test]
    fn native_checkpoint_rejects_changed_snapshot_before_displacing_shutdown_data() {
        let mut checkpoint = fixture();
        fs::write(
            checkpoint.copy.join("world0.ksd"),
            b"changed manual snapshot",
        )
        .unwrap();
        fs::write(checkpoint.saves.join("world0.ksd"), b"shutdown B").unwrap();
        assert!(restore_snapshot(&mut checkpoint, "enshrouded").is_err());
        assert!(!checkpoint.restored);
        assert!(!checkpoint.fixture.join("shutdown-checkpoint").exists());
        assert_eq!(
            fs::read(checkpoint.saves.join("world0.ksd")).unwrap(),
            b"shutdown B"
        );
        assert!(
            log_paths(
                &checkpoint.fixture,
                vec![checkpoint.fixture.parent().unwrap().join("external.log")]
            )
            .is_err()
        );
        fs::remove_dir_all(checkpoint.fixture).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn native_checkpoint_rejects_redirected_snapshot_before_displacing_shutdown_data() {
        use std::os::windows::process::CommandExt;
        let mut checkpoint = fixture();
        let retained = checkpoint.fixture.join("retained-snapshot");
        fs::rename(&checkpoint.copy, &retained).unwrap();
        let result = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&checkpoint.copy)
            .arg(&retained)
            .creation_flags(0x0800_0000)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "create owned checkpoint junction: {result:?}"
        );
        assert!(restore_snapshot(&mut checkpoint, "enshrouded").is_err());
        assert!(!checkpoint.fixture.join("shutdown-checkpoint").exists());
        assert_eq!(
            fs::read(checkpoint.saves.join("world0.ksd")).unwrap(),
            b"manual A"
        );
        fs::remove_dir(&checkpoint.copy).unwrap();
        assert_eq!(fs::read(retained.join("world0.ksd")).unwrap(), b"manual A");
        fs::remove_dir_all(checkpoint.fixture).unwrap();
    }
}
