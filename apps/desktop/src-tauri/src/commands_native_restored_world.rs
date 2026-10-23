//! Test-only native content readback after backup/archive/program maintenance.
//! A ready server is insufficient: the restored bytes and its last saved native
//! identity must survive startup and be loaded by a new owned process generation.
use super::super::{
    RunGeneration, generation, generation_from_run, primary_process, require_current,
};
use super::*;
use app_core::{ActiveInstanceRun, InstanceRunRecord, InstanceStatus};
use std::collections::BTreeSet;

type Key = (String, String);
static PREPARED: OnceLock<Mutex<BTreeMap<Key, Prepared>>> = OnceLock::new();

struct Prepared {
    fixture: PathBuf,
    saves: PathBuf,
    program: PathBuf,
    config: PathBuf,
    config_snapshot: serde_json::Value,
    id: String,
    module: String,
    settings: String,
    manifest: Manifest,
    previous: RunGeneration,
    console_parent: PathBuf,
    existing_logs: BTreeSet<PathBuf>,
    native_before: String,
    logs: Vec<(PathBuf, log_cursor::LogBaseline)>,
    verified: bool,
}

fn applies(module: &str, cycle: &str) -> Result<bool, String> {
    if !matches!(
        cycle,
        "after_restore" | "after_archive_restore" | "after_program_maintenance"
    ) || matches!(module, "returntomoria" | "terraria")
    {
        return Ok(false);
    }
    match module {
        "valheim" | "runescapedragonwilds" | "vrising" | "necesse" => Ok(true),
        _ => Err(format!(
            "{module}: restored-world acceptance has no verified native saved-content identity; readiness alone is insufficient"
        )),
    }
}

fn prepared() -> Result<std::sync::MutexGuard<'static, BTreeMap<Key, Prepared>>, String> {
    PREPARED
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .map_err(|_| "Restored-world ownership lock failed".into())
}

/// Called before production startup, after the existing harness has verified
/// that backup/archive restoration or maintenance retained the stopped bytes.
pub(in super::super) async fn before_start<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    details: &InstanceDetails,
    cycle: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if !enabled()? || !applies(&details.summary.module_id, cycle)? {
        return Ok(());
    }
    let (fixture, expected) = input(runtime, details)?;
    let (current, overview) = app_storage::read_instance_details_and_runtime_overview(
        &runtime.storage.paths,
        &details.summary.id,
    )
    .await?;
    same_stopped_instance(&expected, &current)?;
    let prior = overview
        .recent_runs
        .into_iter()
        .next()
        .ok_or("Restored world has no retained previous run identity")?;
    let cycle = cycle.to_owned();
    worker(move || {
        let snapshot = Prepared::capture(&fixture, &current, &prior)?;
        {
            let checkpoints = checkpoints()?;
            let checkpoint = checkpoints.get(&current.summary.id)
                .ok_or("Restored-world readback has no verified manual checkpoint")?;
            if !checkpoint.restored || !checkpoint.read_back
                || checkpoint.fixture != snapshot.fixture || checkpoint.saves != snapshot.saves
                || checkpoint.run_id == snapshot.previous.run_id
            {
                return Err("Restored-world readback is outside its verified fixture history".into());
            }
        }
        let mut records = prepared()?;
        let key = (current.summary.id.clone(), cycle.clone());
        if records.len() >= 128 || records.contains_key(&key) {
            return Err("Restored-world capture is duplicated or exceeds its fixture bound".into());
        }
        println!(
            "NATIVE_RESTORED_WORLD module={} cycle={cycle} phase=captured prior_run_id={} files={} stopped=true",
            snapshot.module, snapshot.previous.run_id, snapshot.manifest.len()
        );
        records.insert(key, snapshot);
        Ok(())
    }).await?;
    let latest = read_instance_details(&runtime.storage.paths, &details.summary.id).await?;
    same_stopped_instance(details, &latest)?;
    Ok(())
}

/// Must run after native readiness and before any later formal/manual save.
pub(in super::super) async fn verify_loaded<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    running: &InstanceDetails,
    cycle: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if !enabled()? || !applies(&running.summary.module_id, cycle)? {
        return Ok(());
    }
    let expected = generation(running)?;
    require_current(runtime, running, &expected).await?;
    let (fixture, owned) = input(runtime, running)?;
    let cycle = cycle.to_owned();
    let key = (owned.summary.id.clone(), cycle.clone());
    let owned_key = key.clone();
    worker(move || {
        let mut records = prepared()?;
        let snapshot = records
            .get_mut(&owned_key)
            .ok_or("Restored-world readback has no stopped capture")?;
        snapshot.verify(&fixture, &owned)
    })
    .await?;
    require_current(runtime, running, &expected).await?;
    let records = prepared()?;
    let snapshot = records
        .get(&key)
        .ok_or("Restored-world verification record disappeared")?;
    println!(
        "NATIVE_RESTORED_WORLD module={} cycle={cycle} phase=loaded prior_run_id={} run_id={} restored_bytes=unchanged native_saved_identity=matched fresh_logs=true second_save_sent=false",
        snapshot.module, snapshot.previous.run_id, expected.run_id
    );
    Ok(())
}

fn same_stopped_instance(
    expected: &InstanceDetails,
    current: &InstanceDetails,
) -> Result<(), String> {
    if !matches!(current.summary.status, InstanceStatus::Stopped)
        || current.active_run.is_some()
        || current.summary.id != expected.summary.id
        || current.summary.module_id != expected.summary.module_id
        || current.saves_path != expected.saves_path
        || current.config_file_path != expected.config_file_path
        || current.settings_json != expected.settings_json
    {
        return Err("Restored-world capture requires the unchanged stopped fixture".into());
    }
    Ok(())
}

fn previous_run(record: &InstanceRunRecord) -> Result<(RunGeneration, PathBuf), String> {
    if record.run_id <= 0
        || record.status != "stopped"
        || record.stopped_at.is_none()
        || record.crash_flag
        || record.exit_code != Some(0)
        || record.process_count != record.processes.len()
        || record.session_id.as_deref().is_none_or(str::is_empty)
        || record.processes.iter().any(|process| {
            process.status != "stopped"
                || process.stopped_at.is_none()
                || process.crash_flag
                || process.exit_code != Some(0)
        })
    {
        return Err("Restored world requires a retained normally stopped process tree".into());
    }
    // Reuse the authoritative run/process/session consistency validation. Only
    // the view's status changes; the retained identities and paths stay intact.
    let mut run = ActiveInstanceRun {
        run_id: record.run_id,
        session_id: record.session_id.clone(),
        pid: record.pid,
        log_path: record.log_path.clone(),
        process_count: record.process_count,
        processes: record.processes.clone(),
    };
    for process in &mut run.processes {
        process.status = "running".into();
    }
    let generation = generation_from_run(&run)?;
    let console = record
        .processes
        .iter()
        .find(|process| process.is_primary)
        .and_then(|process| process.log_path.as_deref())
        .ok_or("Previous restored-world run has no primary console log")?;
    if record.log_path.as_deref() != Some(console) {
        return Err("Previous primary log differs from its retained run".into());
    }
    Ok((generation, PathBuf::from(console)))
}

fn require_owned_image(program: &Path, image: &str) -> Result<(), String> {
    let image = Path::new(image);
    let _pins = guard_pin_directories(
        image
            .parent()
            .ok_or("Restored-world process image has no parent")?,
    )?;
    if !ordinary(image)?.is_file() {
        return Err("Restored-world process image must be an ordinary file".into());
    }
    // Runtime identities normalize Windows image paths to lowercase. Resolve
    // the existing ordinary file just as binding resolves the program root;
    // comparing the raw identity spelling would reject the same owned image.
    let image =
        dunce::canonicalize(image).map_err(|_| "Restored-world process image is unavailable")?;
    relative(program, &image)?;
    Ok(())
}

fn configuration_snapshot(path: &Path) -> Result<serde_json::Value, String> {
    let _pins = log_parent(path)?;
    let mut value: serde_json::Value =
        serde_json::from_slice(&guard_read_regular(path, 1024 * 1024)?)
            .map_err(|_| "Restored instance configuration is not valid JSON")?;
    let object = value
        .as_object_mut()
        .ok_or("Restored instance configuration is not an object")?;
    // templates::render_instance_config refreshes only this document metadata
    // on every materialization. Keep every other field, including unknown ones.
    let generated = object
        .remove("generated_at_unix_ms")
        .ok_or("Restored instance configuration has no generated timestamp")?;
    if generated.as_u64().is_none() {
        return Err("Restored instance configuration has an invalid generated timestamp".into());
    }
    Ok(value)
}

fn persistence_difference(expected: &Manifest, actual: &Manifest) -> String {
    let names: BTreeSet<_> = expected.keys().chain(actual.keys()).collect();
    let changed: Vec<_> = names
        .into_iter()
        .filter(|name| expected.get(*name) != actual.get(*name))
        .collect();
    let details = changed
        .iter()
        .take(8)
        .map(|name| {
            let kind = match (expected.get(*name), actual.get(*name)) {
                (None, _) => "added",
                (_, None) => "missing",
                _ => "changed",
            };
            let path: String = name.to_string_lossy().escape_debug().take(200).collect();
            format!("{kind}:{path}")
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "Restored persistence changed during startup; changed_files={} first_differences=[{details}]; this profile cannot certify content readback",
        changed.len()
    )
}

impl Prepared {
    fn capture(
        fixture: &Path,
        stopped: &InstanceDetails,
        record: &InstanceRunRecord,
    ) -> Result<Self, String> {
        same_stopped_instance(stopped, stopped)?;
        let (fixture, saves, program) = binding(fixture, stopped)?;
        let (previous, console) = previous_run(record)?;
        for process in &previous.processes {
            require_owned_image(&program, &process.identity.image_path)?;
            if app_runtime::process_matches_identity(process.pid, &process.identity)
                .map_err(|error| error.to_string())?
            {
                return Err("Previous restored-world process is still alive".into());
            }
        }
        let console_parent = console
            .parent()
            .ok_or("Previous native console has no parent")?
            .to_path_buf();
        log_paths(&fixture, vec![console.clone()])?;
        let _console_pins = guard_pin_directories(&console_parent)?;
        let mut existing_logs = BTreeSet::new();
        for entry in fs::read_dir(&console_parent)
            .map_err(|_| "Previous native log directory unavailable")?
        {
            let path = entry
                .map_err(|_| "Previous native log enumeration failed")?
                .path();
            if existing_logs.len() >= MAX_FILES {
                return Err("Previous native log directory exceeds its entry bound".into());
            }
            ordinary(&path)?;
            existing_logs.insert(path);
        }
        if !existing_logs.contains(&console) {
            return Err("Previous native primary log is no longer retained".into());
        }
        let roots = profiles::roots(&stopped.summary.module_id, &saves)?;
        let manifest = files(&saves, &roots)?;
        let config = dunce::canonicalize(&stopped.config_file_path)
            .map_err(|_| "Restored config unavailable")?;
        let config_snapshot = configuration_snapshot(&config)?;
        let fixed_logs = log_paths(
            &fixture,
            profiles::native_logs(&stopped.summary.module_id, &program, stopped),
        )?;
        let mut prior_logs = fixed_logs.clone();
        prior_logs.push(console);
        prior_logs.sort();
        prior_logs.dedup();
        let native_before = native_text(&prior_logs)?;
        if stopped.summary.module_id == "vrising" {
            let saved = native_before
                .lines()
                .rev()
                .find_map(profiles::vrising_completed_path)
                .ok_or("V Rising restored capture has no completed native save path")?;
            let saved =
                dunce::canonicalize(saved).map_err(|_| "V Rising completed save is unavailable")?;
            if manifest.get(&relative(&saves, &saved)?) != Some(&digest(&saved)?) {
                return Err(
                    "V Rising completed save is outside the captured persistence family".into(),
                );
            }
        }
        let logs = fixed_logs
            .into_iter()
            .map(|path| {
                let _pins = log_parent(&path)?;
                log_cursor::LogBaseline::capture(&path).map(|baseline| (path, baseline))
            })
            .collect::<Result<_, _>>()?;
        if native_before.trim().is_empty()
            || files(&saves, &roots)? != manifest
            || configuration_snapshot(&config)? != config_snapshot
        {
            return Err(
                "Restored-world capture changed or has no prior native save evidence".into(),
            );
        }
        Ok(Self {
            fixture,
            saves,
            program,
            config,
            config_snapshot,
            id: stopped.summary.id.clone(),
            module: stopped.summary.module_id.clone(),
            settings: stopped.settings_json.clone(),
            manifest,
            previous,
            console_parent,
            existing_logs,
            native_before,
            logs,
            verified: false,
        })
    }

    fn verify(&mut self, fixture: &Path, running: &InstanceDetails) -> Result<(), String> {
        let current = generation(running)?;
        let (fixture, saves, program) = binding(fixture, running)?;
        if self.verified
            || self.fixture != fixture
            || self.saves != saves
            || self.program != program
            || self.id != running.summary.id
            || self.module != running.summary.module_id
            || self.settings != running.settings_json
            || self.config != Path::new(&running.config_file_path)
            || self.previous.run_id == current.run_id
            || self.previous.session == current.session
            || current.session.as_deref().is_none_or(str::is_empty)
            || current.processes.iter().any(|process| {
                self.previous.processes.iter().any(|previous| {
                    app_runtime::process_identities_match(&previous.identity, &process.identity)
                })
            })
        {
            return Err(
                "Restored-world readback requires the captured fixture and a new owned run".into(),
            );
        }
        self.unchanged()?;
        for process in &current.processes {
            require_owned_image(&program, &process.identity.image_path)?;
        }
        let console = PathBuf::from(
            primary_process(running)?
                .log_path
                .as_deref()
                .ok_or("Restored-world run has no primary console log")?,
        );
        if console.parent() != Some(self.console_parent.as_path())
            || self.existing_logs.contains(&console)
        {
            return Err("Restored-world run reused a prior log or changed its log binding".into());
        }
        if running
            .active_run
            .as_ref()
            .and_then(|run| run.log_path.as_deref())
            != console.to_str()
        {
            return Err("Restored-world primary log differs from its active run".into());
        }
        let sources = log_paths(
            &fixture,
            profiles::native_logs(&self.module, &program, running),
        )?;
        for path in sources {
            if !self.logs.iter().any(|(known, _)| known == &path) {
                if path != console {
                    return Err("Restored-world evidence source has no pre-start baseline".into());
                }
                self.logs.push((path, log_cursor::LogBaseline::default()));
            }
        }
        let mut fresh = String::new();
        for (path, baseline) in &mut self.logs {
            log_paths(&fixture, vec![path.clone()])?;
            let _pins = log_parent(path)?;
            log_cursor::new_complete_log_line_matches(path, baseline, |line| {
                fresh.push_str(line);
                fresh.push('\n');
                false
            })?;
        }
        profiles::require_loaded(&self.module, &self.native_before, &fresh)?;
        self.unchanged()?;
        self.verified = true;
        Ok(())
    }

    fn unchanged(&self) -> Result<(), String> {
        let current = files(&self.saves, &profiles::roots(&self.module, &self.saves)?)?;
        if current != self.manifest {
            return Err(persistence_difference(&self.manifest, &current));
        }
        if configuration_snapshot(&self.config)? != self.config_snapshot {
            return Err("Restored configuration content changed during startup (excluding its validated generated timestamp)".into());
        }
        Ok(())
    }
}

#[path = "commands_native_restored_world_tests.rs"]
mod tests;
