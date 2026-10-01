use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};

#[path = "managed_console_log_fs.rs"]
pub(crate) mod owned_fs;
#[path = "managed_console_log_store.rs"]
mod store;
use owned_fs::reject_links;
use store::{Part, Store};

const SEGMENT_BYTES: u64 = 8 * 1024 * 1024;
const INSTANCE_BYTES: u64 = 256 * 1024 * 1024;
const RUN_SEGMENTS: usize = 4;
const MAX_RUNS: usize = 128;

#[derive(Clone, Copy)]
struct LogLimits {
    segment_bytes: u64,
    instance_bytes: u64,
    run_segments: usize,
    runs: usize,
}
const LIMITS: LogLimits = LogLimits {
    segment_bytes: SEGMENT_BYTES,
    instance_bytes: INSTANCE_BYTES,
    run_segments: RUN_SEGMENTS,
    runs: MAX_RUNS,
};

#[derive(Clone)]
pub struct ManagedConsoleLog {
    inner: Arc<Sink>,
}

struct Sink {
    kind: SinkKind,
    failure: Mutex<Option<String>>,
}
enum SinkKind {
    Plain(Mutex<File>),
    Managed {
        directory: Arc<Directory>,
        run: String,
    },
}
struct Directory {
    state: Mutex<DirectoryState>,
}
struct DirectoryState {
    limits: LogLimits,
    store: Store,
    parts: Vec<(String, Part, u64)>,
    total: u64,
    active: HashMap<String, Weak<Sink>>,
    needs_refresh: bool,
}
#[derive(Default)]
struct Registry {
    sinks: HashMap<PathBuf, Weak<Sink>>,
    directories: HashMap<PathBuf, Weak<Directory>>,
}
static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();

/// Handles stay readable if retention later removes their names.
pub struct ManagedLogSegment {
    pub path: PathBuf,
    pub file: File,
    pub start_offset: u64,
}

fn registry() -> &'static Mutex<Registry> {
    REGISTRY.get_or_init(|| Mutex::new(Registry::default()))
}
fn poisoned() -> io::Error {
    io::Error::other("managed console log lock poisoned")
}

fn managed_path(path: &Path) -> bool {
    path.parent()
        .and_then(Path::file_name)
        .is_some_and(|name| name == "managed-console")
}

fn canonical_key(path: &Path, create: bool) -> io::Result<PathBuf> {
    reject_links(path)?;
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("console log directory missing"))?;
    if create {
        fs::create_dir_all(parent)?;
    }
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("console log name missing"))?;
    Ok(fs::canonicalize(parent)?.join(name))
}

fn directory(registry: &mut Registry, path: &Path) -> io::Result<Arc<Directory>> {
    if let Some(existing) = registry.directories.get(path).and_then(Weak::upgrade) {
        return Ok(existing);
    }
    let store = Store::open(path)?;
    let parts = store.lengths()?;
    let total = parts.iter().map(|(_, _, bytes)| bytes).sum();
    let directory = Arc::new(Directory {
        state: Mutex::new(DirectoryState {
            limits: LIMITS,
            store,
            parts,
            total,
            active: HashMap::new(),
            needs_refresh: false,
        }),
    });
    registry
        .directories
        .retain(|_, entry| entry.strong_count() > 0);
    registry
        .directories
        .insert(path.to_path_buf(), Arc::downgrade(&directory));
    Ok(directory)
}

impl ManagedConsoleLog {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = canonical_key(path.as_ref(), true)?;
        let mut registry = registry().lock().map_err(|_| poisoned())?;
        registry.sinks.retain(|_, entry| entry.strong_count() > 0);
        if let Some(inner) = registry.sinks.get(&path).and_then(Weak::upgrade) {
            return Ok(Self { inner });
        }
        let kind = if managed_path(&path) {
            let run = path
                .file_name()
                .and_then(|name| name.to_str())
                .filter(|name| {
                    name.starts_with("run-")
                        && name.ends_with(".log")
                        && name.bytes().all(|byte| {
                            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')
                        })
                })
                .ok_or_else(|| io::Error::other("invalid managed console run name"))?
                .to_owned();
            let directory = directory(
                &mut registry,
                path.parent()
                    .ok_or_else(|| io::Error::other("console directory missing"))?,
            )?;
            {
                let mut state = directory.state.lock().map_err(|_| poisoned())?;
                let registration = (|| {
                    state.refresh_if_needed()?;
                    if !state.store.ledger.runs.contains_key(&run) {
                        state.make_room(&run, 0, true, false)?;
                        state.store.create_run(&run)?;
                        state.refresh()?;
                    }
                    Ok::<_, io::Error>(())
                })();
                if registration.is_err() {
                    state.needs_refresh = true;
                }
                registration?;
            }
            SinkKind::Managed { directory, run }
        } else {
            SinkKind::Plain(Mutex::new(
                OpenOptions::new().create(true).append(true).open(&path)?,
            ))
        };
        let inner = Arc::new(Sink {
            kind,
            failure: Mutex::new(None),
        });
        if let SinkKind::Managed { directory, run } = &inner.kind {
            directory
                .state
                .lock()
                .map_err(|_| poisoned())?
                .active
                .insert(run.clone(), Arc::downgrade(&inner));
        }
        registry.sinks.insert(path, Arc::downgrade(&inner));
        Ok(Self { inner })
    }

    pub fn write_all(&self, bytes: &[u8]) -> io::Result<()> {
        let mut failure = self.inner.failure.lock().map_err(|_| poisoned())?;
        if let Some(error) = failure.as_ref() {
            return Err(io::Error::other(error.clone()));
        }
        let result = match &self.inner.kind {
            SinkKind::Plain(file) => file
                .lock()
                .map_err(|_| poisoned())
                .and_then(|mut file| file.write_all(bytes)),
            SinkKind::Managed { directory, run } => {
                let mut state = directory.state.lock().map_err(|_| poisoned())?;
                let result = state.write(run, bytes);
                if result.is_err() {
                    state.needs_refresh = true;
                }
                result
            }
        };
        if let Err(error) = &result {
            *failure = Some(error.to_string());
        }
        result
    }

    pub fn failure_summary(&self) -> Option<String> {
        self.inner
            .failure
            .lock()
            .map(|failure| {
                failure
                    .as_ref()
                    .map(|error| format!("Console log recording stopped: {error}"))
            })
            .unwrap_or_else(|_| Some(poisoned().to_string()))
    }
}

impl Write for ManagedConsoleLog {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        ManagedConsoleLog::write_all(self, bytes)?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl DirectoryState {
    fn refresh(&mut self) -> io::Result<()> {
        self.parts = self.store.lengths()?;
        self.total = self.parts.iter().map(|(_, _, bytes)| bytes).sum();
        self.needs_refresh = false;
        Ok(())
    }
    fn refresh_if_needed(&mut self) -> io::Result<()> {
        if self.needs_refresh {
            self.store.recover()?;
            self.refresh()?;
        }
        Ok(())
    }
    fn make_room(&mut self, run: &str, added: u64, new_run: bool, rotate: bool) -> io::Result<()> {
        self.active.retain(|_, sink| sink.strong_count() > 0);
        loop {
            let run_full = rotate
                && self
                    .store
                    .ledger
                    .runs
                    .get(run)
                    .is_some_and(|parts| parts.len() >= self.limits.run_segments);
            let directory_full = self.total.saturating_add(added) > self.limits.instance_bytes;
            let run_limit = new_run && self.store.ledger.runs.len() >= self.limits.runs;
            if !run_full && !directory_full && !run_limit {
                return Ok(());
            }
            let candidate = self
                .parts
                .iter()
                .filter(|(owner, part, _)| {
                    let is_current = &part.file == owner;
                    let is_active = owner == run
                        || self
                            .active
                            .get(owner)
                            .is_some_and(|sink| sink.strong_count() > 0);
                    (!is_current || !is_active) && (!run_full || owner == run)
                })
                .min_by_key(|(_, part, _)| part.sequence)
                .cloned();
            let Some((owner, part, bytes)) = candidate else {
                return Err(io::Error::other(
                    "console log budget exhausted; all retained segments are active",
                ));
            };
            self.store.remove_part(&owner, &part)?;
            self.parts
                .retain(|(_, entry, _)| entry.sequence != part.sequence);
            self.total = self.total.saturating_sub(bytes);
        }
    }
    fn write(&mut self, run: &str, mut bytes: &[u8]) -> io::Result<()> {
        self.refresh_if_needed()?;
        while !bytes.is_empty() {
            let part = self
                .store
                .ledger
                .runs
                .get(run)
                .and_then(|parts| parts.last())
                .cloned()
                .ok_or_else(|| io::Error::other("console run is not registered"))?;
            let cached = self
                .parts
                .iter()
                .find(|(_, entry, _)| entry.sequence == part.sequence)
                .map(|(_, _, bytes)| *bytes)
                .ok_or_else(|| io::Error::other("console segment size missing"))?;
            if cached >= self.limits.segment_bytes {
                self.make_room(run, 0, false, true)?;
                self.store.rotate(run)?;
                self.refresh()?;
                continue;
            }
            let count = bytes
                .len()
                .min((self.limits.segment_bytes - cached) as usize);
            self.make_room(run, count as u64, false, false)?;
            // Completed segment lengths are cached. Normal output chunks neither
            // scan the directory nor rewrite the ownership ledger.
            let mut file = self.store.append_part(&part)?;
            if file.metadata()?.len() != cached {
                return Err(io::Error::other(
                    "active console log changed outside its writer",
                ));
            }
            file.write_all(&bytes[..count])?;
            if let Some((_, _, size)) = self
                .parts
                .iter_mut()
                .find(|(_, entry, _)| entry.sequence == part.sequence)
            {
                *size += count as u64;
            }
            self.total += count as u64;
            bytes = &bytes[count..];
        }
        Ok(())
    }
}

pub fn failure_summary_for_path(path: &Path) -> Option<String> {
    let key = canonical_key(path, false).ok()?;
    let inner = registry().lock().ok()?.sinks.get(&key)?.upgrade()?;
    ManagedConsoleLog { inner }.failure_summary()
}

pub fn open_log_segments(path: &Path) -> io::Result<Option<Vec<ManagedLogSegment>>> {
    if !managed_path(path) {
        return Ok(None);
    }
    let key = canonical_key(path, false)?;
    if !key
        .parent()
        .is_some_and(|parent| parent.join("ownership.json").is_file())
    {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "managed console ownership record is unavailable",
        ));
    }
    let mut registry = registry().lock().map_err(|_| poisoned())?;
    let directory = directory(
        &mut registry,
        key.parent()
            .ok_or_else(|| io::Error::other("console directory missing"))?,
    )?;
    let mut state = directory.state.lock().map_err(|_| poisoned())?;
    state.refresh_if_needed()?;
    let run = key
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| io::Error::other("console name missing"))?;
    let parts = state.store.ledger.runs.get(run).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "console log expired under the retention policy or is not registered",
        )
    })?;
    let mut result = Vec::with_capacity(parts.len());
    for part in parts {
        result.push(ManagedLogSegment {
            path: state.store.path(&part.file)?,
            file: state.store.open_part(part)?,
            start_offset: part.start,
        });
    }
    Ok(Some(result))
}

#[cfg(test)]
#[path = "managed_console_log_tests.rs"]
mod tests;
