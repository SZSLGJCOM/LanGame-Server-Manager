//! Independent native evidence for Requested saves in disposable acceptance.
//! Product receipts remain Requested; shutdown and periodic writes cannot pay this debt.
use super::*;
use std::time::{Duration, Instant};

pub(in super::super) struct Observation {
    fixture: PathBuf,
    saves: PathBuf,
    log: PathBuf,
    baseline: log_cursor::LogBaseline,
    before: Manifest,
    creation_time: u64,
    deadline: Instant,
}

fn age_ms(creation_time: u64, now_ms: u128) -> Result<u128, String> {
    let created = creation_time
        .checked_sub(116_444_736_000_000_000)
        .ok_or("Requested-save observation requires a Windows process identity")?
        / 10_000;
    now_ms
        .checked_sub(u128::from(created))
        .ok_or_else(|| "Requested-save process start is in the future".into())
}

fn timing(line: &str, age: u128) -> Result<bool, String> {
    let Some(value) = line.split_once("LANGAME_NATIVE_SAVE_TIMING interval_seconds=") else {
        return Ok(false);
    };
    let (interval, variance) = value
        .1
        .split_once(" variance_seconds=")
        .ok_or("V Rising native timing diagnostic is incomplete")?;
    let interval = interval
        .parse::<f64>()
        .map_err(|_| "Invalid native autosave interval")?;
    let variance = variance
        .trim()
        .parse::<f64>()
        .map_err(|_| "Invalid native autosave variance")?;
    // The native scheduler adds a bounded random variance to its interval.
    // Process age is a conservative upper bound on time since that scheduler began.
    if !interval.is_finite()
        || !variance.is_finite()
        || interval < 3600.0
        || (interval - variance.abs()) * 1000.0 <= age as f64 + 30_000.0
    {
        return Err("V Rising request is not safely before its first periodic autosave".into());
    }
    Ok(true)
}

fn completed(text: &str, saves: &Path, age: u128) -> Result<Option<PathBuf>, String> {
    let mut observed_timing = false;
    let mut started = None;
    let mut finished = None;
    for line in text.lines() {
        observed_timing |= timing(line, age)?;
        if line.contains("SaveOnExit()") {
            return Err("V Rising shutdown save cannot prove manual persistence".into());
        }
        if let Some((_, value)) = line.split_once("Triggering AutoSave ") {
            let number = value
                .strip_suffix('!')
                .and_then(|v| v.parse::<u64>().ok())
                .ok_or("Invalid native autosave number")?;
            if !observed_timing || started.is_some_and(|previous| previous != number) {
                return Err("V Rising emitted an unattributed or overlapping native save".into());
            }
            // Unity may repeat the same line through both its logger and stdout.
            started = Some(number);
        }
        if line.contains("PersistenceV2 - Finished Saving to '") {
            let path = PathBuf::from(
                profiles::vrising_completed_path(line)
                    .ok_or("V Rising native completion has no nonempty persistence result")?,
            );
            let number = started.ok_or("V Rising completion preceded its native request")?;
            let name = format!("AutoSave_{number}.save.gz");
            if !path.is_absolute()
                || !path.starts_with(saves.join("v4"))
                || path.file_name().and_then(|name| name.to_str()) != Some(name.as_str())
                || relative(saves, &path).is_err()
                || finished.as_ref().is_some_and(|previous| previous != &path)
            {
                return Err("V Rising completion does not match its requested native save".into());
            }
            finished = Some(path);
        }
    }
    Ok(finished)
}

fn inventory(saves: &Path) -> Result<Manifest, String> {
    let roots = profiles::roots("vrising", saves)?;
    match fs::symlink_metadata(&roots[0]) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Manifest::new()),
        Err(_) => Err("V Rising persistence root is unreadable".into()),
        Ok(_) => super::inventory(saves, &roots),
    }
}

pub(in super::super) async fn prepare<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    running: &InstanceDetails,
    cycle: &str,
) -> Result<Option<Observation>, Box<dyn std::error::Error>> {
    if !enabled()? || running.summary.module_id != "vrising" || cycle != "initial" {
        return Ok(None);
    }
    let (fixture, running) = input(runtime, running)?;
    tokio::task::spawn_blocking(move || -> Result<Option<Observation>, String> {
        let (fixture, saves, program) = binding(&fixture, &running)?;
        let primary = running
            .active_run
            .as_ref()
            .and_then(|run| {
                run.processes
                    .iter()
                    .find(|process| process.is_primary && process.status == "running")
            })
            .ok_or("V Rising checkpoint has no running primary process")?;
        let identity = primary
            .process_identity
            .as_ref()
            .ok_or("V Rising process identity is missing")?;
        let pid = primary.pid.ok_or("V Rising process PID is missing")?;
        if dunce::canonicalize(&identity.image_path).map_err(|_| "V Rising image unavailable")?
            != program.join("VRisingServer.exe")
            || !app_runtime::process_matches_identity(pid, identity).map_err(|e| e.to_string())?
        {
            return Err("V Rising checkpoint process differs from its owned native image".into());
        }
        age_ms(
            identity.creation_time,
            super::super::unix_ms().map_err(|e| e.to_string())?,
        )?;
        let log = PathBuf::from(
            primary
                .log_path
                .as_ref()
                .ok_or("V Rising primary log is missing")?,
        );
        log_paths(&fixture, vec![log.clone()])?;
        let _pins = log_parent(&log)?;
        let before = inventory(&saves)?;
        let baseline = log_cursor::LogBaseline::capture(&log)?;
        Ok(Some(Observation {
            fixture,
            saves,
            log,
            baseline,
            before,
            creation_time: identity.creation_time,
            deadline: Instant::now() + Duration::from_secs(30),
        }))
    })
    .await
    .map_err(|e| format!("Requested-save observation preparation failed: {e}"))?
    .map_err(Into::into)
}

impl Observation {
    pub(in super::super) async fn wait(self) -> Result<(), Box<dyn std::error::Error>> {
        worker(move || self.wait_owned()).await
    }

    fn wait_owned(mut self) -> Result<(), String> {
        let _save_pins = guard_pin_directories(&self.saves)?;
        loop {
            if Instant::now() >= self.deadline {
                return Err("V Rising native save evidence did not arrive within the bounded request window".into());
            }
            log_paths(&self.fixture, vec![self.log.clone()])?;
            let _pins = log_parent(&self.log)?;
            let mut fresh = String::new();
            log_cursor::new_complete_log_line_matches(&self.log, &mut self.baseline, |line| {
                fresh.push_str(line);
                fresh.push('\n');
                false
            })?;
            let age = age_ms(
                self.creation_time,
                super::super::unix_ms().map_err(|e| e.to_string())?,
            )?;
            if let Some(path) = completed(&fresh, &self.saves, age)? {
                let after = inventory(&self.saves)?;
                let key = relative(&self.saves, &path)?;
                if after
                    .get(&key)
                    .is_some_and(|value| value.0 > 0 && Some(value) != self.before.get(&key))
                {
                    println!(
                        "NATIVE_SAVE_OBSERVATION module=vrising receipt=requested fresh_native_save=true save_family_changed=true periodic_autosave_excluded=true production_completion=not_claimed"
                    );
                    return Ok(());
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const TIMING: &str = "LANGAME_NATIVE_SAVE_TIMING interval_seconds=3600 variance_seconds=60";
    const START: &str = "Triggering AutoSave 1!";

    fn complete_line(saves: &Path) -> String {
        format!(
            "PersistenceV2 - Finished Saving to '{}'. Total Persistent ChunkCount 2678. Total Save Size: 7096465 Total Uncompressed Save Size: 33580424",
            saves.join("v4/world1/AutoSave_1.save.gz").display()
        )
    }

    #[test]
    fn vrising_requested_observation_requires_ordered_native_completion() {
        let saves = std::env::temp_dir().join("vrising-observation/Saves");
        let end = complete_line(&saves);
        let valid = format!("{TIMING}\n[Message: Unity] {START}\n{START}\n{end}");
        assert_eq!(
            completed(&valid, &saves, 180_000).unwrap(),
            Some(saves.join("v4/world1/AutoSave_1.save.gz"))
        );
        assert_eq!(
            completed(&format!("{TIMING}\n{START}"), &saves, 180_000).unwrap(),
            None
        );
        for invalid in [
            format!("{TIMING}\n{end}\n{START}"),
            format!("{START}\n{end}"),
            valid.replace("AutoSave_1", "AutoSave_2"),
            valid.replace("ChunkCount 2678", "ChunkCount 0"),
            format!("{valid}\nTriggering AutoSave 2!"),
            format!("{valid}\nSaveOnExit() - Saving the world on exit!"),
        ] {
            assert!(completed(&invalid, &saves, 180_000).is_err(), "{invalid}");
        }
    }

    #[test]
    fn vrising_requested_observation_requires_applied_interval_and_process_age() {
        assert_eq!(timing(TIMING, 180_000), Ok(true));
        for line in [
            TIMING.replace("3600", "120"),
            TIMING.replace("3600", "NaN"),
            TIMING.replace("60", "Infinity"),
            TIMING.replace("variance_seconds=60", "variance_seconds=3590"),
        ] {
            assert!(timing(&line, 180_000).is_err());
        }
        assert!(timing(TIMING, 3_520_000).is_err());
        assert!(age_ms(0, 0).is_err());
        assert_eq!(age_ms(116_444_736_000_000_000, 180_000), Ok(180_000));
    }

    #[test]
    fn vrising_requested_observation_excludes_pre_request_log_history() {
        let root = crate::commands::tests::temp_test_dir("vrising-observation");
        let log = root.join("native.log");
        let saves = root.join("Saves");
        let text = format!("{TIMING}\n{START}\n{}\n", complete_line(&saves));
        fs::write(&log, &text).unwrap();
        let mut baseline = log_cursor::LogBaseline::capture(&log).unwrap();
        let mut fresh = String::new();
        log_cursor::new_complete_log_line_matches(&log, &mut baseline, |line| {
            fresh.push_str(line);
            fresh.push('\n');
            false
        })
        .unwrap();
        assert_eq!(completed(&fresh, &saves, 180_000).unwrap(), None);
        // Timing, request and completion can each arrive across separate polls.
        let split = text.find("variance_seconds=").unwrap() + "variance_seconds=".len();
        let end_split = text.find("Total Persistent ChunkCount").unwrap();
        for (chunk, complete) in [
            (&text[..split], false),
            (&text[split..end_split], false),
            (&text[end_split..text.len() - 1], false),
            (&text[text.len() - 1..], true),
        ] {
            fs::OpenOptions::new()
                .append(true)
                .open(&log)
                .unwrap()
                .write_all(chunk.as_bytes())
                .unwrap();
            fresh.clear();
            log_cursor::new_complete_log_line_matches(&log, &mut baseline, |line| {
                fresh.push_str(line);
                fresh.push('\n');
                false
            })
            .unwrap();
            assert_eq!(
                completed(&fresh, &saves, 180_000).unwrap().is_some(),
                complete
            );
        }
        fs::remove_dir_all(root).unwrap();
    }
}
