//! Stop deploying save-only UE4SS mods after returning to native admin commands.
//! Only recorded, unchanged leaves are retired; the loader and other mods stay.
use super::{installation, process, runtime_root, stopped};
use app_core::InstanceDetails;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ownership {
    protocol: u32,
    module_id: String,
    files: BTreeMap<String, String>,
    pending_files: Option<BTreeMap<String, String>>,
}

fn profile(module: &str) -> Option<(&'static str, process::GameProfile, &'static [&'static str])> {
    match module {
        "palworld" => Some((
            "Pal",
            process::GameProfile::Palworld,
            &[
                "Scripts/main.lua",
                "enabled.txt",
                "native/LgsmPalworldSaveControl.dll",
                "dlls/main.dll",
            ],
        )),
        "astroneer" => Some((
            "Astro",
            process::GameProfile::Astroneer,
            &[
                "Scripts/main.lua",
                "enabled.txt",
                "native/LgsmAstroneerSaveControl.dll",
            ],
        )),
        "humanitz" => Some((
            "HumanitZServer",
            process::GameProfile::Humanitz,
            &[
                "Scripts/main.lua",
                "enabled.txt",
                "native/LgsmHumanitZSave.dll",
                "native/THIRD_PARTY_NOTICES.txt",
            ],
        )),
        _ => None,
    }
}

fn digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn inspect_tree(directory: &Path, relative: &str, allowed: &[&str]) -> Result<(), String> {
    if !installation::exists(directory)? {
        return Ok(());
    }
    let _pins = installation::pin(directory, false)?;
    for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "An unknown save-mod entry was preserved.")?;
        let relative = if relative.is_empty() {
            name
        } else {
            format!("{relative}/{name}")
        };
        let kind = entry.file_type().map_err(|error| error.to_string())?;
        if kind.is_dir()
            && allowed
                .iter()
                .any(|path| path.starts_with(&format!("{relative}/")))
        {
            inspect_tree(&entry.path(), &relative, allowed)?;
        } else if !kind.is_file() || !allowed.contains(&relative.as_str()) {
            return Err(
                "The retired save mod contains an unknown entry; all files were preserved.".into(),
            );
        }
    }
    Ok(())
}

fn retire(module: &str, root: &Path, project: &str, allowed: &[&str]) -> Result<(), String> {
    let _root = installation::pin(root, false)?;
    let directory = root
        .join(project)
        .join("Binaries/Win64/ue4ss/Mods/LgsmWorldSave");
    let record = root.join(installation::OWNER);
    if !installation::exists(&record)? {
        if allowed.iter().any(|path| directory.join(path).exists()) {
            return Err("An unowned save extension remains; it was preserved. Remove or relocate that save extension before starting.".into());
        }
        return Ok(());
    }
    let before = installation::read(&record, 8192)?;
    let owner: Ownership = serde_json::from_slice(&before)
        .map_err(|_| "The retired save ownership record is invalid; files were preserved.")?;
    let pending = owner.pending_files.as_ref();
    let known = owner
        .files
        .keys()
        .chain(pending.into_iter().flat_map(|files| files.keys()))
        .cloned()
        .collect::<BTreeSet<_>>();
    if owner.protocol != 1
        || owner.module_id != module
        || !known.contains("Scripts/main.lua")
        || !known.contains("enabled.txt")
        || owner
            .files
            .iter()
            .chain(pending.into_iter().flat_map(|files| files.iter()))
            .any(|(path, value)| !allowed.contains(&path.as_str()) || !digest(value))
    {
        return Err(
            "The retired save ownership record does not match this module; files were preserved."
                .into(),
        );
    }
    let recorded = known.iter().map(String::as_str).collect::<Vec<_>>();
    inspect_tree(&directory, "", &recorded)?;
    let mut actual = BTreeMap::new();
    // Validate every retained leaf before changing the first one. Missing leaves
    // permit an interrupted prior cleanup to resume using the same owner record.
    for name in &known {
        let path = directory.join(name);
        if !installation::exists(&path)? {
            continue;
        }
        let _parents =
            installation::pin(path.parent().ok_or("Invalid retired save path.")?, false)?;
        let value = installation::hash(&installation::read(&path, 16 * 1024 * 1024)?);
        if owner.files.get(name) != Some(&value)
            && pending.and_then(|files| files.get(name)) != Some(&value)
        {
            return Err("A retired save extension was modified; all files were preserved.".into());
        }
        actual.insert(path, value);
    }
    // Disable its owned Lua entry before removing code. C++ early-entry leaves
    // also appear in this same recorded set. Unknown user mods are never scanned.
    if let Some(value) = actual.remove(&directory.join("enabled.txt")) {
        crate::commands::commands_managed_save_control::retire_owned_file(
            &directory.join("enabled.txt"),
            &value,
        )?;
    }
    for (path, value) in actual {
        crate::commands::commands_managed_save_control::retire_owned_file(&path, &value)?;
    }
    crate::commands::commands_managed_save_control::retire_owned_file(
        &record,
        &installation::hash(&before),
    )
}

pub(crate) fn cleanup_retired(details: &InstanceDetails) -> Result<(), String> {
    let module = details.summary.module_id.as_str();
    let Some((project, game, allowed)) = profile(module) else {
        return Ok(());
    };
    stopped(details)?;
    let root = runtime_root(details)?;
    // Ordinary shared runtimes have no owned observer to retire. They can be
    // used by another managed instance; do not require that unrelated run to
    // stop merely to establish that there is nothing to change here.
    if !installation::exists(&root.join(installation::OWNER))? {
        return retire(module, &root, project, allowed);
    }
    // This checks current process/image ownership only, never a pinned build.
    process::stopped(game, &root)?;
    retire(module, &root, project, allowed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    struct Fixture(std::path::PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
    fn fixture() -> (Fixture, std::path::PathBuf) {
        let root =
            std::env::temp_dir().join(format!("langame-retired-unreal-{}", uuid::Uuid::new_v4()));
        let directory = root.join("Pal/Binaries/Win64/ue4ss/Mods/LgsmWorldSave");
        fs::create_dir_all(directory.join("Scripts")).unwrap();
        fs::write(directory.join("Scripts/main.lua"), b"owned script").unwrap();
        fs::write(directory.join("enabled.txt"), b"").unwrap();
        fs::write(root.join(installation::OWNER), serde_json::to_vec(&json!({"protocol":1,"module_id":"palworld",
            "files":{"Scripts/main.lua":installation::hash(b"owned script"),"enabled.txt":installation::hash(b"")},
            "pending_files":null})).unwrap()).unwrap();
        (Fixture(root), directory)
    }
    #[test]
    fn retirement_preserves_loader_other_mods_and_resumes_missing_owned_leaves() {
        let (fixture, directory) = fixture();
        let loader = directory
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("UE4SS.dll");
        fs::write(&loader, b"user loader").unwrap();
        let other = directory.parent().unwrap().join("UserMod");
        fs::create_dir(&other).unwrap();
        fs::write(other.join("enabled.txt"), b"user").unwrap();
        fs::remove_file(directory.join("enabled.txt")).unwrap();
        let (_, _, allowed) = profile("palworld").unwrap();
        retire("palworld", &fixture.0, "Pal", allowed).unwrap();
        assert!(!directory.join("Scripts/main.lua").exists());
        assert!(!fixture.0.join(installation::OWNER).exists());
        assert_eq!(fs::read(loader).unwrap(), b"user loader");
        assert_eq!(fs::read(other.join("enabled.txt")).unwrap(), b"user");
        retire("palworld", &fixture.0, "Pal", allowed).unwrap();
    }
    #[test]
    fn altered_or_unknown_files_prevent_any_retirement() {
        for scenario in 0..3 {
            let (fixture, directory) = fixture();
            match scenario {
                0 => {
                    fs::write(directory.join("Scripts/main.lua"), b"external edit").unwrap();
                }
                1 => {
                    fs::write(directory.join("user.lua"), b"user").unwrap();
                }
                _ => {
                    fs::create_dir(directory.join("native")).unwrap();
                    fs::write(
                        directory.join("native/LgsmPalworldSaveControl.dll"),
                        b"unrecorded",
                    )
                    .unwrap();
                }
            }
            let (_, _, allowed) = profile("palworld").unwrap();
            assert!(retire("palworld", &fixture.0, "Pal", allowed).is_err());
            assert!(directory.join("enabled.txt").exists());
            assert!(fixture.0.join(installation::OWNER).exists());
        }
    }
}
