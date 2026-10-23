//! Test-only content oracle for the exact pre-shutdown Moria checkpoint.
//! Difficulty is read from the native running world, never inferred from a
//! timestamp, readiness, a save receipt, or the create-world configuration.
use super::*;
#[cfg(feature = "native-test-console")]
use crate::live_players::file_ipc::unreal_world_save::moria_test_timing::Timing;
use std::time::{Duration, Instant};

#[derive(Clone)]
pub(in super::super) struct Observation {
    fixture: PathBuf,
    saves: PathBuf,
    running: InstanceDetails,
    before: Manifest,
    expected: Expected,
}

#[derive(Clone)]
struct Expected {
    fixture: PathBuf,
    saves: PathBuf,
    world: PathBuf,
    initial_run: i64,
    initial_creation: u64,
    config_digest: (u64, String),
    #[cfg(feature = "native-test-console")]
    timing: Timing,
    captured_before_periodic: bool,
}

static EXPECTED: OnceLock<Mutex<BTreeMap<String, Expected>>> = OnceLock::new();
static MUTATIONS: OnceLock<Mutex<std::collections::HashSet<(String, i64)>>> = OnceLock::new();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Difficulty {
    Normal,
    Hard,
}

fn world_file(manifest: &Manifest) -> Result<PathBuf, String> {
    let worlds = manifest
        .keys()
        .filter(|name| {
            name.components().count() == 1
                && name.to_str().is_some_and(|name| {
                    name.strip_prefix("MW_")
                        .and_then(|name| name.strip_suffix(".sav"))
                        .is_some_and(|id| {
                            id.len() == 32 && id.bytes().all(|c| c.is_ascii_hexdigit())
                        })
                })
        })
        .collect::<Vec<_>>();
    match worlds.as_slice() {
        [world] => Ok((*world).clone()),
        _ => Err("Moria content oracle requires exactly one native world file".into()),
    }
}

fn require_normal_create_config(text: &str) -> Result<(), String> {
    let mut section = "";
    let mut presets = Vec::new();
    for line in text.lines().map(str::trim) {
        if line.starts_with([';', '#']) || line.is_empty() {
            continue;
        }
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|value| value.strip_suffix(']'))
        {
            section = name;
        } else if section.eq_ignore_ascii_case("World.Create")
            && let Some((key, value)) = line.split_once('=')
            && key.trim().eq_ignore_ascii_case("Difficulty.Preset")
        {
            presets.push(value.trim());
        }
    }
    if presets.len() != 1 || !presets[0].eq_ignore_ascii_case("normal") {
        return Err(
            "Moria content oracle requires unchanged Normal world-creation settings".into(),
        );
    }
    Ok(())
}

fn config(program: &Path) -> Result<(u64, String), String> {
    let path = program.join("MoriaServerConfig.ini");
    let bytes = guard_read_regular(&path, 128 * 1024)?;
    require_normal_create_config(
        std::str::from_utf8(&bytes).map_err(|_| "Moria native configuration is not UTF-8")?,
    )?;
    digest(&path)
}

fn parse_difficulty(text: &str, nonce: &str) -> Result<Difficulty, String> {
    let begin = format!("Unknown command \"LGM_PLAYER_QUERY_BEGIN_{nonce}\"!");
    let end = format!("Unknown command \"LGM_PLAYER_QUERY_END_{nonce}\"!");
    let lines = text
        .lines()
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    if lines.first() != Some(&begin.as_str())
        || lines.last() != Some(&end.as_str())
        || lines.iter().filter(|line| **line == "> difficulty").count() != 1
        || lines
            .iter()
            .filter(|line| line.starts_with("Unknown command "))
            .count()
            != 2
        || lines.iter().filter(|line| line.starts_with("> ")).count() != 2
        || lines.get(lines.len().saturating_sub(2))
            != Some(&format!("> LGM_PLAYER_QUERY_END_{nonce}").as_str())
    {
        return Err("Moria difficulty query has no complete fresh command frame".into());
    }
    let presets = lines
        .iter()
        .filter_map(|line| line.strip_prefix("Preset: "))
        .collect::<Vec<_>>();
    match presets.as_slice() {
        ["Normal"] => Ok(Difficulty::Normal),
        ["Hard"] => Ok(Difficulty::Hard),
        _ => Err("Moria difficulty query did not return one recognized native preset".into()),
    }
}

fn primary(running: &InstanceDetails) -> Result<(u32, &app_core::ProcessIdentity), String> {
    super::super::generation(running)?;
    let process = super::super::primary_process(running)?;
    let pid = process
        .pid
        .ok_or("Moria content oracle has no primary PID")?;
    let identity = process
        .process_identity
        .as_ref()
        .ok_or("Moria content oracle has no process generation")?;
    if !app_runtime::process_matches_identity(pid, identity).map_err(|e| e.to_string())? {
        return Err("Moria content oracle lost its running process generation".into());
    }
    Ok((pid, identity))
}

#[cfg(feature = "native-test-console")]
fn execute(running: &InstanceDetails, set_hard: bool) -> Result<(String, String), String> {
    use app_runtime::native_player_console::test_support::{self, Command};
    let (pid, identity) = primary(running)?;
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    // libtest names omit the crate prefix. Derive the exact current child name
    // so moving this test module cannot silently select zero helper tests.
    let module = module_path!()
        .split_once("::")
        .ok_or("Moria helper module path is unavailable")?
        .1;
    let operation = if set_hard {
        Command::SetHard
    } else {
        Command::QueryDifficulty
    };
    let text = test_support::execute(
        pid,
        identity,
        &nonce,
        operation,
        &format!("{module}::console_helper_entry"),
    )
    .map_err(|error| {
        format!("Moria content console operation {operation:?} failed: {error}; not retried")
    })?;
    Ok((text, nonce))
}

#[cfg(not(feature = "native-test-console"))]
fn execute(_: &InstanceDetails, _: bool) -> Result<(String, String), String> {
    Err("Moria content acceptance requires the explicit native-test-console feature".into())
}

fn query(running: &InstanceDetails) -> Result<Difficulty, String> {
    let (text, nonce) = execute(running, false)?;
    parse_difficulty(&text, &nonce)
}

#[cfg(feature = "native-test-console")]
#[test]
#[ignore = "private child for the opt-in disposable Moria difficulty oracle"]
fn console_helper_entry() {
    std::process::exit(app_runtime::native_player_console::test_support::run_helper().unwrap_or(2));
}

pub(in super::super) async fn prepare<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    running: &InstanceDetails,
    cycle: &str,
) -> Result<Option<Observation>, Box<dyn std::error::Error>> {
    if !enabled()? || running.summary.module_id != "returntomoria" || cycle != "initial" {
        return Ok(None);
    }
    let (fixture, running) = input(runtime, running)?;
    tokio::task::spawn_blocking(move || prepare_owned(&fixture, &running))
        .await
        .map_err(|e| format!("Moria content preparation worker failed: {e}"))?
        .map(Some)
        .map_err(Into::into)
}

fn prepare_owned(fixture: &Path, running: &InstanceDetails) -> Result<Observation, String> {
    let (fixture, saves, program) = binding(fixture, running)?;
    let config_digest = config(&program)?;
    let (_, identity) = primary(running)?;
    let initial_run = running
        .active_run
        .as_ref()
        .ok_or("Moria has no active run")?
        .run_id;
    let initial_creation = identity.creation_time;
    #[cfg(feature = "native-test-console")]
    let (timing, sample) = Timing::begin(running)?;
    #[cfg(feature = "native-test-console")]
    println!(
        "NATIVE_MORIA_TIMER phase=admitted build=21872765 interval_seconds={} game_seconds={} real_seconds={} shipping_age_seconds={} time_dilation=1 no_other_mods=true clock_assumption=vanilla_no_acceleration",
        sample.interval, sample.game_seconds, sample.real_seconds, sample.shipping_age
    );
    if query(running)? != Difficulty::Normal {
        return Err("Moria content mutation requires an initially Normal running world".into());
    }
    let roots = profiles::roots("returntomoria", &saves)?;
    let before = files(&saves, &roots)?;
    let world = world_file(&before)?;
    if !MUTATIONS
        .get_or_init(|| Mutex::new(Default::default()))
        .lock()
        .map_err(|_| "Moria mutation ownership lock failed")?
        .insert((running.summary.id.clone(), initial_run))
    {
        return Err("Moria content mutation was already attempted; not retried".into());
    }
    execute(running, true)?;
    if query(running)? != Difficulty::Hard {
        return Err("Moria did not independently report the requested Hard world change".into());
    }
    let expected = Expected {
        fixture: fixture.clone(),
        saves: saves.clone(),
        world,
        initial_run,
        initial_creation,
        config_digest,
        #[cfg(feature = "native-test-console")]
        timing,
        captured_before_periodic: false,
    };
    let observation = Observation {
        fixture,
        saves,
        running: running.clone(),
        before,
        expected,
    };
    observation.require_unchanged()?;
    println!(
        "NATIVE_MORIA_CONTENT phase=mutated before=Normal current=Hard native_query=true save_bytes_unchanged=true create_config=Normal"
    );
    Ok(observation)
}

impl Observation {
    fn require_unchanged(&self) -> Result<(), String> {
        let (_, saves, program) = binding(&self.fixture, &self.running)?;
        primary(&self.running)?;
        if saves != self.saves
            || config(&program)? != self.expected.config_digest
            || files(&saves, &profiles::roots("returntomoria", &saves)?)? != self.before
        {
            return Err("Moria data changed before the formal manual save; this trial cannot establish a checkpoint".into());
        }
        #[cfg(feature = "native-test-console")]
        self.expected.timing.verify(&self.running, false)?;
        Ok(())
    }

    pub(in super::super) async fn before_save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let observation = self.clone();
        worker(move || observation.require_unchanged()).await
    }

    pub(in super::super) async fn wait(self) -> Result<(), Box<dyn std::error::Error>> {
        worker(move || self.wait_owned()).await
    }

    fn wait_owned(self) -> Result<(), String> {
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut previous = None;
        loop {
            primary(&self.running)?;
            let (_, saves, program) = binding(&self.fixture, &self.running)?;
            if saves != self.saves || config(&program)? != self.expected.config_digest {
                return Err("Moria checkpoint binding or Normal creation settings changed".into());
            }
            let after = files(&saves, &profiles::roots("returntomoria", &saves)?)?;
            if world_file(&after)? != self.expected.world {
                return Err("Moria wrote a different or ambiguous native world".into());
            }
            if after.get(&self.expected.world) != self.before.get(&self.expected.world)
                && previous.as_ref() == Some(&after)
            {
                #[cfg(feature = "native-test-console")]
                self.expected.timing.verify(&self.running, true)?;
                let mut expected = EXPECTED
                    .get_or_init(|| Mutex::new(BTreeMap::new()))
                    .lock()
                    .map_err(|_| "Moria content expectation lock failed")?;
                if expected.contains_key(&self.running.summary.id) {
                    return Err(
                        "Moria content expectation was already registered; not replaced".into(),
                    );
                }
                expected.insert(self.running.summary.id, self.expected);
                println!(
                    "NATIVE_MORIA_CONTENT phase=snapshot_candidate world_bytes_changed=true captured_before_stop=pending native_readback=pending periodic_autosave_excluded=pending_capture production_completion=not_claimed"
                );
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err("Moria world bytes did not produce a stable candidate after the one formal save request; not retried".into());
            }
            previous = Some(after);
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

/// The generic copy calls this after its final byte. A check before copy cannot
/// exclude a periodic save that becomes due while the checkpoint is copied.
pub(super) fn verify_capture(running: &InstanceDetails) -> Result<(), String> {
    if running.summary.module_id != "returntomoria" {
        return Ok(());
    }
    #[cfg(not(feature = "native-test-console"))]
    return Err("Moria causal acceptance requires the native-test-console feature".into());
    #[cfg(feature = "native-test-console")]
    {
        let mut expected = EXPECTED
            .get_or_init(|| Mutex::new(BTreeMap::new()))
            .lock()
            .map_err(|_| "Moria content expectation lock failed")?;
        let expected = expected
            .get_mut(&running.summary.id)
            .ok_or("Moria has no pre-stop timer expectation")?;
        primary(running)?;
        let sample = expected.timing.verify(running, true)?;
        expected.captured_before_periodic = true;
        println!(
            "NATIVE_MORIA_TIMER phase=captured interval_seconds={} shipping_age_seconds={} game_seconds={} real_seconds={} periodic_autosave_excluded=true clock_assumption=vanilla_no_acceleration",
            sample.interval, sample.shipping_age, sample.game_seconds, sample.real_seconds
        );
        Ok(())
    }
}

pub(super) fn verify_loaded_owned(
    fixture: &Path,
    running: &InstanceDetails,
    cycle: &str,
) -> Result<(), String> {
    if !matches!(
        cycle,
        "restart" | "after_restore" | "after_archive_restore" | "after_program_maintenance"
    ) {
        return Err("Moria content reload requires an explicit restoration cycle".into());
    }
    let expected = EXPECTED
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .map_err(|_| "Moria content expectation lock failed")?
        .get(&running.summary.id)
        .cloned()
        .ok_or("Moria has no pre-stop content expectation")?;
    let (fixture, saves, program) = binding(fixture, running)?;
    let (_, identity) = primary(running)?;
    let run_id = running
        .active_run
        .as_ref()
        .ok_or("Moria readback has no active run")?
        .run_id;
    if fixture != expected.fixture
        || !expected.captured_before_periodic
        || saves != expected.saves
        || run_id == expected.initial_run
        || identity.creation_time == expected.initial_creation
        || config(&program)? != expected.config_digest
        || world_file(&files(&saves, &profiles::roots("returntomoria", &saves)?)?)?
            != expected.world
    {
        return Err("Moria readback is not a new owned run of the expected world with Normal creation settings".into());
    }
    if query(running)? != Difficulty::Hard {
        return Err("Moria restored world did not load the prior Hard content change".into());
    }
    println!(
        "NATIVE_MORIA_CONTENT phase=reloaded cycle={cycle} preset=Hard create_config=Normal native_query=true different_run=true periodic_autosave_excluded=true production_completion=not_claimed"
    );
    Ok(())
}

pub(in super::super) async fn verify_loaded<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    running: &InstanceDetails,
    cycle: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if !enabled()? || running.summary.module_id != "returntomoria" {
        return Ok(());
    }
    let (fixture, running) = input(runtime, running)?;
    let cycle = cycle.to_owned();
    worker(move || verify_loaded_owned(&fixture, &running, &cycle)).await
}

#[cfg(test)]
#[path = "commands_native_moria_save_observation_tests.rs"]
mod tests;
