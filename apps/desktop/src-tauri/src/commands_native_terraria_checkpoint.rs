//! Disposable vanilla acceptance: mutate the world clock, save once, then query
//! the restored world without mutating it. This does not upgrade product receipts.
use super::*;
use std::time::{Duration, Instant};

// Official 1.4.5.8: Main.DoUpdate_AutoSave waits 600000 ms; midnight writes
// dayTime=false/time=16200. WorldFile serializes both fields. Pin the executable
// because these timing/format facts are not a contract for arbitrary mods/builds.
const VANILLA_SHA256: &str = "d87e3faf08637f6be8882c63e7f11fb7e792b0230006309618473ece0f863e1e";
const AUTOSAVE_MS: u128 = 600_000;
const SAVE_SECONDS: u64 = 30;
const QUERY_SECONDS: u64 = 10;
const MAX_RESTART_SECONDS: u128 = 120;
const MAX_WORLD_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone)]
struct Target {
    fixture: PathBuf,
    saves: PathBuf,
    world: PathBuf,
    log: PathBuf,
    pid: u32,
    identity: ProcessIdentity,
}

#[derive(Debug, PartialEq, Eq)]
struct World {
    revision: u32,
    minute: u16,
    digest: String,
}

pub(in super::super) struct Observation {
    target: Target,
    before: World,
    baseline: log_cursor::LogBaseline,
    deadline: Instant,
    minute: u16,
}

#[derive(Clone, Copy)]
enum Command {
    Midnight,
    Time,
}

impl Command {
    fn text(self) -> &'static str {
        match self {
            Self::Midnight => "midnight",
            Self::Time => "time",
        }
    }
}

fn process_age_ms(creation_time: u64, now: u128) -> Result<u128, String> {
    let created = creation_time
        .checked_sub(116_444_736_000_000_000)
        .ok_or("Terraria checkpoint requires a Windows process identity")?
        / 10_000;
    now.checked_sub(u128::from(created))
        .ok_or_else(|| "Terraria process start is in the future".into())
}

fn before_autosave(age: u128, remaining: Duration) -> Result<(), String> {
    if age.saturating_add(remaining.as_millis()) >= AUTOSAVE_MS {
        return Err("Terraria checkpoint overlaps its first native periodic save".into());
    }
    Ok(())
}

impl Target {
    fn current_age(&self) -> Result<u128, String> {
        if !app_runtime::process_matches_identity(self.pid, &self.identity)
            .map_err(|error| error.to_string())?
        {
            return Err("Terraria checkpoint process exited or changed generation".into());
        }
        process_age_ms(
            self.identity.creation_time,
            super::super::unix_ms().map_err(|error| error.to_string())?,
        )
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

    fn world(&self) -> Result<Option<World>, String> {
        let _pins = log_parent(&self.world)?;
        let metadata = match fs::symlink_metadata(&self.world) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("Inspect Terraria primary world: {error}")),
        };
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err("Terraria primary world cannot be redirected".into());
            }
        }
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err("Terraria primary world is not an ordinary file".into());
        }
        let mut options = fs::OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            // A changed backup or a still-open native writer is insufficient.
            options.share_mode(0);
        }
        let file = match options.open(&self.world) {
            Ok(file) => file,
            // The native writer briefly owns/rotates the primary. All other
            // I/O and format errors retain their cause instead of timing out.
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound
                    || matches!(error.raw_os_error(), Some(32 | 33)) =>
            {
                return Ok(None);
            }
            Err(error) => return Err(format!("Read Terraria primary world: {error}")),
        };
        let mut bytes = Vec::new();
        file.take(MAX_WORLD_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        if bytes.len() as u64 > MAX_WORLD_BYTES {
            return Err("Terraria primary world exceeds the acceptance read bound".into());
        }
        world(&bytes).map(Some)
    }
}

fn world(bytes: &[u8]) -> Result<World, String> {
    // FileMetadata.Write emits relogic + file type 2 + uint32 revision after
    // the 1.4.5.8 world format version. Require a payload, not only this header.
    if bytes.len() < 168 || bytes[..4] != 326_i32.to_le_bytes() || &bytes[4..12] != b"relogic\x02" {
        return Err("Terraria primary world has an unsupported or incomplete header".into());
    }
    let minute = world_clock(bytes)?;
    Ok(World {
        revision: u32::from_le_bytes(bytes[12..16].try_into().map_err(|_| "Missing revision")?),
        minute,
        digest: Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    })
}

fn world_clock(bytes: &[u8]) -> Result<u16, String> {
    let invalid = || String::from("Terraria world clock header is incomplete or unsupported");
    let header = u32::from_le_bytes(
        bytes
            .get(26..30)
            .ok_or_else(invalid)?
            .try_into()
            .map_err(|_| invalid())?,
    ) as usize;
    let end = u32::from_le_bytes(
        bytes
            .get(30..34)
            .ok_or_else(invalid)?
            .try_into()
            .map_err(|_| invalid())?,
    ) as usize;
    if bytes.get(24..26) != Some(11_u16.to_le_bytes().as_slice())
        || header < 70
        || end > bytes.len()
        || header >= end
    {
        return Err(invalid());
    }
    let mut cursor = header;
    // SaveWorldHeader starts with BinaryWriter's two 7-bit length UTF-8 strings
    // (world name and seed), then 52 bytes of generator/identity/size fields.
    for _ in 0..2 {
        let mut length = 0_usize;
        for shift in [0, 7] {
            let byte = *bytes.get(cursor).ok_or_else(invalid)?;
            cursor += 1;
            length |= usize::from(byte & 127) << shift;
            if byte & 128 == 0 {
                break;
            }
            if shift == 7 {
                return Err(invalid());
            }
        }
        if length > 4096 {
            return Err(invalid());
        }
        cursor = cursor.checked_add(length).ok_or_else(invalid)?;
    }
    let flags = cursor.checked_add(52).ok_or_else(invalid)?;
    // Mode, nine special-world flags, dates/moon, tree/cave/background/spawn
    // integers, surface and rock doubles precede time (122 bytes in v326).
    let mode = u32::from_le_bytes(
        bytes
            .get(flags..flags + 4)
            .ok_or_else(invalid)?
            .try_into()
            .map_err(|_| invalid())?,
    );
    if mode > 2
        || bytes
            .get(flags + 4..flags + 13)
            .ok_or_else(invalid)?
            .iter()
            .any(|flag| *flag != 0)
    {
        return Err(invalid());
    }
    let at = flags + 122;
    if at + 9 > end {
        return Err(invalid());
    }
    let time = f64::from_le_bytes(
        bytes
            .get(at..at + 8)
            .ok_or_else(invalid)?
            .try_into()
            .map_err(|_| invalid())?,
    );
    let day = *bytes.get(at + 8).ok_or_else(invalid)?;
    if !time.is_finite()
        || day > 1
        || time < 0.0
        || time > if day == 1 { 54_000.0 } else { 32_400.0 }
    {
        return Err(invalid());
    }
    Ok((((time + if day == 1 { 16_200.0 } else { 70_200.0 }) / 60.0).floor() as u32 % 1440) as u16)
}

fn config_value<'a>(config: &'a str, key: &str) -> Result<&'a str, String> {
    let mut values = config.lines().filter_map(|line| {
        let (name, value) = line.trim().split_once('=')?;
        (name == key).then_some(value)
    });
    let value = values
        .next()
        .ok_or_else(|| format!("Terraria config omits {key}"))?;
    if values.next().is_some() {
        return Err(format!("Terraria config repeats {key}"));
    }
    Ok(value)
}

async fn target<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    running: &InstanceDetails,
) -> Result<Target, Box<dyn std::error::Error>> {
    let (fixture, running) = input(runtime, running)?;
    tokio::task::spawn_blocking(move || -> Result<Target, String> {
        let (fixture, saves, program) = binding(&fixture, &running)?;
        let settings: Value = serde_json::from_str(&running.settings_json)
            .map_err(|_| "Terraria fixture settings are invalid")?;
        if settings
            .get("managed_save_interval_seconds")
            .and_then(Value::as_u64)
            != Some(0)
        {
            return Err(
                "Terraria manual checkpoint requires managed periodic saves disabled".into(),
            );
        }
        let primary = super::super::primary_process(&running)?;
        let identity = primary
            .process_identity
            .clone()
            .ok_or("Terraria has no process identity")?;
        let image =
            dunce::canonicalize(&identity.image_path).map_err(|_| "Terraria image unavailable")?;
        if image != program.join("1458/Windows/TerrariaServer.exe")
            || digest(&image)?.1 != VANILLA_SHA256
        {
            return Err(
                "Terraria clock checkpoint requires the verified vanilla 1.4.5.8 executable".into(),
            );
        }
        // Launch rendering passes config/serverconfig.txt to -config; the
        // instance.json path identifies its parent, not the native text file.
        let config_path = Path::new(&running.config_file_path)
            .parent()
            .ok_or("Terraria config has no parent")?
            .join("serverconfig.txt");
        let _config_pins = log_parent(&config_path)?;
        let config = guard_read_regular(&config_path, 128 * 1024)?;
        let config = std::str::from_utf8(&config).map_err(|_| "Terraria config is not UTF-8")?;
        if config_value(config, "language")? != "en-US" {
            return Err("Terraria clock checkpoint requires native en-US console output".into());
        }
        let world = PathBuf::from(config_value(config, "world")?);
        relative(&saves, &world)?;
        if !world.is_absolute() || world.extension().and_then(|name| name.to_str()) != Some("wld") {
            return Err(
                "Terraria checkpoint must bind the configured primary .wld inside its saves".into(),
            );
        }
        let log = PathBuf::from(
            primary
                .log_path
                .as_ref()
                .ok_or("Terraria primary log is missing")?,
        );
        log_paths(&fixture, vec![log.clone()])?;
        let target = Target {
            fixture,
            saves,
            world,
            log,
            pid: primary.pid.ok_or("Terraria primary PID is missing")?,
            identity,
        };
        target.current_age()?;
        Ok(target)
    })
    .await
    .map_err(|error| format!("Terraria checkpoint preparation failed: {error}"))?
    .map_err(Into::into)
}

async fn dispatch<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    running: &InstanceDetails,
    command: Command,
) -> Result<(), Box<dyn std::error::Error>> {
    let generation = super::super::generation(running)?;
    super::super::require_current(runtime, running, &generation).await?;
    let primary = super::super::primary_process(running)?;
    let before = super::super::unix_ms()?;
    let result = crate::commands::send_instance_gm_command(
        runtime.state.clone(),
        serde_json::from_value(json!({
            "instanceId": running.summary.id, "command": command.text(),
            "processKey": primary.process_key, "transport": "stdin",
        }))?,
    )
    .await?;
    let after = super::super::unix_ms()?;
    if result.instance_id != running.summary.id
        || result.command != command.text()
        || Some(result.pid) != primary.pid
        || result.process_key != primary.process_key
        || !(before..=after).contains(&result.submitted_at_unix_ms)
    {
        return Err("Terraria clock command receipt differs from its owned request".into());
    }
    super::super::require_current(runtime, running, &generation).await
}

fn times(text: &str) -> Result<Vec<u16>, String> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            let line = line.strip_prefix(": ").unwrap_or(line);
            line.strip_prefix("Time: ")
        })
        .map(|value| {
            let (clock, suffix) = value
                .split_once(' ')
                .ok_or("Terraria time has no AM/PM suffix")?;
            let (hour, minute) = clock
                .split_once(':')
                .ok_or("Terraria time has no minute component")?;
            let hour = hour
                .parse::<u16>()
                .map_err(|_| "Terraria time has an invalid hour")?;
            let minute = minute
                .parse::<u16>()
                .map_err(|_| "Terraria time has an invalid minute")?;
            if !(1..=12).contains(&hour) || minute > 59 || !matches!(suffix, "AM" | "PM") {
                return Err("Terraria time is outside the native twelve-hour clock format".into());
            }
            Ok((hour % 12 + if suffix == "PM" { 12 } else { 0 }) * 60 + minute)
        })
        .collect()
}

async fn query<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    running: &InstanceDetails,
    target: &Target,
) -> Result<u16, Box<dyn std::error::Error>> {
    let owned = target.clone();
    let mut baseline = tokio::task::spawn_blocking(move || owned.baseline()).await??;
    dispatch(runtime, running, Command::Time).await?;
    let owned = target.clone();
    tokio::task::spawn_blocking(move || -> Result<u16, String> {
        let deadline = Instant::now() + Duration::from_secs(QUERY_SECONDS);
        loop {
            owned.current_age()?;
            let observed = times(&owned.fresh(&mut baseline)?)?;
            match observed.as_slice() {
                [minute] => return Ok(*minute),
                [] if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(100)),
                [] => return Err("Terraria did not emit a fresh native Time response".into()),
                _ => return Err("Terraria emitted ambiguous native Time responses".into()),
            }
        }
    })
    .await
    .map_err(|error| format!("Terraria time observation failed: {error}"))?
    .map_err(Into::into)
}

pub(in super::super) async fn prepare<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    running: &InstanceDetails,
    cycle: &str,
) -> Result<Option<Observation>, Box<dyn std::error::Error>> {
    if !enabled()? || running.summary.module_id != "terraria" || cycle != "initial" {
        return Ok(None);
    }
    let target = target(runtime, running).await?;
    before_autosave(
        target.current_age()?,
        Duration::from_secs(SAVE_SECONDS + 3 * QUERY_SECONDS),
    )?;
    super::super::admit_once(
        &running.summary.id,
        super::super::generation(running)?.run_id,
        "terraria_midnight",
    )?;
    // Establish the counterexample first: a default new-world clock cannot pay
    // for the deliberate midnight mutation, even if readiness/logs look healthy.
    if !(270..1170).contains(&query(runtime, running, &target).await?) {
        return Err("Terraria initial clock is not the expected fresh daytime world".into());
    }
    let changed_at = Instant::now();
    dispatch(runtime, running, Command::Midnight).await?;
    let minute = query(runtime, running, &target).await?;
    if u64::from(minute) > changed_at.elapsed().as_secs() + 2 || minute > 30 {
        return Err("Terraria midnight command did not change the native world clock".into());
    }
    tokio::task::spawn_blocking(move || -> Result<Option<Observation>, String> {
        before_autosave(target.current_age()?, Duration::from_secs(SAVE_SECONDS))?;
        let before = target
            .world()?
            .ok_or("Terraria primary world is still being written before the formal request")?;
        let baseline = target.baseline()?;
        Ok(Some(Observation {
            target,
            before,
            baseline,
            deadline: Instant::now() + Duration::from_secs(SAVE_SECONDS),
            minute,
        }))
    })
    .await
    .map_err(|error| format!("Terraria save observation preparation failed: {error}"))?
    .map_err(Into::into)
}

impl Observation {
    pub(in super::super) async fn wait(self) -> Result<(), Box<dyn std::error::Error>> {
        worker(move || self.wait_owned()).await
    }

    fn wait_owned(mut self) -> Result<(), String> {
        loop {
            if Instant::now() >= self.deadline {
                return Err("Terraria did not close exactly one changed primary world after the formal save".into());
            }
            before_autosave(self.target.current_age()?, Duration::ZERO)?;
            let fresh = self.target.fresh(&mut self.baseline)?;
            if fresh.contains("Saving before exit") {
                return Err("Terraria shutdown save cannot prove manual persistence".into());
            }
            if fresh.contains("Saving world data:")
                && let Some(after) = self.target.world()?
            {
                if after.revision
                    == self
                        .before
                        .revision
                        .checked_add(1)
                        .ok_or("Terraria save revision overflow")?
                    && after.digest != self.before.digest
                {
                    validate_clock(self.minute, after.minute, u128::from(SAVE_SECONDS))?;
                    println!(
                        "NATIVE_SAVE_OBSERVATION module=terraria receipt=requested primary_revision={} previous_revision={} stored_minute={} writer_closed=true periodic_autosave_excluded=true production_completion=not_claimed",
                        after.revision, self.before.revision, after.minute
                    );
                    return Ok(());
                }
                if after.revision != self.before.revision {
                    return Err(
                        "Terraria emitted overlapping or unexpected primary world revisions".into(),
                    );
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

/// Call after the generic initial checkpoint copy, before accepting that copy.
/// Checking only before copy would leave a slow copy crossing the autosave timer.
pub(super) fn verify_capture(running: &InstanceDetails) -> Result<(), String> {
    if running.summary.module_id != "terraria" {
        return Ok(());
    }
    let primary = super::super::primary_process(running)?;
    let identity = primary
        .process_identity
        .as_ref()
        .ok_or("Terraria process identity is missing")?;
    if !app_runtime::process_matches_identity(
        primary.pid.ok_or("Terraria primary PID is missing")?,
        identity,
    )
    .map_err(|e| e.to_string())?
    {
        return Err("Terraria process changed before checkpoint capture completed".into());
    }
    before_autosave(
        process_age_ms(
            identity.creation_time,
            super::super::unix_ms().map_err(|e| e.to_string())?,
        )?,
        Duration::ZERO,
    )
}

/// Call before generic checkpoint verification. This sends only `time`; neither
/// midnight nor another save may precede the restored checkpoint's readback.
pub(in super::super) async fn verify_loaded<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    running: &InstanceDetails,
    cycle: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if running.summary.module_id != "terraria" {
        return Ok(());
    }
    if !matches!(
        cycle,
        "restart" | "after_restore" | "after_archive_restore" | "after_program_maintenance"
    ) {
        return Err("Terraria content query requires an explicit restoration cycle".into());
    }
    let target = target(runtime, running).await?;
    {
        let owned = checkpoints()?;
        let checkpoint = owned
            .get(&running.summary.id)
            .ok_or("Terraria readback has no captured checkpoint")?;
        if !checkpoint.restored
            || checkpoint.read_back != (cycle != "restart")
            || checkpoint.run_id == super::super::generation(running)?.run_id
            || checkpoint.fixture != target.fixture
            || !checkpoint
                .manifest
                .contains_key(&relative(&target.saves, &target.world)?)
            || checkpoint
                .manifest
                .keys()
                .filter(|path| path.extension().and_then(|name| name.to_str()) == Some("wld"))
                .count()
                != 1
        {
            return Err(
                "Terraria content readback requires a fresh restored checkpoint run".into(),
            );
        }
    }
    let owned = target.clone();
    let saved = tokio::task::spawn_blocking(move || owned.world())
        .await??
        .ok_or("Terraria restored primary world is still being written before content query")?;
    before_autosave(target.current_age()?, Duration::from_secs(QUERY_SECONDS))?;
    let minute = query(runtime, running, &target).await?;
    let age = target.current_age()?;
    if age > MAX_RESTART_SECONDS * 1000 {
        return Err("Terraria content query exceeded its current-run clock bound".into());
    }
    validate_clock(saved.minute, minute, age.div_ceil(1000))?;
    println!(
        "NATIVE_CONTENT_READBACK module=terraria cycle={cycle} stored_revision={} stored_minute={} loaded_minute={minute} fresh_time_query=true clock_reset_sent=false second_save_sent=false",
        saved.revision, saved.minute
    );
    Ok(())
}

fn validate_clock(before: u16, loaded: u16, advancement_seconds: u128) -> Result<(), String> {
    // Vanilla advances one in-game minute per real second. A stopped world does
    // not advance. Midnight's native night lasts until 04:30; ordinary default
    // daylight, backwards movement and a day wrap cannot satisfy this witness.
    if before >= 270
        || loaded >= 270
        || loaded < before
        || u128::from(loaded - before) > advancement_seconds + 2
    {
        return Err(
            "Terraria reloaded a different clock or exceeded the native advancement bound".into(),
        );
    }
    Ok(())
}

/// Profiles call this on the generic checkpoint's fresh managed-log slice.
pub(super) fn require_loaded(before: &str, fresh: &str) -> Result<(), String> {
    let initial = times(before)?;
    let loaded = times(fresh)?;
    let ([.., previous], [current]) = (initial.as_slice(), loaded.as_slice()) else {
        return Err("Terraria readback lacks one unambiguous fresh clock response".into());
    };
    if !fresh.contains("Loading world data:")
        || !fresh.contains("Server started")
        || fresh.contains("Generating world")
        || fresh.contains("Load failed")
        || fresh.contains("Saving before exit")
        || fresh.lines().any(|line| {
            matches!(
                line.trim().strip_prefix(": ").unwrap_or(line.trim()),
                "midnight" | "noon" | "dawn" | "dusk"
            )
        })
    {
        return Err("Terraria did not report loading the restored native world".into());
    }
    if *previous > 30 {
        return Err("Terraria initial checkpoint has no midnight witness".into());
    }
    validate_clock(
        *previous,
        *current,
        u128::from(SAVE_SECONDS) + MAX_RESTART_SECONDS,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terraria_clock_rejects_default_time_and_allows_natural_progress() {
        validate_clock(0, 21, 20).unwrap();
        validate_clock(2, 2, 1).unwrap();
        validate_clock(150, 175, 24).unwrap();
        for (before, after, age) in [
            (0, 495, 20),
            (0, 720, 20),
            (3, 2, 20),
            (0, 23, 20),
            (269, 270, 20),
        ] {
            assert!(validate_clock(before, after, age).is_err());
        }
        assert_eq!(
            times(": Time: 12:00 AM\nTime: 12:30 PM\nTime: 7:30 AM\n").unwrap(),
            vec![0, 750, 450]
        );
        for value in [
            "Time: 00:00 AM",
            "Time: 12:60 AM",
            "Time: 1:05",
            "Time: 1:05 AM trailing",
        ] {
            assert!(times(value).is_err());
        }
        assert!(times("say Time: 12:00 AM\ntime\n").unwrap().is_empty());
    }

    #[test]
    fn terraria_readback_requires_fresh_loaded_content_and_no_second_mutation() {
        let before = "Time: 7:30 AM\nTime: 12:01 AM\n";
        let fresh = "Loading world data: 100%\n: Server started\nTime: 12:14 AM\n";
        require_loaded(before, fresh).unwrap();
        assert!(require_loaded(before, &fresh.replace("12:14 AM", "7:30 AM")).is_err());
        assert!(
            require_loaded(
                before,
                &fresh.replace("Loading world data: 100%", "Generating world")
            )
            .is_err()
        );
        assert!(require_loaded(before, &format!("{fresh}Time: 12:15 AM\n")).is_err());
        assert!(require_loaded(before, &format!("midnight\n{fresh}")).is_err());
        assert!(require_loaded("Time: 7:30 AM\n", fresh).is_err());
    }

    #[test]
    fn terraria_autosave_exclusion_includes_observation_and_capture_time() {
        before_autosave(539_999, Duration::from_secs(60)).unwrap();
        assert!(before_autosave(540_000, Duration::from_secs(60)).is_err());
        assert!(before_autosave(600_000, Duration::ZERO).is_err());
        assert!(process_age_ms(10, 20).is_err());
        assert!(process_age_ms(116_444_736_000_100_000, 9).is_err());
    }

    #[test]
    fn terraria_world_witness_rejects_unknown_header_and_tracks_primary_revision() {
        let mut bytes = vec![0; 512];
        bytes[..4].copy_from_slice(&326_i32.to_le_bytes());
        bytes[4..12].copy_from_slice(b"relogic\x02");
        bytes[12..16].copy_from_slice(&4_u32.to_le_bytes());
        bytes[24..26].copy_from_slice(&11_u16.to_le_bytes());
        bytes[26..30].copy_from_slice(&70_u32.to_le_bytes());
        bytes[30..34].copy_from_slice(&512_u32.to_le_bytes());
        bytes[70..74].copy_from_slice(b"\x01n\x01s");
        bytes[248..256].copy_from_slice(&16_200_f64.to_le_bytes());
        let before = world(&bytes).unwrap();
        assert_eq!(before.minute, 0);
        bytes[12..16].copy_from_slice(&5_u32.to_le_bytes());
        bytes[248..256].copy_from_slice(&16_920_f64.to_le_bytes());
        let after = world(&bytes).unwrap();
        assert_eq!(after.minute, 12);
        assert_eq!(after.revision, before.revision + 1);
        assert_ne!(after.digest, before.digest);
        bytes[248..256].copy_from_slice(&13_500_f64.to_le_bytes());
        bytes[256] = 1;
        assert_eq!(world(&bytes).unwrap().minute, 495);
        bytes[248..256].copy_from_slice(&f64::NAN.to_le_bytes());
        assert!(world(&bytes).is_err());
        bytes[11] = 1;
        assert!(world(&bytes).is_err());
        assert!(world(&bytes[..16]).is_err());
    }

    #[test]
    fn terraria_time_cursor_ignores_stale_and_incomplete_native_lines() {
        let root = crate::commands::tests::temp_test_dir("terraria-time");
        let log = root.join("native.log");
        fs::write(&log, "Time: 12:00 AM\n").unwrap();
        let mut baseline = log_cursor::LogBaseline::capture(&log).unwrap();
        fs::OpenOptions::new()
            .append(true)
            .open(&log)
            .unwrap()
            .write_all(b"Time: 7:30 AM")
            .unwrap();
        let read = |baseline: &mut log_cursor::LogBaseline| {
            let mut fresh = String::new();
            log_cursor::new_complete_log_line_matches(&log, baseline, |line| {
                fresh.push_str(line);
                fresh.push('\n');
                false
            })
            .unwrap();
            times(&fresh).unwrap()
        };
        assert!(read(&mut baseline).is_empty());
        fs::OpenOptions::new()
            .append(true)
            .open(&log)
            .unwrap()
            .write_all(b"\n")
            .unwrap();
        assert_eq!(read(&mut baseline), vec![450]);
        fs::remove_file(&log).unwrap();
        fs::remove_dir(&root).unwrap();
    }
}
