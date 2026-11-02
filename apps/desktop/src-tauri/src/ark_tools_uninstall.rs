//! Removal is limited to the existing ownership record. Shared loaders remain
//! when another plugin is present, and the record remains for a later install.
use super::*;

#[path = "ark_tools_uninstall_recovery.rs"]
mod recovery;

const STAGING: &str = ".langame-ark-tools-uninstall-staging";
const PENDING: &str = ".langame-ark-tools-uninstall-pending";
const COMMITTED: &str = ".langame-ark-tools-uninstall-committed";
const ROLLED_BACK: &str = ".langame-ark-tools-uninstall-rolled-back";
const JOURNAL: &str = "journal.json";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    schema: u32,
    owner: Vec<u8>,
    framework_retained: bool,
    before: BTreeMap<String, Option<String>>,
}

pub(super) fn pending(bin: &Path) -> Result<bool, String> {
    Ok(!states(bin)?.is_empty())
}

fn states(bin: &Path) -> Result<Vec<&'static str>, String> {
    let mut found = Vec::new();
    for name in [PENDING, COMMITTED, ROLLED_BACK] {
        match fs::symlink_metadata(bin.join(name)) {
            Ok(_) => found.push(name),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("Cannot inspect ARK removal state: {e}")),
        }
    }
    Ok(found)
}

pub(super) fn available(bin: &Path, owned: &Ownership) -> Result<bool, String> {
    for path in [PLUGIN, INFO, NOTICES] {
        if owned.files.contains_key(path) && files::read(bin, path)?.is_some() {
            return Ok(true);
        }
    }
    Ok(false)
}

fn selected(path: &str, framework_retained: bool) -> bool {
    !framework_retained || matches!(path, PLUGIN | INFO | NOTICES)
}

fn other_plugins(bin: &Path, pins: &mut Vec<File>) -> Result<bool, String> {
    // Pin the directory chain before listing it, including an otherwise empty
    // Plugins directory. Never traverse an unknown plugin or remove its files.
    if !files::pin_parent(bin, "ArkApi/Plugins/entry", pins)? {
        return Ok(false);
    }
    for entry in fs::read_dir(bin.join("ArkApi/Plugins")).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if !entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.eq_ignore_ascii_case("LgsmArkTools"))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) fn recover_all(
    bin: &Path,
    profile: &Profile,
    wanted: &BTreeMap<String, String>,
) -> Result<(), String> {
    if pending(bin)? && transaction::pending(bin)? {
        return Err("Conflicting ARK preparation and removal states were retained".into());
    }
    recovery::recover(bin, profile, wanted)?;
    transaction::recover(bin, profile, wanted)
}

pub(super) fn remove(
    root: &Path,
    profile: &Profile,
    wanted: &BTreeMap<String, String>,
) -> Result<bool, String> {
    remove_with(root, profile, wanted, |_| Ok(()))
}

fn remove_with(
    root: &Path,
    profile: &Profile,
    wanted: &BTreeMap<String, String>,
    after_remove: impl Fn(usize) -> Result<(), String>,
) -> Result<bool, String> {
    remove_with_writes(root, profile, wanted, after_remove, recovery::write_new)
}

fn remove_with_writes(
    root: &Path,
    profile: &Profile,
    wanted: &BTreeMap<String, String>,
    after_remove: impl Fn(usize) -> Result<(), String>,
    write_new: impl Fn(&Path, &[u8]) -> Result<(), String>,
) -> Result<bool, String> {
    let (bin, _root_pins) = files::runtime(root, profile.executable)?;
    recover_all(&bin, profile, wanted)?;
    let owner = files::lock_regular(&bin.join(OWNER), false)?
        .ok_or("No managed ARK server extension is installed")?;
    let owned = files::decode_ownership(&owner.bytes, profile, wanted)?;
    let mut pins = Vec::new();
    let framework_retained = other_plugins(&bin, &mut pins)?;
    let mut locked = BTreeMap::new();
    let mut before = BTreeMap::new();
    for (path, hash) in &owned.files {
        if !selected(path, framework_retained) {
            continue;
        }
        let file = if files::pin_parent(&bin, path, &mut pins)? {
            files::lock_regular(&bin.join(path), true)?
        } else {
            None
        };
        if file
            .as_ref()
            .is_some_and(|file| digest(&file.bytes) != *hash)
        {
            return Err(format!(
                "ARK removal will not delete an unknown or modified file: {path}"
            ));
        }
        before.insert(path.clone(), file.as_ref().map(|_| hash.clone()));
        locked.insert(path.clone(), file);
    }
    // Holding every existing target open prevents a late conflict from causing
    // partial deletion. Missing targets are also checked before mutation.
    let journal = Journal {
        schema: 1,
        owner: owner.bytes.clone(),
        framework_retained,
        before,
    };
    let bytes = serde_json::to_vec(&journal).map_err(|e| e.to_string())?;
    // Preparation has not changed the runtime. A crash can leave truncated
    // files here, so each attempt owns a fresh directory that is never treated
    // as a pending transaction or blindly cleaned by a later attempt.
    let staging = bin.join(format!("{STAGING}-{}", uuid::Uuid::new_v4().simple()));
    fs::create_dir(&staging).map_err(|e| e.to_string())?;
    let expected = recovery::entries(&journal, &bytes);
    let directory = bin.join(PENDING);
    (|| {
        let stage_pin = files::pin(&staging)?;
        write_new(&staging.join(JOURNAL), &bytes)?;
        for (index, (_, file)) in locked.iter().enumerate() {
            if let Some(file) = file {
                write_new(&staging.join(format!("{index}.old")), &file.bytes)?;
            }
        }
        recovery::verify_entries(&staging, &expected, true)?;
        for (path, file) in &locked {
            if file.is_none() && files::read(&bin, path)?.is_some() {
                return Err(format!(
                    "ARK removal preserved a newly created file: {path}"
                ));
            }
        }
        if !framework_retained && other_plugins(&bin, &mut pins)? {
            return Err("ARK plugins changed before removal; all files were retained".into());
        }
        drop(stage_pin);
        fs::rename(&staging, &directory).map_err(|e| e.to_string())
    })()
    .map_err(|error| {
        format!(
            "{error}; ARK removal preparation retained at {}",
            staging.display()
        )
    })?;
    let transaction_pin = files::pin(&directory)?;
    recovery::verify_entries(&directory, &expected, true)?;
    let result = (|| {
        for (index, (_, file)) in std::mem::take(&mut locked).into_iter().enumerate() {
            if let Some(file) = file {
                file.remove()?;
            }
            after_remove(index)?;
        }
        recovery::verify_runtime(&bin, &journal, true, false)?;
        if !framework_retained && other_plugins(&bin, &mut pins)? {
            return Err(
                "ARK plugins changed during removal; restoring the shared framework".into(),
            );
        }
        Ok(())
    })();
    drop(locked);
    drop(pins);
    drop(owner);
    drop(transaction_pin);
    if let Err(error) = result {
        return match recovery::recover(&bin, profile, wanted) {
            Ok(()) => Err(error),
            Err(recovery) => Err(format!(
                "{error}; ARK removal retained for recovery: {recovery}"
            )),
        };
    }
    fs::rename(directory, bin.join(COMMITTED)).map_err(|e| e.to_string())?;
    recovery::recover(&bin, profile, wanted)?;
    Ok(framework_retained)
}

#[cfg(test)]
#[path = "ark_tools_uninstall_tests.rs"]
mod tests;
