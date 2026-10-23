//! Native console completion and world-seed readback for disposable Necesse saves.
//! The product receipt remains Requested; no runtime plugin is installed.
use super::*;
use std::time::{Duration, Instant};

// Dedicated build 24926481: SaveServerCommand registers a completion callback.
// Server.tickSaveHandler reloads the ZIP filesystem before invoking it. The
// first periodic save is due after 60 seconds; keep this witness before it.
const SERVER_SHA256: &str = "83e08ad78ceb5f6f24b5ed9bee96ce63160e4f72a7bff8dafd8b99cb649952ef";
const AUTOSAVE_MS: u128 = 60_000;
const SAVE_SECONDS: u64 = 30;

fn before_autosave(
    pid: u32,
    identity: &ProcessIdentity,
    remaining: Duration,
) -> Result<(), String> {
    if !app_runtime::process_matches_identity(pid, identity).map_err(|error| error.to_string())? {
        return Err("Necesse checkpoint process exited or changed generation".into());
    }
    let created = identity
        .creation_time
        .checked_sub(116_444_736_000_000_000)
        .ok_or("Necesse checkpoint requires a Windows process identity")?
        / 10_000;
    let age = super::super::unix_ms()
        .map_err(|error| error.to_string())?
        .checked_sub(u128::from(created))
        .ok_or("Necesse process start is in the future")?;
    if age.saturating_add(remaining.as_millis()) >= AUTOSAVE_MS {
        return Err("Necesse checkpoint overlaps its first native periodic save".into());
    }
    Ok(())
}

#[derive(Clone)]
struct Target {
    fixture: PathBuf,
    saves: PathBuf,
    log: PathBuf,
    pid: u32,
    identity: ProcessIdentity,
}

impl Target {
    fn before_autosave(&self, remaining: Duration) -> Result<(), String> {
        before_autosave(self.pid, &self.identity, remaining)
    }

    fn baseline(&self) -> Result<log_cursor::LogBaseline, String> {
        log_paths(&self.fixture, vec![self.log.clone()])?;
        let _pins = log_parent(&self.log)?;
        log_cursor::LogBaseline::capture(&self.log)
    }

    fn fresh(&self, baseline: &mut log_cursor::LogBaseline) -> Result<String, String> {
        log_paths(&self.fixture, vec![self.log.clone()])?;
        let _pins = log_parent(&self.log)?;
        let mut fresh = String::new();
        log_cursor::new_complete_log_line_matches(&self.log, baseline, |line| {
            fresh.push_str(line);
            fresh.push('\n');
            false
        })?;
        Ok(fresh)
    }

    fn inventory(&self) -> Result<Manifest, String> {
        let _pins = guard_pin_directories(&self.saves)?;
        inventory(&self.saves, &profiles::roots("necesse", &self.saves)?)
    }
}

async fn target<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    running: &InstanceDetails,
) -> Result<Target, Box<dyn std::error::Error>> {
    let (fixture, running) = input(runtime, running)?;
    tokio::task::spawn_blocking(move || -> Result<Target, String> {
        let (fixture, saves, program) = binding(&fixture, &running)?;
        let _pins = guard_pin_directories(&program)?;
        let primary = super::super::primary_process(&running)?;
        let identity = primary
            .process_identity
            .clone()
            .ok_or("Necesse process identity is missing")?;
        if dunce::canonicalize(&identity.image_path)
            .map_err(|_| "Necesse Java image is unavailable")?
            != program.join("jre/bin/java.exe")
            || digest(&program.join("Server.jar"))?.1 != SERVER_SHA256
        {
            return Err(
                "Necesse checkpoint requires the verified native dedicated server build".into(),
            );
        }
        let log = PathBuf::from(
            primary
                .log_path
                .as_ref()
                .ok_or("Necesse primary log is missing")?,
        );
        log_paths(&fixture, vec![log.clone()])?;
        let target = Target {
            fixture,
            saves,
            log,
            pid: primary.pid.ok_or("Necesse primary PID is missing")?,
            identity,
        };
        target.before_autosave(Duration::ZERO)?;
        Ok(target)
    })
    .await
    .map_err(|error| format!("Necesse checkpoint preparation failed: {error}"))?
    .map_err(Into::into)
}

fn native_line(line: &str) -> &str {
    line.split_once("] ").map_or(line, |(_, body)| body).trim()
}

fn seeds(text: &str) -> Result<Vec<&str>, String> {
    text.lines()
        .filter_map(|line| native_line(line).strip_prefix("World seed: "))
        .map(|seed| {
            if seed.is_empty() || seed.len() > 128 || seed.chars().any(char::is_control) {
                Err("Necesse emitted an invalid native world seed".into())
            } else {
                Ok(seed)
            }
        })
        .collect()
}

async fn query<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    running: &InstanceDetails,
    target: &Target,
) -> Result<String, Box<dyn std::error::Error>> {
    let generation = super::super::generation(running)?;
    super::super::require_current(runtime, running, &generation).await?;
    let owned = target.clone();
    let mut baseline = tokio::task::spawn_blocking(move || owned.baseline()).await??;
    let primary = super::super::primary_process(running)?;
    let result = crate::commands::send_instance_gm_command(
        runtime.state.clone(),
        serde_json::from_value(json!({ "instanceId": running.summary.id,
            "command": "worldseed", "processKey": primary.process_key, "transport": "stdin" }))?,
    )
    .await?;
    if result.instance_id != running.summary.id
        || result.command != "worldseed"
        || Some(result.pid) != primary.pid
        || result.process_key != primary.process_key
        || result.write_confirmation_pending
    {
        return Err("Necesse world-seed query was not delivered to the owned process".into());
    }
    let owned = target.clone();
    let seed = tokio::task::spawn_blocking(move || -> Result<String, String> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            owned.before_autosave(Duration::ZERO)?;
            let fresh = owned.fresh(&mut baseline)?;
            match seeds(&fresh)?.as_slice() {
                [seed] => return Ok((*seed).to_owned()),
                [] if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(100)),
                [] => return Err("Necesse did not emit a fresh native world-seed response".into()),
                _ => return Err("Necesse emitted ambiguous world-seed responses".into()),
            }
        }
    })
    .await??;
    super::super::require_current(runtime, running, &generation).await?;
    Ok(seed)
}

#[derive(Default)]
struct SaveProgress(u8);

impl SaveProgress {
    fn observe(&mut self, text: &str) -> Result<bool, String> {
        for line in text.lines().map(native_line) {
            let next = match line {
                "> save" => Some((0, 1)),
                "Starting world save" => Some((1, 2)),
                "Starting save..." => Some((2, 3)),
                "Completed world save" => Some((3, 4)),
                "A save is already in progress"
                | "Save interrupted by a new save request"
                | "> stop"
                | "Completed world save before stopping server" => {
                    return Err("Necesse rejected, interrupted or replaced the manual save".into());
                }
                _ => None,
            };
            if let Some((expected, next)) = next {
                if self.0 != expected {
                    return Err("Necesse emitted an unattributed or overlapping save".into());
                }
                self.0 = next;
            }
        }
        Ok(self.0 == 4)
    }
}

pub(in super::super) struct Observation {
    target: Target,
    before: Manifest,
    baseline: log_cursor::LogBaseline,
    deadline: Instant,
}

pub(in super::super) async fn prepare<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    running: &InstanceDetails,
    cycle: &str,
) -> Result<Option<Observation>, Box<dyn std::error::Error>> {
    if !enabled()? || running.summary.module_id != "necesse" {
        return Ok(None);
    }
    let target = target(runtime, running).await?;
    if cycle == "initial" {
        query(runtime, running, &target).await?;
    }
    tokio::task::spawn_blocking(move || -> Result<Option<Observation>, String> {
        target.before_autosave(Duration::from_secs(SAVE_SECONDS))?;
        let before = target.inventory()?;
        let baseline = target.baseline()?;
        Ok(Some(Observation {
            target,
            before,
            baseline,
            deadline: Instant::now() + Duration::from_secs(SAVE_SECONDS),
        }))
    })
    .await?
    .map_err(Into::into)
}

impl Observation {
    pub(in super::super) async fn wait(self) -> Result<(), Box<dyn std::error::Error>> {
        worker(move || {
            let mut observation = self;
            let mut progress = SaveProgress::default();
            while Instant::now() < observation.deadline {
                observation.target.before_autosave(Duration::ZERO)?;
                if progress.observe(&observation.target.fresh(&mut observation.baseline)?)? {
                    let after = observation.target.inventory()?;
                    if after.is_empty() || after == observation.before {
                        return Err("Necesse completed without changing its native world family".into());
                    }
                    println!("NATIVE_SAVE_OBSERVATION module=necesse receipt=requested native_callback=completed save_family_changed=true periodic_autosave_excluded=true production_completion=not_claimed");
                    return Ok(());
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err("Necesse did not complete the requested native save within its bounded window".into())
        }).await
    }
}

pub(super) fn verify_capture(running: &InstanceDetails) -> Result<(), String> {
    if running.summary.module_id != "necesse" {
        return Ok(());
    }
    let primary = super::super::primary_process(running)?;
    let identity = primary
        .process_identity
        .clone()
        .ok_or("Necesse process identity is missing")?;
    before_autosave(
        primary.pid.ok_or("Necesse primary PID is missing")?,
        &identity,
        Duration::ZERO,
    )
}

pub(in super::super) async fn verify_loaded<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    running: &InstanceDetails,
    cycle: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if running.summary.module_id != "necesse" {
        return Ok(());
    }
    let target = target(runtime, running).await?;
    let (expected, initial_manifest) = {
        let owned = checkpoints()?;
        let checkpoint = owned
            .get(&running.summary.id)
            .ok_or("Necesse readback has no checkpoint")?;
        if !checkpoint.restored
            || checkpoint.read_back != (cycle != "restart")
            || checkpoint.run_id == super::super::generation(running)?.run_id
            || checkpoint.fixture != target.fixture
            || checkpoint.saves != target.saves
        {
            return Err("Necesse readback requires a new restored checkpoint run".into());
        }
        let seeds = seeds(&checkpoint.native_before)?;
        let expected = seeds
            .last()
            .ok_or("Necesse checkpoint has no native world seed")?
            .to_string();
        (expected, checkpoint.manifest.clone())
    };
    let owned = target.clone();
    let before = tokio::task::spawn_blocking(move || owned.inventory()).await??;
    if cycle == "restart" && before != initial_manifest {
        return Err("Necesse restarted without the exact manual world snapshot".into());
    }
    if query(runtime, running, &target).await? != expected {
        return Err("Necesse loaded a different world seed after restoration".into());
    }
    let after = tokio::task::spawn_blocking(move || target.inventory()).await??;
    if after != before {
        return Err("Necesse world changed during native content readback".into());
    }
    println!(
        "NATIVE_CONTENT_READBACK module=necesse cycle={cycle} loaded_world_seed=matched fresh_native_query=true second_save_sent=false"
    );
    Ok(())
}

pub(super) fn require_loaded(before: &str, fresh: &str) -> Result<(), String> {
    let previous = seeds(before)?;
    let current = seeds(fresh)?;
    if previous.last().is_none()
        || current.len() != 1
        || previous.last() != current.first()
        || !fresh.contains("Loading existing world at ")
        || fresh.contains("Creating new world at ")
        || fresh.contains("Could not find world file")
        || fresh
            .lines()
            .map(native_line)
            .any(|line| line == "> save" || line == "> stop")
    {
        return Err("Necesse did not load the captured native world before any new save".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn necesse_requested_save_waits_for_its_native_callback_and_same_world_reload() {
        let mut progress = SaveProgress::default();
        let started = "\u{1b}[39m[2026-10-23 15:53:51] > save\n[2026-10-23 15:53:51] Starting world save\n[2026-10-23 15:53:51] Starting save...\n";
        assert!(!progress.observe(started).unwrap());
        assert!(!progress.observe("").unwrap());
        assert!(
            progress
                .observe("[2026-10-23 15:53:53] Completed world save\n")
                .unwrap()
        );
        for rejected in [
            "A save is already in progress",
            "Save interrupted by a new save request",
            "> stop",
            "Completed world save before stopping server",
            "Starting world save",
        ] {
            let mut progress = SaveProgress::default();
            progress.observe(started).unwrap();
            assert!(progress.observe(rejected).is_err(), "{rejected}");
        }
        assert!(
            SaveProgress::default()
                .observe("Completed world save")
                .is_err()
        );
        let before = "[2026-10-23 15:53:51] World seed: ik4RN\n";
        let fresh = "[2026-10-23 15:54:01] Loading existing world at fixture/data/saves/worlds/world.zip\n[2026-10-23 15:54:01] > worldseed\n[2026-10-23 15:54:01] World seed: ik4RN\n";
        require_loaded(before, fresh).unwrap();
        assert!(require_loaded(before, &fresh.replace("ik4RN", "different")).is_err());
        assert!(
            require_loaded(
                before,
                &fresh.replace("Loading existing world", "Creating new world")
            )
            .is_err()
        );
        assert!(require_loaded(before, &format!("{fresh}> save\n")).is_err());
    }
}
