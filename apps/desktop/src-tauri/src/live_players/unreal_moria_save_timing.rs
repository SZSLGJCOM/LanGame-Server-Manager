//! Opt-in native acceptance evidence; no production protocol or timer mutation.
use super::*;
use sha2::{Digest, Sha256};
use std::io::Read;

// Build 21872765, matching Shipping/PDB: StartAutoSave reads SaveGameData's
// AutoSaveTime and calls InternalSetTimer(loop=true, first_delay=-1). That
// selects the full interval for the first expiration, including pending timers.
// This is a bound for this disposable unmodified build, not a timer API promise.
// The harness supplies a fully hashed clean source. Engine timers use game time:
// the oracle also observes dilation=1 and game<=real<=process age, and assumes the
// vanilla fixture has no unobserved clock acceleration. It does not inspect a
// private TimerHandle or claim a completion receipt from these observations.
const SHIPPING_SHA256: &str = "4513fb58ca31a3c7c7640ebeff9717ef68776b922986036f0c5b325dd28255fb";
const EVIDENCE: &str = "moria-test-timing.jsonl";

#[derive(Clone)]
pub(crate) struct Timing {
    root: PathBuf,
    launch_token: String,
    native: process::NativeProcess,
    interval: f64,
    age_at_anchor: Duration,
    anchor: Instant,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Sample {
    pub interval: f64,
    pub game_seconds: f64,
    pub real_seconds: f64,
    pub shipping_age: f64,
}

#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Stage {
    Inspect,
    Before,
    After,
}

impl Stage {
    fn label(&self) -> &'static str {
        match self {
            Self::Inspect => "inspect",
            Self::Before => "before",
            Self::After => "after",
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Cause {
    ReflectionUnavailable,
    WorldChanged,
    ClockUnusable,
    ObservationLimit,
}

impl Cause {
    fn label(&self) -> &'static str {
        match self {
            Self::ReflectionUnavailable => "reflection_unavailable",
            Self::WorldChanged => "world_changed",
            Self::ClockUnusable => "clock_unusable",
            Self::ObservationLimit => "observation_limit",
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    run: String,
    sequence: usize,
    stage: Stage,
    valid: bool,
    cause: Option<Cause>,
    interval: Option<f64>,
    game_seconds: Option<f64>,
    real_seconds: Option<f64>,
    dilation: Option<f64>,
}

fn parse(bytes: &[u8], launch: &str, age: Duration, after: bool) -> Result<Sample, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "Moria timer evidence is not UTF-8")?;
    if !text.ends_with('\n') {
        return Err("Moria timer evidence is incomplete".into());
    }
    let mut previous: Option<Sample> = None;
    let (mut before_count, mut after_count) = (0, 0);
    for (index, line) in text.lines().enumerate() {
        let record: Record = serde_json::from_str(line)
            .map_err(|_| "Moria timer evidence has an unsupported or incomplete record")?;
        if index >= 64
            || record.sequence != index + 1
            || record.run != launch
            || (index == 0 && record.stage != Stage::Inspect)
        {
            return Err("Moria timer evidence has a stale run or invalid sequence".into());
        }
        if !record.valid || record.cause.is_some() {
            return Err(format!(
                "Moria timer observation rejected at {}: {}",
                record.stage.label(),
                record.cause.as_ref().map_or("invalid", Cause::label)
            ));
        }
        let sample = Sample {
            interval: record
                .interval
                .ok_or("Moria timer interval is unavailable")?,
            game_seconds: record
                .game_seconds
                .ok_or("Moria game clock is unavailable")?,
            real_seconds: record
                .real_seconds
                .ok_or("Moria real clock is unavailable")?,
            shipping_age: age.as_secs_f64(),
        };
        if !sample.interval.is_finite()
            || sample.interval <= 0.0
            || !sample.game_seconds.is_finite()
            || sample.game_seconds < 0.0
            || !sample.real_seconds.is_finite()
            || sample.game_seconds > sample.real_seconds
            || sample.real_seconds > sample.shipping_age
            || record.dilation != Some(1.0)
        {
            return Err("Moria timer cannot be bounded by the Shipping wall-clock age".into());
        }
        if let Some(previous) = previous
            && (previous.interval != sample.interval
                || previous.game_seconds > sample.game_seconds
                || previous.real_seconds > sample.real_seconds)
        {
            return Err("Moria timer interval changed or its world clocks moved backwards".into());
        }
        match record.stage {
            Stage::Inspect => {}
            Stage::Before => before_count += 1,
            Stage::After if before_count == 1 => after_count += 1,
            Stage::After => return Err("Moria timer evidence has no preceding save entry".into()),
        }
        if before_count > 1 || after_count > 1 {
            return Err("Moria timing trial contains more than one native save invocation".into());
        }
        previous = Some(sample);
    }
    if (before_count, after_count) != if after { (1, 1) } else { (0, 0) } {
        return Err("Moria timer evidence lacks the required one-call observation phases".into());
    }
    let sample = previous.ok_or("Moria timer evidence is empty")?;
    before_first_periodic(age, sample.interval)?;
    Ok(sample)
}

fn process_age(creation: u64, now: Duration) -> Result<Duration, String> {
    let nanos = creation
        .checked_sub(116_444_736_000_000_000)
        .and_then(|ticks| ticks.checked_mul(100))
        .ok_or("Moria timing requires a valid Windows Shipping creation time")?;
    now.checked_sub(Duration::from_nanos(nanos))
        .ok_or_else(|| "Moria Shipping creation time is in the future".into())
}

fn before_first_periodic(age: Duration, interval: f64) -> Result<(), String> {
    if !interval.is_finite() || interval <= 0.0 || age.as_secs_f64() >= interval {
        return Err("Moria checkpoint overlaps the first possible native periodic save".into());
    }
    Ok(())
}

fn age(native: &process::NativeProcess) -> Result<Duration, String> {
    if !app_runtime::process_matches_identity(native.pid, &native.identity)
        .map_err(|e| e.to_string())?
    {
        return Err("Moria timer observation lost its Shipping generation".into());
    }
    process_age(
        native.identity.creation_time,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "Moria timer observation has no valid wall clock")?,
    )
}

fn evidence(context: &Context, age: Duration, after: bool) -> Result<Sample, String> {
    let directory = context.root.join("langame_world_save");
    let _pins = installation::pin(&directory, false)?;
    let bytes = installation::read(&directory.join(EVIDENCE), 64 * 1024)
        .map_err(|e| format!("Read Moria native timer evidence: {e}"))?;
    parse(&bytes, &context.launch_token, age, after)
}

fn verify_content(root: &Path) -> Result<(), String> {
    // Source content hashes belong to the disposable-fixture admission. This
    // narrow guard additionally refuses injected pak/mod directories at runtime.
    let directory = root.join("Moria/Content/Paks");
    let _pins = installation::pin(&directory, false)?;
    let mut names = std::collections::BTreeSet::new();
    for entry in fs::read_dir(directory).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if !entry.file_type().map_err(|e| e.to_string())?.is_file() {
            return Err("Moria timing fixture contains an added content directory or link".into());
        }
        names.insert(entry.file_name().to_string_lossy().into_owned());
    }
    let expected = [
        "global.ucas",
        "global.utoc",
        "Moria-WindowsServer.pak",
        "Moria-WindowsServer.ucas",
        "Moria-WindowsServer.utoc",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    if names != expected {
        return Err("Moria timing fixture contains missing or added content packages".into());
    }
    Ok(())
}

impl Timing {
    pub(crate) fn begin(details: &InstanceDetails) -> Result<(Self, Sample), String> {
        let context = context(details)?;
        if context.game != Game::Moria {
            return Err("Native timer evidence is restricted to Moria acceptance".into());
        }
        let _pins = installation::validate(Game::Moria, &context.root)?;
        verify_owned_mods(Game::Moria.id(), &context.root)?;
        verify_content(&context.root)?;
        if !installation::complete(Game::Moria, &context.root, &Game::Moria.payload())? {
            return Err("Moria timer observation requires the exact test payload".into());
        }
        let mut image = fs::File::open(&context.native.identity.image_path)
            .map_err(|e| format!("Read Moria timing-contract image: {e}"))?;
        let mut hash = Sha256::new();
        let mut buffer = [0u8; 65536];
        loop {
            let count = image.read(&mut buffer).map_err(|e| e.to_string())?;
            if count == 0 {
                break;
            }
            hash.update(&buffer[..count]);
        }
        let digest: String = hash
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        if digest != SHIPPING_SHA256 {
            return Err("Moria timer semantics are not verified for this Shipping image".into());
        }
        // The production protocol inspects a world only for an explicit probe
        // or save request. Readiness alone does not execute this Lua observer.
        // Probe once through that same owned exchange; no save is invoked.
        preflight(&context)?;
        let anchor = Instant::now();
        let age_at_anchor = age(&context.native)?;
        let sample = evidence(&context, age_at_anchor, false)?;
        Ok((
            Self {
                root: context.root,
                launch_token: context.launch_token,
                native: context.native,
                interval: sample.interval,
                age_at_anchor,
                anchor,
            },
            sample,
        ))
    }

    pub(crate) fn verify(&self, details: &InstanceDetails, after: bool) -> Result<Sample, String> {
        let context = context(details)?;
        if context.root != self.root
            || context.launch_token != self.launch_token
            || context.native.pid != self.native.pid
            || !app_runtime::process_identities_match(
                &context.native.identity,
                &self.native.identity,
            )
        {
            return Err("Moria timer evidence changed its owned process or run".into());
        }
        verify_owned_mods(Game::Moria.id(), &context.root)?;
        verify_content(&context.root)?;
        // A backwards wall-clock adjustment cannot extend this acceptance window.
        let age = age(&self.native)?.max(self.age_at_anchor + self.anchor.elapsed());
        let sample = evidence(&context, age, after)?;
        if sample.interval != self.interval {
            return Err("Moria autosave interval changed during the manual-save trial".into());
        }
        Ok(sample)
    }
}

#[path = "unreal_moria_save_timing_tests.rs"]
mod tests;
