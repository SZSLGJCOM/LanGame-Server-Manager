//! A bounded journal for this one pinned ARK installation, not a general
//! updater. Runtime mutation starts only after a complete staged directory is
//! atomically renamed to PENDING. Its backups survive until rollback or commit.
use super::*;

const STAGING: &str = ".langame-ark-tools-staging";
const PENDING: &str = ".langame-ark-tools-pending";
const COMMITTED: &str = ".langame-ark-tools-committed";
const ROLLED_BACK: &str = ".langame-ark-tools-rolled-back";
const JOURNAL: &str = "journal.json";
const MAX_JOURNAL: usize = 64 * 1024;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    schema: u32,
    module_id: String,
    old_owner: Option<Vec<u8>>,
    new_owner: Vec<u8>,
    before: BTreeMap<String, Option<String>>,
    created_directories: Vec<String>,
}

struct ValidatedJournal {
    journal: Journal,
    serialized_bytes: Vec<u8>,
    expected_hashes: BTreeMap<String, String>,
}

pub(super) fn pending(bin: &Path) -> Result<bool, String> {
    for name in [STAGING, PENDING, COMMITTED, ROLLED_BACK] {
        match fs::symlink_metadata(bin.join(name)) {
            Ok(_) => return Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("Cannot inspect ARK preparation state: {e}")),
        }
    }
    Ok(false)
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())
}

fn order(hashes: &BTreeMap<String, String>) -> Vec<String> {
    let mut paths: Vec<_> = hashes
        .keys()
        .filter(|p| *p != "version.dll")
        .cloned()
        .collect();
    paths.push("version.dll".into()); // Loader publication is always last.
    paths
}

fn parents(paths: impl Iterator<Item = String>) -> std::collections::BTreeSet<String> {
    let mut result = std::collections::BTreeSet::new();
    for path in paths {
        let parts: Vec<_> = path.split('/').collect();
        for count in 1..parts.len() {
            result.insert(parts[..count].join("/"));
        }
    }
    result
}

fn validate(
    journal: &Journal,
    profile: &Profile,
    wanted: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, String> {
    if journal.schema != 1 || journal.module_id != profile.module_id {
        return Err("ARK preparation journal belongs to a different profile".into());
    }
    let new = files::decode_ownership(&journal.new_owner, profile, wanted)?;
    let old = journal
        .old_owner
        .as_ref()
        .map(|bytes| files::decode_ownership(bytes, profile, wanted))
        .transpose()?;
    let mut after = new.files;
    after.insert(OWNER.into(), digest(&journal.new_owner));
    if journal.before.keys().ne(after.keys()) {
        return Err("ARK preparation journal has an unexpected file set".into());
    }
    for (path, hash) in &journal.before {
        let owned = if path == OWNER {
            journal.old_owner.as_ref().map(|bytes| digest(bytes))
        } else {
            old.as_ref()
                .and_then(|owner| owner.files.get(path))
                .cloned()
        };
        if (path == OWNER && hash != &owned)
            || hash.as_ref().is_some_and(|h| owned.as_ref() != Some(h))
        {
            return Err(format!(
                "ARK preparation journal has an unowned backup: {path}"
            ));
        }
    }
    let allowed = parents(after.keys().cloned());
    let mut previous: Option<&str> = None;
    for directory in &journal.created_directories {
        if !allowed.contains(directory) || previous.is_some_and(|p| p >= directory.as_str()) {
            return Err("ARK preparation journal contains an unexpected directory".into());
        }
        previous = Some(directory);
    }
    Ok(after)
}

fn load(
    directory: &Path,
    profile: &Profile,
    wanted: &BTreeMap<String, String>,
) -> Result<ValidatedJournal, String> {
    let bytes = files::read(directory, JOURNAL)?.ok_or("ARK preparation journal is missing")?;
    if bytes.len() > MAX_JOURNAL {
        return Err("ARK preparation journal is oversized".into());
    }
    let journal: Journal = serde_json::from_slice(&bytes)
        .map_err(|_| "ARK preparation journal is invalid; its files were retained")?;
    let expected_hashes = validate(&journal, profile, wanted)?;
    Ok(ValidatedJournal {
        journal,
        serialized_bytes: bytes,
        expected_hashes,
    })
}

fn transaction_files(
    journal: &Journal,
    after: &BTreeMap<String, String>,
    bytes: &[u8],
) -> BTreeMap<String, String> {
    let mut result = BTreeMap::from([(JOURNAL.into(), digest(bytes))]);
    for (index, path) in order(after).iter().enumerate() {
        result.insert(format!("{index}.new"), after[path].clone());
        if let Some(hash) = &journal.before[path] {
            result.insert(format!("{index}.old"), hash.clone());
        }
    }
    result
}

fn verify_entries(
    directory: &Path,
    expected: &BTreeMap<String, String>,
    complete: bool,
) -> Result<(), String> {
    let mut found = std::collections::BTreeSet::new();
    for entry in fs::read_dir(directory).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "Unexpected ARK transaction entry")?;
        let hash = expected
            .get(&name)
            .ok_or("Unexpected ARK transaction entry; all files were retained")?;
        let file = files::lock_regular(&entry.path(), false)?
            .ok_or("ARK transaction entry disappeared")?;
        if digest(&file.bytes) != *hash {
            return Err(format!(
                "Changed ARK transaction entry was retained: {name}"
            ));
        }
        found.insert(name);
    }
    if complete && found.len() != expected.len() {
        return Err("ARK preparation transaction is incomplete; its files were retained".into());
    }
    Ok(())
}

fn verify_runtime(
    bin: &Path,
    journal: &Journal,
    after: &BTreeMap<String, String>,
    committed: bool,
    mixed: bool,
) -> Result<(), String> {
    for path in after.keys() {
        let hash = files::read(bin, path)?.as_ref().map(|bytes| digest(bytes));
        let expected = if committed {
            Some(after[path].clone())
        } else {
            journal.before[path].clone()
        };
        if hash != expected && !(mixed && (hash.is_none() || hash.as_ref() == Some(&after[path]))) {
            return Err(format!(
                "ARK recovery preserved an externally modified file: {path}"
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
        return fs::remove_dir(&directory).map_err(|e| e.to_string());
    }
    let ValidatedJournal {
        journal,
        serialized_bytes: bytes,
        expected_hashes: after,
    } = load(&directory, profile, wanted)?;
    let expected = transaction_files(&journal, &after, &bytes);
    verify_entries(&directory, &expected, false)?;
    verify_runtime(bin, &journal, &after, name == COMMITTED, false)?;
    if name == ROLLED_BACK {
        let created: Vec<_> = journal
            .created_directories
            .iter()
            .map(|p| bin.join(p))
            .collect();
        files::remove_created_directories(&created)?;
    }
    for (entry, hash) in &expected {
        if entry != JOURNAL {
            files::remove_hash(&directory.join(entry), hash)?;
        }
    }
    // Keep the journal until every backup is gone. An empty cleanup directory
    // is the only journal-less state recovery is permitted to remove.
    files::remove_hash(&directory.join(JOURNAL), &digest(&bytes))?;
    drop(pin);
    fs::remove_dir(directory).map_err(|e| e.to_string())
}

pub(super) fn recover(
    bin: &Path,
    profile: &Profile,
    wanted: &BTreeMap<String, String>,
) -> Result<(), String> {
    let mut states = Vec::new();
    for name in [STAGING, PENDING, COMMITTED, ROLLED_BACK] {
        match fs::symlink_metadata(bin.join(name)) {
            Ok(_) => states.push(name),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    if states.len() > 1 {
        return Err("Conflicting ARK preparation states were retained".into());
    }
    let Some(name) = states.first().copied() else {
        return Ok(());
    };
    if matches!(name, COMMITTED | ROLLED_BACK) {
        return cleanup(bin, name, profile, wanted);
    }
    let directory = bin.join(name);
    let pin = files::pin(&directory)?;
    if name == STAGING
        && fs::read_dir(&directory)
            .map_err(|e| e.to_string())?
            .next()
            .is_none()
    {
        drop(pin);
        return fs::remove_dir(directory).map_err(|e| e.to_string());
    }
    let ValidatedJournal {
        journal,
        serialized_bytes: bytes,
        expected_hashes: after,
    } = load(&directory, profile, wanted)?;
    verify_entries(
        &directory,
        &transaction_files(&journal, &after, &bytes),
        name == PENDING,
    )?;
    verify_runtime(bin, &journal, &after, false, name == PENDING)?;
    if name == STAGING {
        // No runtime directories or files have been created in this phase.
        // Keep staging as its own cleanup state; planned parents are not owned.
        drop(pin);
        return cleanup(bin, STAGING, profile, wanted);
    }
    let mut pins = Vec::new();
    let mut created = Vec::new();
    if name == PENDING {
        for (index, path) in order(&after).iter().enumerate().rev() {
            let before = files::read(&directory, &format!("{index}.old"))?;
            let replacement = files::read(&directory, &format!("{index}.new"))?
                .ok_or("ARK replacement disappeared during recovery")?;
            if before.as_ref().map(|b| digest(b)) != journal.before[path]
                || digest(&replacement) != after[path]
            {
                return Err("ARK recovery backup changed; its files were retained".into());
            }
            files::prepare_parent(bin, path, &mut pins, &mut created)?;
            files::rollback_file(
                &bin.join(path),
                &directory.join(format!("{index}.old")),
                before.as_deref(),
                &replacement,
            )?;
        }
        verify_runtime(bin, &journal, &after, false, false)?;
    }
    drop(pins);
    drop(pin);
    fs::rename(&directory, bin.join(ROLLED_BACK)).map_err(|e| e.to_string())?;
    cleanup(bin, ROLLED_BACK, profile, wanted)
}

pub(super) fn publish(
    root: &Path,
    profile: &Profile,
    payload: &mut BTreeMap<String, Vec<u8>>,
    wanted: &BTreeMap<String, String>,
    before_publish: impl Fn(usize) -> Result<(), String>,
) -> Result<(), String> {
    let (bin, _root_pins) = files::runtime(root, profile.executable)?;
    recover(&bin, profile, wanted)?;
    let owned = files::ownership(&bin, profile, wanted)?;
    files::preflight(&bin, profile, wanted, owned.as_ref())?;
    if payload.keys().ne(wanted.keys())
        || payload
            .iter()
            .any(|(p, b)| b.len() > MAX_FILE || digest(b) != wanted[p])
    {
        return Err("ARK payload does not match the fixed installation file set".into());
    }
    let new_owner = serde_json::to_vec_pretty(&Ownership {
        schema: 1,
        module_id: profile.module_id.into(),
        files: wanted.clone(),
    })
    .map_err(|e| e.to_string())?;
    payload.insert(OWNER.into(), new_owner.clone());
    let before: BTreeMap<_, _> = payload
        .keys()
        .map(|path| Ok((path.clone(), files::read(&bin, path)?)))
        .collect::<Result<_, String>>()?;
    let mut journal = Journal {
        schema: 1,
        module_id: profile.module_id.into(),
        new_owner,
        old_owner: before[OWNER].clone(),
        before: before
            .iter()
            .map(|(p, b)| (p.clone(), b.as_ref().map(|b| digest(b))))
            .collect(),
        created_directories: Vec::new(),
    };
    for directory in parents(payload.keys().cloned()) {
        match fs::symlink_metadata(bin.join(&directory)) {
            Ok(_) => {
                files::pin(&bin.join(&directory))?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                journal.created_directories.push(directory)
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    let after = validate(&journal, profile, wanted)?;
    let bytes = serde_json::to_vec(&journal).map_err(|e| e.to_string())?;
    let staging = bin.join(STAGING);
    fs::create_dir(&staging).map_err(|e| e.to_string())?;
    let stage_pin = files::pin(&staging)?;
    write_new(&staging.join(JOURNAL), &bytes)?;
    for (index, path) in order(&after).iter().enumerate() {
        write_new(&staging.join(format!("{index}.new")), &payload[path])?;
        if let Some(original) = &before[path] {
            write_new(&staging.join(format!("{index}.old")), original)?;
        }
    }
    verify_entries(&staging, &transaction_files(&journal, &after, &bytes), true)?;
    verify_runtime(&bin, &journal, &after, false, false)?;
    drop(stage_pin);
    let transaction = bin.join(PENDING);
    fs::rename(&staging, &transaction).map_err(|e| e.to_string())?;
    let transaction_pin = files::pin(&transaction)?;
    verify_entries(
        &transaction,
        &transaction_files(&journal, &after, &bytes),
        true,
    )?;
    verify_runtime(&bin, &journal, &after, false, false)?;
    let mut pins = Vec::new();
    let mut directories = Vec::new();
    let result = (|| {
        for (index, path) in order(&after).iter().enumerate() {
            files::prepare_parent(&bin, path, &mut pins, &mut directories)?;
            let target = bin.join(path);
            let current = files::lock_regular(&target, true)?;
            if current.as_ref().map(|f| digest(&f.bytes)) != journal.before[path] {
                return Err(format!(
                    "ARK extension file changed during preparation: {path}"
                ));
            }
            if current.as_ref().is_some_and(|f| f.bytes == payload[path]) {
                continue;
            }
            let staged = files::lock_regular(&transaction.join(format!("{index}.new")), false)?
                .ok_or("ARK staged replacement disappeared")?;
            if digest(&staged.bytes) != after[path] {
                return Err("ARK staged replacement changed".into());
            }
            if let Some(current) = current {
                current.remove()?;
            }
            before_publish(index)?;
            fs::hard_link(transaction.join(format!("{index}.new")), &target)
                .map_err(|e| format!("Failed to publish {path}: {e}"))?;
        }
        verify_runtime(&bin, &journal, &after, true, false)
    })();
    drop(pins);
    drop(transaction_pin);
    if let Err(error) = result {
        return match recover(&bin, profile, wanted) {
            Ok(()) => Err(error),
            Err(recovery) => Err(format!(
                "{error}; ARK transaction retained for recovery: {recovery}"
            )),
        };
    }
    fs::rename(transaction, bin.join(COMMITTED)).map_err(|e| e.to_string())?;
    cleanup(&bin, COMMITTED, profile, wanted)
}

#[cfg(test)]
#[path = "ark_tools_install_transaction_tests.rs"]
mod tests;
