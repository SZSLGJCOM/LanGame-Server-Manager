use super::*;
use crate::private_runtime_refresh::hash_file;
use std::path::PathBuf;
use std::sync::atomic::Ordering;

struct Fixture {
    directory: PathBuf,
    root: PathBuf,
    descriptor: ModuleDescriptor,
}

impl Fixture {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "langame-restore-program-defaults-{}",
            uuid::Uuid::new_v4()
        ));
        let module = directory.join("modules/fixture");
        fs::create_dir_all(&module).unwrap();
        fs::write(
            module.join("module.toml"),
            r#"
id = "fixture"
name = "Restore original defaults fixture"
version = "1.0.0"
[install]
shared_game_dir = "fixture"
[process]
executable = "server.bin"
[storage]
runtime_copy_exclusions = ["mods"]
"#,
        )
        .unwrap();
        let descriptor = app_modules::discover_modules(directory.join("modules"))
            .unwrap()
            .remove(0);
        let root = directory.join("library");
        fs::create_dir_all(root.join("assets")).unwrap();
        fs::write(root.join("server.bin"), b"official program").unwrap();
        fs::write(root.join("assets/required.bin"), b"official assets").unwrap();
        record_library_program_baseline(&root, &descriptor, true, None).unwrap();
        Self {
            directory,
            root,
            descriptor,
        }
    }

    fn default_file(&self) -> BTreeMap<String, String> {
        fs::create_dir_all(self.root.join("mods")).unwrap();
        let relative = "mods/shipped-default.lua";
        let path = self.root.join(relative);
        fs::write(&path, b"official shipped Lua defaults").unwrap();
        BTreeMap::from([(relative.into(), hash_file(&path, None).unwrap())])
    }

    fn manifests(&self) -> [Option<Vec<u8>>; 2] {
        [CLEAN_PACKAGE, INITIAL_PACKAGE].map(|name| {
            let path = self.root.join(name);
            path.exists().then(|| fs::read(path).unwrap())
        })
    }

    fn restore(&self, defaults: BTreeMap<String, String>) -> Result<(), StorageError> {
        restore_library_initial_program_defaults_with_progress(
            &self.root,
            &self.descriptor,
            defaults,
            None,
            &mut |_| {},
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.directory).unwrap();
    }
}

#[test]
fn restore_defaults_preserves_the_allowlist_without_steam_depot_caches() {
    for missing_initial in [false, true] {
        let fixture = Fixture::new();
        let before = read_manifest(&fixture.root).unwrap().unwrap();
        if missing_initial {
            fs::remove_file(fixture.root.join(INITIAL_PACKAGE)).unwrap();
        }
        let defaults = fixture.default_file();
        fs::write(fixture.root.join("operator.txt"), b"private operator notes").unwrap();
        fs::write(fixture.root.join("mods/user.lua"), b"user Mod").unwrap();
        let mut progress = Vec::new();
        restore_library_initial_program_defaults_with_progress(
            &fixture.root,
            &fixture.descriptor,
            defaults.clone(),
            None,
            &mut |value| progress.push(value),
        )
        .unwrap();
        let initial = read_named_manifest(&fixture.root, INITIAL_PACKAGE)
            .unwrap()
            .unwrap();
        let clean = read_manifest(&fixture.root).unwrap().unwrap();
        assert_eq!(clean.files, before.files);
        assert_eq!(clean.directories, before.directories);
        assert_eq!(initial.files.len(), before.files.len() + defaults.len());
        for (path, digest) in before.files.iter().chain(&defaults) {
            assert_eq!(initial.files.get(path), Some(digest));
        }
        assert!(initial.directories.contains("mods"));
        assert!(!initial.files.contains_key("operator.txt"));
        assert!(!initial.files.contains_key("mods/user.lua"));
        assert_eq!(
            fs::read(fixture.root.join("operator.txt")).unwrap(),
            b"private operator notes"
        );
        assert_eq!(
            fs::read(fixture.root.join("mods/user.lua")).unwrap(),
            b"user Mod"
        );
        assert!(
            progress
                .iter()
                .any(|value| value.stage == InstallVerificationStage::PackageFiles)
        );
        let final_progress = progress.last().unwrap();
        assert_eq!(
            final_progress.stage,
            InstallVerificationStage::RecordingBaseline
        );
        assert_eq!(final_progress.completed_files, 2);
    }
}

#[test]
fn restore_defaults_preserves_previously_certified_excluded_files() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.root.join("mods")).unwrap();
    fs::write(
        fixture.root.join("mods/previous.lua"),
        b"older certified default",
    )
    .unwrap();
    record_library_program_baseline(&fixture.root, &fixture.descriptor, true, None).unwrap();
    let before = read_named_manifest(&fixture.root, INITIAL_PACKAGE)
        .unwrap()
        .unwrap();
    fixture.restore(fixture.default_file()).unwrap();
    let initial = read_named_manifest(&fixture.root, INITIAL_PACKAGE)
        .unwrap()
        .unwrap();
    for (key, hash) in before.files {
        assert_eq!(initial.files.get(&key), Some(&hash));
    }
    assert!(
        !read_manifest(&fixture.root)
            .unwrap()
            .unwrap()
            .files
            .contains_key("mods/previous.lua")
    );
}

#[test]
fn restore_defaults_rejects_damaged_payloads_and_existing_hash_conflicts_without_publication() {
    for damage in ["program", "missing_program", "default", "conflicting_hash"] {
        let fixture = Fixture::new();
        let mut defaults = fixture.default_file();
        match damage {
            "program" => fs::write(fixture.root.join("server.bin"), b"operator edit").unwrap(),
            "missing_program" => fs::remove_file(fixture.root.join("assets/required.bin")).unwrap(),
            "default" => fs::write(
                fixture.root.join("mods/shipped-default.lua"),
                b"changed Lua",
            )
            .unwrap(),
            "conflicting_hash" => {
                defaults.insert("server.bin".into(), "0".repeat(64));
            }
            _ => unreachable!(),
        }
        let before = fixture.manifests();
        let payload_before = crate::test_file_snapshot::tree_snapshot(&fixture.root).unwrap();
        assert!(fixture.restore(defaults).is_err(), "{damage}");
        assert_eq!(fixture.manifests(), before, "{damage}");
        assert_eq!(
            crate::test_file_snapshot::tree_snapshot(&fixture.root).unwrap(),
            payload_before,
            "{damage}"
        );
    }
}

#[test]
fn restore_defaults_requires_consistent_existing_clean_and_initial_inventories() {
    for damage in [
        "missing_clean",
        "invalid_initial",
        "clean_module",
        "initial_module",
        "clean_hash",
        "clean_directory",
    ] {
        let fixture = Fixture::new();
        let defaults = fixture.default_file();
        match damage {
            "missing_clean" => fs::remove_file(fixture.root.join(CLEAN_PACKAGE)).unwrap(),
            "invalid_initial" => {
                fs::write(fixture.root.join(INITIAL_PACKAGE), b"invalid JSON").unwrap()
            }
            "initial_module" => {
                let mut initial = read_named_manifest(&fixture.root, INITIAL_PACKAGE)
                    .unwrap()
                    .unwrap();
                initial.module_id = "other".into();
                write_named_manifest(&fixture.root, &initial, INITIAL_PACKAGE).unwrap();
            }
            _ => {
                let mut clean = read_manifest(&fixture.root).unwrap().unwrap();
                match damage {
                    "clean_module" => clean.module_id = "other".into(),
                    "clean_hash" => {
                        clean.files.insert("server.bin".into(), "0".repeat(64));
                    }
                    "clean_directory" => {
                        clean.directories.insert("unrecorded".into());
                    }
                    _ => unreachable!(),
                }
                write_manifest(&fixture.root, &clean).unwrap();
            }
        }
        let before = fixture.manifests();
        assert!(fixture.restore(defaults).is_err(), "{damage}");
        assert_eq!(fixture.manifests(), before, "{damage}");
    }
}

#[test]
fn restore_defaults_cancellation_keeps_both_inventories_unchanged() {
    for cancel_at in [
        InstallVerificationStage::PackageFiles,
        InstallVerificationStage::RecordingBaseline,
    ] {
        let fixture = Fixture::new();
        let defaults = fixture.default_file();
        let before = fixture.manifests();
        let cancellation = AtomicBool::new(false);
        let result = restore_library_initial_program_defaults_with_progress(
            &fixture.root,
            &fixture.descriptor,
            defaults,
            Some(&cancellation),
            &mut |progress| {
                if progress.stage == cancel_at {
                    cancellation.store(true, Ordering::Release);
                }
            },
        );
        assert!(result.is_err());
        assert_eq!(fixture.manifests(), before);
    }
}
