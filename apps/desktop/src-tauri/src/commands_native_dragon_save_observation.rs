//! Independent disposable-world evidence for a Requested Dragonwilds save.
//! This observation never changes the production receipt to Confirmed.
use super::*;
use std::time::{Duration, Instant};

pub(in super::super) struct Observation {
    fixture: PathBuf,
    saves: PathBuf,
    log: PathBuf,
    baseline: log_cursor::LogBaseline,
    before: Manifest,
    world: String,
    deadline: Instant,
}

fn field<'a>(line: &'a str, prefix: &str) -> Option<&'a str> {
    line.split_once(prefix)?
        .1
        .split_once(']')
        .map(|(value, _)| value)
}

fn world_guid(value: &str) -> Option<String> {
    (value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| value.to_ascii_lowercase())
}

fn initial_world(text: &str) -> Result<String, String> {
    let intervals = text
        .lines()
        .filter_map(|line| {
            line.split_once("Set CVar [[dom.StateSaveFrequencyMins:")
                .map(|(_, value)| value.trim())
        })
        .collect::<Vec<_>>();
    if intervals.is_empty() || intervals.iter().any(|value| *value != "5]]") {
        return Err("Dragonwilds must report its native five-minute save interval".into());
    }
    let worlds = text
        .lines()
        .filter(|line| line.contains("Setting [\"WorldSaveGuid\"]"))
        .filter_map(|line| field(line, "value[").and_then(world_guid))
        .collect::<std::collections::BTreeSet<_>>();
    if worlds.len() != 1 {
        return Err("Dragonwilds has no unique current world GUID before the request".into());
    }
    Ok(worlds.into_iter().next().unwrap())
}

fn completed(text: &str, world: &str) -> Result<bool, String> {
    let mut slot = None;
    let mut stage = 0;
    for line in text.lines() {
        if line.contains("LogPersistence:") && line.contains("SaveGameInternal() : Starting save") {
            if stage != 0 || field(line, "Guid[").and_then(world_guid).as_deref() != Some(world) {
                return Err("Dragonwilds emitted an overlapping or different-world save".into());
            }
            slot = field(line, "SlotName[").filter(|value| !value.is_empty());
            if slot.is_none() {
                return Err("Dragonwilds save has no native slot".into());
            }
            stage = 1;
        } else if line.contains("LogSpudSubsystem: Save to slot ") {
            let value = line
                .split_once("LogSpudSubsystem: Save to slot ")
                .unwrap()
                .1;
            if stage != 1 || value.strip_suffix(": Success") != slot {
                return Err("Dragonwilds native slot did not report matching success".into());
            }
            stage = 2;
        } else if line.contains("LogPersistence:")
            && line.contains("Save completed SUCCESSFULLY (slot: ")
        {
            let value = line
                .split_once("Save completed SUCCESSFULLY (slot: ")
                .unwrap()
                .1;
            if stage != 2 || value.strip_suffix(')') != slot {
                return Err("Dragonwilds completion did not follow the matching slot write".into());
            }
            stage = 3;
        } else if line.contains("LogPersistence:") && line.contains("Save FAILED") {
            return Err("Dragonwilds reported a native save failure".into());
        }
    }
    Ok(stage == 3)
}

fn inventory(saves: &Path) -> Result<Manifest, String> {
    if fs::read_dir(saves)
        .map_err(|_| "Cannot read Dragonwilds save directory")?
        .next()
        .is_none()
    {
        return Ok(Manifest::new());
    }
    super::inventory(saves, &profiles::roots("runescapedragonwilds", saves)?)
}

fn observation_window(creation_time: u64, now_ms: u128) -> Result<Duration, String> {
    let created_ms = creation_time
        .checked_sub(116_444_736_000_000_000)
        .ok_or("Dragonwilds checkpoint requires a Windows process start token")?
        / 10_000;
    let age = now_ms
        .checked_sub(u128::from(created_ms))
        .ok_or("Dragonwilds process start is in the future")?;
    if age >= 240_000 {
        return Err(
            "Dragonwilds manual checkpoint must precede the first periodic autosave".into(),
        );
    }
    Ok(Duration::from_secs(30))
}

pub(in super::super) async fn prepare<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    running: &InstanceDetails,
    cycle: &str,
) -> Result<Option<Observation>, Box<dyn std::error::Error>> {
    if !enabled()? || running.summary.module_id != "runescapedragonwilds" || cycle != "initial" {
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
            .ok_or("Dragonwilds checkpoint has no current primary process")?;
        let identity = primary
            .process_identity
            .as_ref()
            .ok_or("Dragonwilds process identity is missing")?;
        let native = crate::live_players::file_ipc::unreal_save_process::capture(
            crate::live_players::file_ipc::unreal_save_process::GameProfile::Dragonwilds,
            &program,
            primary.pid.ok_or("Dragonwilds process PID is missing")?,
            identity,
        )?;
        let window = observation_window(
            native.identity.creation_time,
            super::super::unix_ms().map_err(|e| e.to_string())?,
        )?;
        let deadline = Instant::now() + window;
        let log = PathBuf::from(
            primary
                .log_path
                .as_ref()
                .ok_or("Dragonwilds primary log is missing")?,
        );
        log_paths(&fixture, vec![log.clone()])?;
        let _pins = log_parent(&log)?;
        let text = native_text(std::slice::from_ref(&log))?;
        let world = initial_world(&text)?;
        let before = inventory(&saves)?;
        let baseline = log_cursor::LogBaseline::capture(&log)?;
        Ok(Some(Observation {
            fixture,
            saves,
            log,
            baseline,
            before,
            world,
            deadline,
        }))
    })
    .await
    .map_err(|e| format!("Dragonwilds observation preparation failed: {e}"))?
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
                return Err("Dragonwilds native save evidence did not arrive before the autosave exclusion deadline".into());
            }
            log_paths(&self.fixture, vec![self.log.clone()])?;
            let _pins = log_parent(&self.log)?;
            let mut fresh = String::new();
            log_cursor::new_complete_log_line_matches(&self.log, &mut self.baseline, |line| {
                fresh.push_str(line);
                fresh.push('\n');
                false
            })?;
            if completed(&fresh, &self.world)? {
                let after = inventory(&self.saves)?;
                if after.values().any(|(bytes, _)| *bytes > 0) && after != self.before {
                    println!(
                        "NATIVE_SAVE_OBSERVATION module=runescapedragonwilds receipt=requested fresh_native_save=true save_family_changed=true periodic_autosave_excluded=true production_completion=not_claimed"
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
    const GUID: &str = "244cff6e493547f66c9223bdb46d65a2";
    const START: &str = "LogPersistence: [DedicatedServer] SaveGameInternal() : Starting save (Guid[244CFF6E493547F66C9223BDB46D65A2] WorldName[Ashenfall] SlotName[Ashenfall] MapName[L_World])";
    const WRITE: &str = "LogSpudSubsystem: Save to slot Ashenfall: Success";
    const END: &str = "LogPersistence: [DedicatedServer] operator ()() : Save completed SUCCESSFULLY (slot: Ashenfall)";

    #[test]
    fn requested_save_observation_requires_ordered_matching_native_write() {
        let valid = format!("{START}\n{WRITE}\n{END}");
        assert_eq!(completed(&valid, GUID), Ok(true));
        for text in ["".into(), START.into(), format!("{START}\n{WRITE}")] {
            assert_eq!(completed(&text, GUID), Ok(false));
        }
        for text in [
            format!("{END}\n{WRITE}\n{START}"),
            valid.replace("slot Ashenfall:", "slot Foreign:"),
            format!("{valid}\n{START}"),
            format!("{START}\nLogPersistence: Save FAILED (slot: Ashenfall)"),
        ] {
            assert!(completed(&text, GUID).is_err());
        }
        assert!(completed(&valid, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").is_err());
    }

    #[test]
    fn requested_save_observation_requires_reported_interval_and_young_process() {
        let native = format!(
            "LogConfig: Set CVar [[dom.StateSaveFrequencyMins:5]]\nLogNetSessionSettings: Setting [\"WorldSaveGuid\"] written with key[xg] value[{GUID}]"
        );
        assert_eq!(initial_world(&native).unwrap(), GUID);
        assert!(initial_world(&native.replace("Mins:5", "Mins:1")).is_err());
        assert!(initial_world(&format!("{native}\nLogNetSessionSettings: Setting [\"WorldSaveGuid\"] value[aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa]")).is_err());
        let created = 116_444_736_000_000_000;
        assert!(observation_window(created, 239_999).is_ok());
        assert!(observation_window(created, 240_000).is_err());
        assert!(observation_window(0, 0).is_err());
    }

    #[test]
    fn requested_save_observation_excludes_pre_request_save_logs() {
        let root = crate::commands::tests::temp_test_dir("dragon-save-observation");
        let path = root.join("native.log");
        let saved = format!("{START}\n{WRITE}\n{END}\n");
        fs::write(&path, &saved).unwrap();
        let mut baseline = log_cursor::LogBaseline::capture(&path).unwrap();
        let read = |baseline: &mut log_cursor::LogBaseline| {
            let mut fresh = String::new();
            log_cursor::new_complete_log_line_matches(&path, baseline, |line| {
                fresh.push_str(line);
                fresh.push('\n');
                false
            })
            .unwrap();
            completed(&fresh, GUID)
        };
        assert_eq!(read(&mut baseline), Ok(false));
        let split = saved.find("SlotName[").unwrap() + "SlotName[".len();
        for (chunk, complete) in [
            (&saved[..split], false),
            (&saved[split..saved.len() - 1], false),
            (&saved[saved.len() - 1..], true),
        ] {
            fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .unwrap()
                .write_all(chunk.as_bytes())
                .unwrap();
            assert_eq!(read(&mut baseline), Ok(complete));
        }
        fs::remove_dir_all(root).unwrap();
    }
}
