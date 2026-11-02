//! Recover only the fixed removal journal, retaining changed or unknown files.
use super::*;

pub(super) fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())
}

fn load(
    directory: &Path,
    profile: &Profile,
    wanted: &BTreeMap<String, String>,
) -> Result<(Journal, Vec<u8>), String> {
    let bytes = files::read(directory, JOURNAL)?.ok_or("ARK removal journal is missing")?;
    if bytes.len() > 64 * 1024 {
        return Err("ARK removal journal is oversized".into());
    }
    let journal: Journal = serde_json::from_slice(&bytes)
        .map_err(|_| "ARK removal journal is invalid; its files were retained")?;
    let owned = files::decode_ownership(&journal.owner, profile, wanted)?;
    let expected: Vec<_> = owned
        .files
        .keys()
        .filter(|path| selected(path, journal.framework_retained))
        .collect();
    if journal.schema != 1 || journal.before.keys().ne(expected) {
        return Err("ARK removal journal has an unexpected file set".into());
    }
    for (path, before) in &journal.before {
        if before
            .as_ref()
            .is_some_and(|hash| owned.files.get(path) != Some(hash))
        {
            return Err(format!("ARK removal journal has an unowned backup: {path}"));
        }
    }
    Ok((journal, bytes))
}

pub(super) fn entries(journal: &Journal, bytes: &[u8]) -> BTreeMap<String, String> {
    let mut expected = BTreeMap::from([(JOURNAL.into(), digest(bytes))]);
    for (index, hash) in journal.before.values().enumerate() {
        if let Some(hash) = hash {
            expected.insert(format!("{index}.old"), hash.clone());
        }
    }
    expected
}

pub(super) fn verify_entries(
    directory: &Path,
    expected: &BTreeMap<String, String>,
    complete: bool,
) -> Result<(), String> {
    let mut count = 0;
    for entry in fs::read_dir(directory).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "Unexpected ARK removal entry")?;
        let hash = expected
            .get(&name)
            .ok_or("Unexpected ARK removal entry; all files were retained")?;
        let file =
            files::lock_regular(&entry.path(), false)?.ok_or("ARK removal entry disappeared")?;
        if digest(&file.bytes) != *hash {
            return Err(format!("Changed ARK removal entry was retained: {name}"));
        }
        count += 1;
    }
    if complete && count != expected.len() {
        return Err("ARK removal backups are incomplete; all files were retained".into());
    }
    Ok(())
}

pub(super) fn verify_runtime(
    bin: &Path,
    journal: &Journal,
    committed: bool,
    mixed: bool,
) -> Result<(), String> {
    if files::read(bin, OWNER)?.as_deref() != Some(journal.owner.as_slice()) {
        return Err("ARK removal preserved a changed ownership record".into());
    }
    for (path, before) in &journal.before {
        let actual = files::read(bin, path)?.as_ref().map(|bytes| digest(bytes));
        let expected = if committed { None } else { before.clone() };
        if actual != expected && !(mixed && actual.is_none()) {
            return Err(format!(
                "ARK removal recovery preserved an externally modified file: {path}"
            ));
        }
    }
    Ok(())
}

fn cleanup(
    bin: &Path,
    name: &str,
    profile: &Profile,
    wanted: &BTreeMap<String, String>,
) -> Result<(), String> {
    let directory = bin.join(name);
    let pin = files::pin(&directory)?;
    if fs::read_dir(&directory)
        .map_err(|e| e.to_string())?
        .next()
        .is_none()
    {
        drop(pin);
        return fs::remove_dir(directory).map_err(|e| e.to_string());
    }
    let (journal, bytes) = load(&directory, profile, wanted)?;
    let expected = entries(&journal, &bytes);
    verify_entries(&directory, &expected, false)?;
    verify_runtime(bin, &journal, name == COMMITTED, false)?;
    for (name, hash) in &expected {
        if name != JOURNAL {
            files::remove_hash(&directory.join(name), hash)?;
        }
    }
    // The journal is last so an interrupted cleanup can validate every backup
    // that still exists. Runtime directories and unknown files stay untouched.
    files::remove_hash(&directory.join(JOURNAL), &digest(&bytes))?;
    drop(pin);
    fs::remove_dir(directory).map_err(|e| e.to_string())
}

pub(super) fn recover(
    bin: &Path,
    profile: &Profile,
    wanted: &BTreeMap<String, String>,
) -> Result<(), String> {
    let states = states(bin)?;
    if states.len() > 1 {
        return Err("Conflicting ARK removal states were retained".into());
    }
    let Some(name) = states.first().copied() else {
        return Ok(());
    };
    if matches!(name, COMMITTED | ROLLED_BACK) {
        return cleanup(bin, name, profile, wanted);
    }
    let directory = bin.join(name);
    let pin = files::pin(&directory)?;
    let (journal, bytes) = load(&directory, profile, wanted)?;
    verify_entries(&directory, &entries(&journal, &bytes), true)?;
    verify_runtime(bin, &journal, false, true)?;

    let owner = files::lock_regular(&bin.join(OWNER), false)?
        .ok_or("ARK removal ownership record disappeared")?;
    if owner.bytes != journal.owner {
        return Err("ARK removal preserved a changed ownership record".into());
    }
    let mut pins = Vec::new();
    let mut originals = Vec::new();
    let mut current_files = Vec::new();
    // Validate and hold all extant originals and destinations before restoring
    // the first missing file. A conflicting replacement is never overwritten.
    for (index, (path, hash)) in journal.before.iter().enumerate() {
        let parent_exists = files::pin_parent(bin, path, &mut pins)?;
        let current = if parent_exists {
            files::lock_regular(&bin.join(path), false)?
        } else {
            None
        };
        if current
            .as_ref()
            .is_some_and(|file| Some(digest(&file.bytes)) != *hash)
        {
            return Err(format!(
                "ARK removal recovery preserved a changed file: {path}"
            ));
        }
        if let Some(hash) = hash {
            if !parent_exists {
                return Err(format!(
                    "ARK removal recovery retained a missing parent: {path}"
                ));
            }
            let backup = directory.join(format!("{index}.old"));
            let original =
                files::lock_regular(&backup, false)?.ok_or("ARK removal backup disappeared")?;
            if digest(&original.bytes) != *hash {
                return Err("ARK removal backup changed; its files were retained".into());
            }
            if current.is_none() {
                originals.push((backup, bin.join(path), original));
            }
        }
        current_files.push(current);
    }
    for (backup, path, _original) in &originals {
        fs::hard_link(backup, path)
            .map_err(|e| format!("ARK removal could not restore {}: {e}", path.display()))?;
    }
    verify_runtime(bin, &journal, false, false)?;
    drop(current_files);
    drop(originals);
    drop(owner);
    drop(pins);
    drop(pin);
    fs::rename(directory, bin.join(ROLLED_BACK)).map_err(|e| e.to_string())?;
    cleanup(bin, ROLLED_BACK, profile, wanted)
}
