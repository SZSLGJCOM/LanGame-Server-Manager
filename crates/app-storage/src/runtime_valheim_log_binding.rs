use super::{
    ANCHOR_BYTES, Checkpoint, ManagedLogSegment, ValheimReadiness, checkpoint_path, digest,
    identity, load_checkpoint, region, reject_links, scan, store_checkpoint,
};
use crate::StartupLogReadiness;
use app_core::{ActiveInstanceRun, ProcessIdentity};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

const HEAD_BYTES: u64 = 4096;

/// The startup cursor has already checked this handle and complete-line boundary
/// against its pre-spawn baseline. No log bytes or private session tokens persist.
pub struct ValheimLogHandoff<'a> {
    pub console_path: &'a Path,
    pub native_path: &'a Path,
    pub file: &'a mut File,
    pub offset: u64,
    pub discard_fragment: bool,
    pub session_id: &'a str,
    pub process_key: &'a str,
    pub pid: u32,
    pub identity: &'a ProcessIdentity,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RunOwner {
    console_path: String,
    session_id: String,
    process_key: String,
    pub(super) pid: u32,
    pub(super) identity: ProcessIdentity,
}

impl RunOwner {
    fn valid(&self) -> bool {
        self.pid > 0
            && self.identity.creation_time > 0
            && !self.identity.image_path.is_empty()
            && !self.session_id.is_empty()
            && !self.process_key.is_empty()
            && Path::new(&self.console_path).is_absolute()
    }

    fn from_run(run: &ActiveInstanceRun) -> Option<Self> {
        let mut primary = run.processes.iter().filter(|process| process.is_primary);
        let process = primary.next()?;
        if primary.next().is_some()
            || process.run_id != run.run_id
            || process.session_id != run.session_id
            || process.pid != run.pid
            || process.log_path != run.log_path
            || process.status != "running"
        {
            return None;
        }
        let owner = Self {
            console_path: process.log_path.clone()?,
            session_id: process.session_id.clone()?,
            process_key: process.process_key.clone(),
            pid: process.pid?,
            identity: process.process_identity.clone()?,
        };
        owner.valid().then_some(owner)
    }

    fn follows(&self, previous: &Self) -> bool {
        self.valid()
            && self.console_path != previous.console_path
            && Path::new(&self.console_path).parent() == Path::new(&previous.console_path).parent()
            && self.session_id != previous.session_id
            && self.process_key == previous.process_key
            && self.identity.creation_time > previous.identity.creation_time
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct NativeBinding {
    path: String,
    head_length: u64,
    head_digest: [u8; 32],
    observed_length: u64,
    modified_ns: u64,
    unavailable: bool,
}

pub(super) fn validate_checkpoint(state: &Checkpoint) -> io::Result<()> {
    if state
        .owner
        .as_ref()
        .is_some_and(|owner| !owner.valid() || owner.console_path != state.source)
        || state.native_binding.as_ref().is_some_and(|binding| {
            state.owner.is_none()
                || !Path::new(&binding.path).is_absolute()
                || binding.path == state.source
                || binding.head_length > HEAD_BYTES
                || binding.head_length > binding.observed_length
                || state.segment_start != 0
                || state.offset > binding.observed_length
        })
    {
        return Err(io::Error::other("Invalid Valheim native log binding"));
    }
    Ok(())
}

#[cfg(windows)]
pub(super) fn process_is_current(pid: u32, identity: &ProcessIdentity) -> io::Result<bool> {
    app_runtime::process_matches_identity(pid, identity).map_err(io::Error::other)
}

#[cfg(not(windows))]
pub(super) fn process_is_current(_: u32, _: &ProcessIdentity) -> io::Result<bool> {
    Err(io::Error::other(
        "Valheim native log ownership verification requires Windows",
    ))
}

pub fn record_valheim_native_log(
    handoff: ValheimLogHandoff<'_>,
    readiness: &StartupLogReadiness,
) -> io::Result<()> {
    record_with(handoff, readiness, process_is_current)
}

fn record_with(
    handoff: ValheimLogHandoff<'_>,
    readiness: &StartupLogReadiness,
    mut current: impl FnMut(u32, &ProcessIdentity) -> io::Result<bool>,
) -> io::Result<()> {
    let typed = readiness
        .valheim_state()
        .filter(|state| state.is_ready() && readiness.is_ready())
        .ok_or_else(|| io::Error::other("Valheim handoff requires completed startup evidence"))?;
    let console = path_string(handoff.console_path)?;
    let native = path_string(handoff.native_path)?;
    let owner = RunOwner {
        console_path: console.clone(),
        session_id: handoff.session_id.into(),
        process_key: handoff.process_key.into(),
        pid: handoff.pid,
        identity: handoff.identity.clone(),
    };
    if !owner.valid() || native == console || !current(owner.pid, &owner.identity)? {
        return Err(io::Error::other("Valheim startup log owner is not current"));
    }
    let checkpoint = checkpoint_path(handoff.console_path)?;
    reject_links(handoff.native_path)?;
    let (length, modified_ns) = stamp(handoff.file)?;
    if handoff.offset > length
        || (!handoff.discard_fragment
            && handoff.offset > 0
            && !matches!(
                region(handoff.file, handoff.offset - 1, 1)?.as_slice(),
                [b'\n' | b'\r']
            ))
    {
        return Err(io::Error::other(
            "Valheim handoff is not at a complete log boundary",
        ));
    }
    let mut replacement = Checkpoint {
        source: console,
        offset: handoff.offset,
        file_identity: Some(identity(handoff.file)?),
        readiness: typed,
        discard_fragment: handoff.discard_fragment,
        owner: Some(owner.clone()),
        native_binding: Some(NativeBinding {
            path: native,
            head_length: 0,
            head_digest: [0; 32],
            observed_length: length,
            modified_ns,
            unavailable: false,
        }),
        ..Default::default()
    };
    capture_digests(handoff.file, &mut replacement)?;
    if !same_source(handoff.native_path, &replacement)? {
        return Err(io::Error::other(
            "Valheim native log changed during handoff",
        ));
    }
    for _ in 0..3 {
        let (expected, previous) = load_checkpoint(&checkpoint)?;
        if let Some(previous_owner) = &previous.owner {
            if previous_owner == &owner {
                // A repeated completion must not rewind health past a revocation.
                return if previous.native_binding.as_ref().map(|value| &value.path)
                    == replacement.native_binding.as_ref().map(|value| &value.path)
                {
                    Ok(())
                } else {
                    Err(io::Error::other(
                        "Valheim run already has a different log binding",
                    ))
                };
            }
            if !owner.follows(previous_owner)
                || current(previous_owner.pid, &previous_owner.identity)?
            {
                return Err(io::Error::other(
                    "Valheim log binding belongs to another run",
                ));
            }
        }
        if !current(owner.pid, &owner.identity)? || !same_source(handoff.native_path, &replacement)?
        {
            return Err(io::Error::other(
                "Valheim startup owner or log changed before publication",
            ));
        }
        // The next DB run id is not allocated yet. Retain the prior id as a
        // lower bound; the first matching registered owner seals the actual id.
        replacement.run_id = previous.run_id;
        if store_checkpoint(&checkpoint, expected.as_deref(), &replacement)? {
            return Ok(());
        }
    }
    Err(io::Error::other(
        "Valheim log binding changed during publication",
    ))
}

pub(super) fn adopt_run(
    state: &mut Checkpoint,
    run: &ActiveInstanceRun,
    current: &mut impl FnMut(u32, &ProcessIdentity) -> io::Result<bool>,
) -> io::Result<bool> {
    if state.run_id > run.run_id {
        return Ok(false);
    }
    let source = run.log_path.as_deref().unwrap_or_default();
    let owner = RunOwner::from_run(run);
    let keep_owner = state.owner.is_some();
    if let Some(previous) = &state.owner {
        if owner.as_ref() == Some(previous) {
            state.run_id = run.run_id;
            return Ok(true);
        }
        let Some(next) = &owner else { return Ok(false) };
        if run.run_id <= state.run_id
            || !next.follows(previous)
            || !current(next.pid, &next.identity)?
            || current(previous.pid, &previous.identity)?
        {
            return Ok(false);
        }
    }
    if state.run_id != run.run_id || state.source != source {
        *state = Checkpoint {
            run_id: run.run_id,
            source: source.into(),
            // Legacy managed transcripts keep their existing run-id contract.
            // Clearing a native binding retains the verified replacement owner.
            owner: if keep_owner { owner } else { None },
            ..Default::default()
        };
    }
    Ok(true)
}

pub(super) fn invalidate_native(state: &mut Checkpoint) {
    state.readiness = ValheimReadiness::default();
    if let Some(binding) = state.native_binding.as_mut() {
        binding.unavailable = true;
        // A read error may happen after advancing past the previous observed
        // length. Persist a valid revoked cursor; unavailable requires an EOF
        // baseline before any later bytes can be consumed, even if the old
        // file reappears under the same name and identity.
        state.offset = 0;
        state.segment_start = 0;
        state.file_identity = None;
        state.anchor_length = 0;
        state.anchor_digest = [0; 32];
        state.discard_fragment = false;
    }
}

pub(super) fn advance_native(state: &mut Checkpoint) -> io::Result<bool> {
    let path = PathBuf::from(&native_binding(state)?.path);
    reject_links(&path)?;
    let file = match File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            invalidate_native(state);
            return Ok(false);
        }
        Err(error) => return Err(error),
    };
    let mut segments = [ManagedLogSegment {
        path: path.clone(),
        file,
        start_offset: 0,
    }];
    let file = &mut segments[0].file;
    if !matches_file(file, state)? {
        baseline_at_end(file, state)?;
        return Ok(false);
    }
    let (end, _) = stamp(file)?;
    let previous_boundary = state.clone();
    let caught_up = scan(&mut segments, state, end)?;
    let file = &mut segments[0].file;
    // Check the old head as well as the committed boundary. An in-place rewrite
    // can preserve the file id and the trailing bytes around the cursor.
    if !matches_file(file, &previous_boundary)? {
        baseline_at_end(file, state)?;
        return Ok(false);
    }
    capture_digests(file, state)?;
    if !same_source(&path, state)? {
        invalidate_native(state);
        return Ok(false);
    }
    Ok(caught_up && stamp(file)?.0 == end)
}

fn baseline_at_end(file: &mut File, state: &mut Checkpoint) -> io::Result<()> {
    invalidate_native(state);
    let (end, _) = stamp(file)?;
    state.offset = end;
    state.segment_start = 0;
    state.file_identity = Some(identity(file)?);
    state.discard_fragment =
        end > 0 && !matches!(region(file, end - 1, 1)?.as_slice(), [b'\n' | b'\r']);
    capture_digests(file, state)?;
    state
        .native_binding
        .as_mut()
        .ok_or_else(|| io::Error::other("Missing Valheim log binding"))?
        .unavailable = false;
    Ok(())
}

fn capture_digests(file: &mut File, state: &mut Checkpoint) -> io::Result<()> {
    let (length, modified_ns) = stamp(file)?;
    if length < state.offset {
        return Err(io::Error::other("Valheim log shrank during observation"));
    }
    state.anchor_length = state.offset.min(ANCHOR_BYTES);
    state.anchor_digest = digest(
        file,
        state.offset - state.anchor_length,
        state.anchor_length,
    )?;
    let binding = state
        .native_binding
        .as_mut()
        .ok_or_else(|| io::Error::other("Missing Valheim log binding"))?;
    binding.head_length = length.min(HEAD_BYTES);
    binding.head_digest = digest(file, 0, binding.head_length)?;
    binding.observed_length = length;
    binding.modified_ns = modified_ns;
    Ok(())
}

fn matches_file(file: &mut File, state: &Checkpoint) -> io::Result<bool> {
    let binding = native_binding(state)?;
    let (length, modified_ns) = stamp(file)?;
    Ok(!binding.unavailable
        && state.file_identity.as_ref() == Some(&identity(file)?)
        && length >= state.offset
        && length >= binding.observed_length
        && (length != binding.observed_length || modified_ns == binding.modified_ns)
        && digest(file, 0, binding.head_length)? == binding.head_digest
        && digest(
            file,
            state.offset - state.anchor_length,
            state.anchor_length,
        )? == state.anchor_digest)
}

fn same_source(path: &Path, state: &Checkpoint) -> io::Result<bool> {
    reject_links(path)?;
    match File::open(path) {
        Ok(mut file) => matches_file(&mut file, state),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn native_binding(state: &Checkpoint) -> io::Result<&NativeBinding> {
    state
        .native_binding
        .as_ref()
        .ok_or_else(|| io::Error::other("Missing Valheim log binding"))
}

fn stamp(file: &File) -> io::Result<(u64, u64)> {
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::other("Valheim log is not a regular file"));
    }
    let modified_ns = u64::try_from(
        metadata
            .modified()?
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos(),
    )
    .map_err(io::Error::other)?;
    Ok((metadata.len(), modified_ns))
}

fn path_string(path: &Path) -> io::Result<String> {
    path.to_str()
        .filter(|_| path.is_absolute())
        .map(str::to_owned)
        .ok_or_else(|| io::Error::other("Valheim log requires an absolute Unicode path"))
}

#[cfg(test)]
#[path = "runtime_valheim_log_binding_tests.rs"]
mod tests;
