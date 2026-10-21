use super::*;
use crate::StoragePaths;
use crate::templates::ModuleSupportMaterializationContext;
use serde_json::Map;
use std::path::PathBuf;

struct Fixture {
    root: PathBuf,
    runtime: PathBuf,
    config: PathBuf,
    paths: StoragePaths,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("lgsm-dst-observer-{}", uuid::Uuid::new_v4()));
        let runtime = root.join("instance/runtime");
        let config = root.join("instance/config");
        fs::create_dir_all(&runtime).unwrap();
        fs::create_dir_all(&config).unwrap();
        fs::write(
            runtime.join(crate::private_runtime::PRIVATE_RUNTIME_MARKER),
            b"managed\n",
        )
        .unwrap();
        let paths = StoragePaths {
            app_data_root: root.join("appdata"),
            settings_path: root.join("appdata/settings.json"),
            database_path: root.join("appdata/db/lgs.db"),
            logs_root: root.join("appdata/logs"),
            modules_root: root.join("modules"),
            migrations_root: root.join("migrations"),
            steamcmd_root: root.join("steamcmd"),
            games_root: root.join("games"),
            instances_root: root.join("instances"),
            archives_root: root.join("instances/.trash"),
        };
        Self {
            root,
            runtime,
            config,
            paths,
        }
    }

    fn settings(&self) -> PathBuf {
        self.runtime.join("mods/modsettings.lua")
    }
    fn helper(&self) -> PathBuf {
        self.runtime.join("mods").join(DIRECTORY)
    }

    fn seed_settings(&self, bytes: &[u8]) {
        fs::create_dir_all(self.settings().parent().unwrap()).unwrap();
        fs::write(self.settings(), bytes).unwrap();
    }

    fn apply(&self, running: bool) -> Result<ManagedConfigMutation, StorageError> {
        self.apply_at(&self.runtime, running)
    }

    fn apply_at(
        &self,
        runtime: &Path,
        running: bool,
    ) -> Result<ManagedConfigMutation, StorageError> {
        let mut mutation = ManagedConfigMutation::new("dontstarve");
        let context = ModuleSupportMaterializationContext {
            storage_paths: &self.paths,
            module_id: "dontstarve",
            install_root: runtime,
            shared_install_root: &self.paths.games_root,
            config_dir: &self.config,
            saves_dir: &self.config.join("clusters/main"),
            instance_id: "observer-fixture",
            instance_running: running,
            settings: &Map::new(),
        };
        match super::super::sync_dst_mod_setup(&context, &mut mutation) {
            Ok(()) => Ok(mutation),
            Err(error) => Err(mutation.rollback_after(error)),
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn observer_ownership_requires_the_complete_marked_directory() {
    let fixture = Fixture::new();
    assert!(!is_managed_dst_map_observer(&fixture.helper()).unwrap());
    fixture.apply(false).unwrap().commit();
    assert!(is_managed_dst_map_observer(&fixture.helper()).unwrap());
    fs::write(fixture.helper().join("user-notes.txt"), b"keep visible").unwrap();
    assert!(!is_managed_dst_map_observer(&fixture.helper()).unwrap());
    fs::remove_file(fixture.helper().join("user-notes.txt")).unwrap();
    fs::write(fixture.helper().join("modinfo.lua"), b"name='User Mod'\n").unwrap();
    assert!(!is_managed_dst_map_observer(&fixture.helper()).unwrap());
    fs::write(fixture.helper().join("modinfo.lua"), RESOURCES[0].1).unwrap();
    fs::remove_file(fixture.helper().join("modmain.lua")).unwrap();
    assert!(!is_managed_dst_map_observer(&fixture.helper()).unwrap());
    fs::write(fixture.helper().join("modmain.lua"), RESOURCES[1].1).unwrap();
    fs::remove_file(fixture.helper().join("render_observer.lua")).unwrap();
    assert!(!is_managed_dst_map_observer(&fixture.helper()).unwrap());
}

#[test]
fn observer_adds_the_shared_collector_when_updating_an_owned_icon_only_helper() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.helper()).unwrap();
    for name in ["modinfo.lua", "modmain.lua"] {
        fs::write(
            fixture.helper().join(name),
            [MARKER, b"\n-- previous owned icon observer\n"].concat(),
        )
        .unwrap();
    }
    fixture.apply(false).unwrap().commit();
    for (name, contents) in RESOURCES {
        assert_eq!(fs::read(fixture.helper().join(name)).unwrap(), contents);
    }
    assert!(is_managed_dst_map_observer(&fixture.helper()).unwrap());
}

#[test]
fn observer_preserves_bom_crlf_and_arbitrary_operator_bytes_idempotently() {
    let fixture = Fixture::new();
    let original =
        b"\xef\xbb\xbf-- operator comment \x80\r\nForceEnableMod('workshop-123')\r\nreturn\r\n";
    fixture.seed_settings(original);
    fixture.apply(false).unwrap().commit();
    let after = fs::read(fixture.settings()).unwrap();
    let expected = [
        b"\xef\xbb\xbf".as_slice(),
        enable_block(b"\r\n").as_slice(),
        &original[3..],
    ]
    .concat();
    assert_eq!(after, expected);
    for (name, contents) in RESOURCES {
        assert_eq!(fs::read(fixture.helper().join(name)).unwrap(), contents);
    }
    fixture.apply(false).unwrap().commit();
    assert_eq!(fs::read(fixture.settings()).unwrap(), after);
}

#[test]
fn observer_installs_before_an_early_return_without_rewriting_operator_settings() {
    let fixture = Fixture::new();
    fixture.seed_settings(b"return\n-- native settings remain untouched");
    fixture.apply(false).unwrap().commit();
    assert_eq!(
        fs::read(fixture.settings()).unwrap(),
        [
            enable_block(b"\n"),
            b"return\n-- native settings remain untouched".to_vec()
        ]
        .concat()
    );
}

#[test]
fn observer_rejects_foreign_files_and_unexpected_entries_without_partial_changes() {
    for (name, bytes) in [
        ("modinfo.lua", b"-- foreign mod\n".as_slice()),
        ("render_observer.lua", b"-- foreign collector\n".as_slice()),
        ("operator.txt", MARKER),
    ] {
        let fixture = Fixture::new();
        fixture.seed_settings(b"-- original settings\n");
        fs::create_dir_all(fixture.helper()).unwrap();
        fs::write(fixture.helper().join(name), bytes).unwrap();
        assert!(fixture.apply(false).is_err());
        assert_eq!(fs::read(fixture.helper().join(name)).unwrap(), bytes);
        assert_eq!(
            fs::read(fixture.settings()).unwrap(),
            b"-- original settings\n"
        );
        assert!(!fixture.helper().join("modmain.lua").exists());
    }
}

#[test]
fn observer_rollback_restores_original_files_and_allows_retry_after_first_install() {
    let fixture = Fixture::new();
    fixture.seed_settings(b"-- original settings\r\n");
    fixture.apply(false).unwrap().rollback().unwrap();
    assert_eq!(
        fs::read(fixture.settings()).unwrap(),
        b"-- original settings\r\n"
    );
    assert!(fs::read_dir(fixture.helper()).unwrap().next().is_none());
    fixture.apply(false).unwrap().commit();
    let old = [MARKER, b"\n-- older managed observer\n"].concat();
    for (name, _) in RESOURCES {
        fs::write(fixture.helper().join(name), &old).unwrap();
    }
    let original_settings = fs::read(fixture.settings()).unwrap();
    crate::atomic_file::fail_next_atomic_write_for_test(&fixture.settings());
    assert!(fixture.apply(false).is_err());
    assert_eq!(fs::read(fixture.settings()).unwrap(), original_settings);
    for (name, _) in RESOURCES {
        assert_eq!(fs::read(fixture.helper().join(name)).unwrap(), old);
    }
}

#[test]
fn observer_compare_and_swap_preserves_settings_changed_after_transform() {
    let fixture = Fixture::new();
    fixture.seed_settings(b"-- planned original\n");
    let plans = prepare(&fixture.runtime).unwrap();
    fs::write(fixture.settings(), b"-- concurrent operator edit\n").unwrap();
    let mut mutation = ManagedConfigMutation::new("dontstarve");
    let error = mutation.apply(plans).unwrap_err();
    let error = mutation.rollback_after(error);
    assert!(error.to_string().contains("changed concurrently"));
    assert_eq!(
        fs::read(fixture.settings()).unwrap(),
        b"-- concurrent operator edit\n"
    );
    assert!(!fixture.helper().join("modmain.lua").exists());
    assert!(!fixture.helper().join("modinfo.lua").exists());
}

#[test]
fn observer_does_not_write_while_running_or_outside_the_verified_runtime() {
    let fixture = Fixture::new();
    fixture.seed_settings(b"-- original settings\n");
    fixture.apply(true).unwrap().commit();
    assert!(!fixture.helper().exists());
    assert_eq!(
        fs::read(fixture.settings()).unwrap(),
        b"-- original settings\n"
    );
    let other = fixture.root.join("other-runtime");
    fs::create_dir(&other).unwrap();
    assert!(fixture.apply_at(&other, false).is_err());
    assert!(fs::read_dir(other).unwrap().next().is_none());
}

#[test]
fn observer_refuses_oversized_settings_or_ambiguous_managed_blocks() {
    let fixture = Fixture::new();
    let oversized = vec![b'x'; MAX_FILE_BYTES + 1];
    fixture.seed_settings(&oversized);
    assert!(fixture.apply(false).is_err());
    assert_eq!(
        fs::metadata(fixture.settings()).unwrap().len(),
        oversized.len() as u64
    );
    assert!(!fixture.helper().exists());
    for original in [
        [enable_block(b"\n"), enable_block(b"\n")].concat(),
        [b"-- operator\n".as_slice(), enable_block(b"\n").as_slice()].concat(),
        [BLOCK_START, b"\n-- edited body\n", BLOCK_END, b"\n"].concat(),
    ] {
        fixture.seed_settings(&original);
        assert!(fixture.apply(false).is_err());
        assert_eq!(fs::read(fixture.settings()).unwrap(), original);
    }
}

#[cfg(unix)]
#[test]
fn observer_rejects_linked_mods_parent_without_writing_the_target() {
    let fixture = Fixture::new();
    let target = fixture.root.join("external-mods");
    fs::create_dir(&target).unwrap();
    std::os::unix::fs::symlink(&target, fixture.runtime.join("mods")).unwrap();
    assert!(fixture.apply(false).is_err());
    assert!(fs::read_dir(target).unwrap().next().is_none());
}
