use super::owned_fs::{
    FileIdentity, identity, move_owned, optional_identity, reject_links, remove_owned,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};

#[path = "managed_console_log_validation.rs"]
mod validation;

const MANIFEST: &str = "ownership.json";
const MANIFEST_LIMIT: u64 = 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Part {
    pub file: String,
    pub identity: FileIdentity,
    pub start: u64,
    pub sequence: u64,
}

#[derive(Clone, Serialize, Deserialize)]
struct FileMove {
    from: String,
    to: String,
    identity: FileIdentity,
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Ledger {
    format: String,
    owner: String,
    next_sequence: u64,
    pub runs: BTreeMap<String, Vec<Part>>,
    moves: Vec<FileMove>,
    deletions: Vec<Part>,
}

pub(super) struct Store {
    pub directory: PathBuf,
    pub ledger: Ledger,
    manifest_identity: FileIdentity,
}

impl Store {
    pub fn open(directory: &Path) -> io::Result<Self> {
        reject_links(directory)?;
        fs::create_dir_all(directory)?;
        let manifest = directory.join(MANIFEST);
        reject_links(&manifest)?;
        if !manifest.try_exists()? {
            let ledger = Ledger {
                format: String::from("langame-managed-console"),
                owner: uuid::Uuid::new_v4().to_string(),
                next_sequence: 1,
                runs: BTreeMap::new(),
                moves: Vec::new(),
                deletions: Vec::new(),
            };
            publish_initial_manifest(&manifest, &serde_json::to_vec(&ledger)?)?;
        }
        let file = File::open(&manifest)?;
        let manifest_identity = identity(&file)?;
        let mut bytes = Vec::new();
        file.take(MANIFEST_LIMIT + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MANIFEST_LIMIT {
            return Err(io::Error::other(
                "console ownership ledger exceeds its limit",
            ));
        }
        let ledger: Ledger = serde_json::from_slice(&bytes)?;
        if ledger.format != "langame-managed-console"
            || uuid::Uuid::parse_str(&ledger.owner).is_err()
            || ledger.runs.len() > 128
        {
            return Err(io::Error::other("invalid console ownership ledger"));
        }
        validation::validate(&ledger, &manifest_identity)?;
        let mut store = Self {
            directory: directory.to_path_buf(),
            ledger,
            manifest_identity,
        };
        store.recover()?;
        Ok(store)
    }

    pub fn path(&self, relative: &str) -> io::Result<PathBuf> {
        let path = Path::new(relative);
        if path.components().count() > 2
            || !path
                .components()
                .all(|part| matches!(part, Component::Normal(_)))
            || path.as_os_str().is_empty()
        {
            return Err(io::Error::other("invalid owned console segment path"));
        }
        let full = self.directory.join(path);
        reject_links(&full)?;
        Ok(full)
    }

    pub fn open_part(&self, part: &Part) -> io::Result<File> {
        let file = File::open(self.path(&part.file)?)?;
        if !file.metadata()?.is_file() || identity(&file)? != part.identity {
            return Err(io::Error::other("owned console segment was replaced"));
        }
        Ok(file)
    }

    pub fn append_part(&self, part: &Part) -> io::Result<File> {
        let file = OpenOptions::new()
            .append(true)
            .open(self.path(&part.file)?)?;
        if !file.metadata()?.is_file() || identity(&file)? != part.identity {
            return Err(io::Error::other("owned console segment was replaced"));
        }
        Ok(file)
    }

    pub fn lengths(&self) -> io::Result<Vec<(String, Part, u64)>> {
        let mut result = Vec::new();
        for (run, parts) in &self.ledger.runs {
            for part in parts {
                result.push((
                    run.clone(),
                    part.clone(),
                    self.open_part(part)?.metadata()?.len(),
                ));
            }
        }
        Ok(result)
    }

    fn save(&mut self, ledger: Ledger) -> io::Result<()> {
        let manifest = self.path(MANIFEST)?;
        if identity(&File::open(&manifest)?)? != self.manifest_identity {
            return Err(io::Error::other("console ownership ledger was replaced"));
        }
        let bytes = serde_json::to_vec(&ledger)?;
        if bytes.len() as u64 > MANIFEST_LIMIT {
            return Err(io::Error::other(
                "console ownership ledger exceeds its limit",
            ));
        }
        crate::atomic_file::write_file_atomically_with_fixed_sibling(
            &manifest,
            &bytes,
            &self.path("ownership.update")?,
        )?;
        self.ledger = ledger;
        self.manifest_identity = identity(&File::open(&manifest)?)?;
        Ok(())
    }

    pub fn recover(&mut self) -> io::Result<()> {
        if self.ledger.moves.is_empty() && self.ledger.deletions.is_empty() {
            return Ok(());
        }
        for movement in &self.ledger.moves {
            let from = self.path(&movement.from)?;
            let to = self.path(&movement.to)?;
            let from_id = optional_identity(&from)?;
            let to_id = optional_identity(&to)?;
            match (from_id, to_id) {
                (Some(source), None) if source == movement.identity => {
                    move_owned(&from, &to, &movement.identity)?;
                }
                (Some(source), Some(target))
                    if source == movement.identity && target == movement.identity =>
                {
                    remove_owned(&from, &movement.identity)?;
                }
                (None, Some(target)) if target == movement.identity => {}
                (Some(source), Some(target))
                    if target == movement.identity
                        && self
                            .ledger
                            .moves
                            .iter()
                            .any(|later| later.to == movement.from && later.identity == source) => {
                }
                _ => {
                    return Err(io::Error::other(
                        "console rotation encountered an unknown or missing file",
                    ));
                }
            }
        }
        for part in &self.ledger.deletions {
            let path = self.path(&part.file)?;
            remove_owned(&path, &part.identity)?;
            if let Some(parent) = path.parent().filter(|parent| *parent != self.directory) {
                match fs::remove_dir(parent) {
                    Ok(()) => {}
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::DirectoryNotEmpty | io::ErrorKind::NotFound
                        ) => {}
                    Err(error) => return Err(error),
                }
            }
        }
        let mut completed = self.ledger.clone();
        completed.moves.clear();
        completed.deletions.clear();
        self.save(completed)
    }

    pub fn create_run(&mut self, name: &str) -> io::Result<()> {
        if self.ledger.runs.contains_key(name) {
            return Ok(());
        }
        if self.path(name)?.try_exists()? {
            return Err(io::Error::other(
                "console log already exists without an ownership record",
            ));
        }
        let (temporary, file_identity) = self.new_temporary()?;
        let mut next = self.ledger.clone();
        let sequence = next.next_sequence;
        next.next_sequence = sequence
            .checked_add(1)
            .ok_or_else(|| io::Error::other("console generation overflow"))?;
        next.runs.insert(
            name.to_owned(),
            vec![Part {
                file: name.to_owned(),
                identity: file_identity.clone(),
                start: 0,
                sequence,
            }],
        );
        next.moves.push(FileMove {
            from: temporary.clone(),
            to: name.to_owned(),
            identity: file_identity.clone(),
        });
        if let Err(error) = self.save(next) {
            self.rollback_unpublished(&temporary, &file_identity);
            return Err(error);
        }
        self.recover()
    }

    pub fn rotate(&mut self, name: &str) -> io::Result<()> {
        let current = self
            .ledger
            .runs
            .get(name)
            .and_then(|parts| parts.last())
            .cloned()
            .ok_or_else(|| io::Error::other("console run is not registered"))?;
        let size = self.open_part(&current)?.metadata()?.len();
        let archive_name = format!("segment-{:020}", self.ledger.next_sequence);
        let archive = self.path(&archive_name)?;
        fs::create_dir(&archive)?;
        let archive_file = format!("{archive_name}/entries.log");
        let (temporary, file_identity) = match self.new_temporary() {
            Ok(created) => created,
            Err(error) => {
                let _ = fs::remove_dir(&archive);
                return Err(error);
            }
        };
        let mut next = self.ledger.clone();
        let sequence = next.next_sequence;
        next.next_sequence = sequence
            .checked_add(1)
            .ok_or_else(|| io::Error::other("console generation overflow"))?;
        let parts = next
            .runs
            .get_mut(name)
            .ok_or_else(|| io::Error::other("console run disappeared"))?;
        parts
            .last_mut()
            .ok_or_else(|| io::Error::other("console segment disappeared"))?
            .file = archive_file.clone();
        parts.push(Part {
            file: name.to_owned(),
            identity: file_identity.clone(),
            start: current.start.saturating_add(size),
            sequence,
        });
        next.moves.push(FileMove {
            from: name.to_owned(),
            to: archive_file.clone(),
            identity: current.identity,
        });
        next.moves.push(FileMove {
            from: temporary.clone(),
            to: name.to_owned(),
            identity: file_identity.clone(),
        });
        if let Err(error) = self.save(next) {
            self.rollback_unpublished(&temporary, &file_identity);
            if !self
                .ledger
                .moves
                .iter()
                .any(|movement| movement.to == archive_file)
            {
                let _ = fs::remove_dir(&archive);
            }
            return Err(error);
        }
        self.recover()
    }

    fn new_temporary(&self) -> io::Result<(String, FileIdentity)> {
        let name = format!("pending-{:020}.log", self.ledger.next_sequence);
        let file = File::create_new(self.path(&name)?)?;
        Ok((name, identity(&file)?))
    }

    fn rollback_unpublished(&self, relative: &str, expected: &FileIdentity) {
        if self
            .ledger
            .moves
            .iter()
            .any(|movement| movement.from == relative)
        {
            return;
        }
        let Ok(manifest) = self.path(MANIFEST).and_then(File::open) else {
            return;
        };
        if identity(&manifest).ok().as_ref() != Some(&self.manifest_identity) {
            return;
        }
        if let Ok(path) = self.path(relative)
            && optional_identity(&path).ok().flatten().as_ref() == Some(expected)
        {
            let _ = remove_owned(&path, expected);
        }
    }

    pub fn remove_part(&mut self, run: &str, part: &Part) -> io::Result<()> {
        let mut next = self.ledger.clone();
        let parts = next
            .runs
            .get_mut(run)
            .ok_or_else(|| io::Error::other("console run disappeared"))?;
        parts.retain(|entry| entry.sequence != part.sequence);
        if parts.is_empty() {
            next.runs.remove(run);
        }
        next.deletions.push(part.clone());
        self.save(next)?;
        self.recover()
    }
}

fn publish_initial_manifest(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let temporary = path.with_file_name("ownership.pending");
    let mut file = File::create_new(&temporary)?;
    let file_identity = identity(&file)?;
    let result = file.write_all(bytes).and_then(|()| file.sync_all());
    drop(file);
    let result = result.and_then(|()| move_owned(&temporary, path, &file_identity));
    if result.is_err() {
        let _ = remove_owned(&temporary, &file_identity);
    }
    result
}

#[cfg(test)]
#[path = "managed_console_log_store_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "managed_console_log_reliability_tests.rs"]
mod reliability_tests;
