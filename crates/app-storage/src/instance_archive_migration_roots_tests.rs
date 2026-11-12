//! Read-only admission probes over disposable UUID directories. Ordinary archive
//! operations must keep inspecting their live roots; only a takeover ignores old tails.
use super::*;
use std::fs;

struct Fixture {
    root: PathBuf,
    paths: StoragePaths,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lgsm-archive-migration-roots-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let state = root.join("state");
        let instances = root.join("instances");
        fs::create_dir(&instances).unwrap();
        let paths = StoragePaths {
            app_data_root: state.clone(),
            settings_path: state.join("settings.json"),
            database_path: state.join("db/lgs.db"),
            logs_root: state.join("logs"),
            modules_root: root.join("modules"),
            migrations_root: root.join("migrations"),
            steamcmd_root: root.join("steamcmd"),
            games_root: root.join("games"),
            archives_root: instances.join(".trash"),
            instances_root: instances,
        };
        Self { root, paths }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        assert!(
            self.root
                .starts_with(fs::canonicalize(std::env::temp_dir()).unwrap())
        );
        assert!(
            self.root
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("lgsm-archive-migration-roots-")
        );
        // Link probes unlink their exact fixture-owned link before teardown.
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn migration_admission_ignores_a_file_at_the_old_targets_archive_leaf() {
    let fixture = Fixture::new();
    fs::write(
        &fixture.paths.archives_root,
        b"previous destination contents",
    )
    .unwrap();
    validate_migration_archive_root(&fixture.paths).unwrap();
    assert!(validate_archive_root(&fixture.paths).is_err());
    assert_eq!(
        fs::read(&fixture.paths.archives_root).unwrap(),
        b"previous destination contents"
    );
    assert!(!fixture.paths.games_root.exists());
    assert!(!fixture.paths.database_path.exists());
}

#[test]
fn migration_admission_ignores_old_internal_types_below_a_common_outer_target() {
    let fixture = Fixture::new();
    let outer = fixture.root.join("whole-target");
    fs::create_dir(&outer).unwrap();
    fs::write(
        outer.join("instances"),
        b"not a directory in the old target",
    )
    .unwrap();
    fs::write(outer.join("tools"), b"unrelated old tools file").unwrap();
    let mut paths = fixture.paths.clone();
    paths.games_root = outer.clone();
    paths.instances_root = outer.join("instances");
    paths.archives_root = paths.instances_root.join(".trash");
    paths.steamcmd_root = outer.join("tools/steamcmd");
    validate_migration_archive_root(&paths).unwrap();
    assert!(validate_archive_root(&paths).is_err());
    assert_eq!(
        fs::read(outer.join("instances")).unwrap(),
        b"not a directory in the old target"
    );
    assert_eq!(
        fs::read(outer.join("tools")).unwrap(),
        b"unrelated old tools file"
    );
}

#[test]
fn migration_admission_preserves_unknown_old_target_contents_without_reading_ownership() {
    let fixture = Fixture::new();
    fs::create_dir(&fixture.paths.archives_root).unwrap();
    let unknown = fixture
        .paths
        .archives_root
        .join("unregistered-archive.json");
    fs::write(&unknown, b"not LGSM metadata").unwrap();
    let recovery = fixture
        .paths
        .instances_root
        .join(".langame-ark-restore.json");
    fs::write(&recovery, b"old target data, not source recovery").unwrap();
    validate_migration_archive_root(&fixture.paths).unwrap();
    assert_eq!(fs::read(unknown).unwrap(), b"not LGSM metadata");
    assert_eq!(
        fs::read(recovery).unwrap(),
        b"old target data, not source recovery"
    );
}

#[test]
fn migration_admission_still_rejects_invalid_archive_layouts() {
    let fixture = Fixture::new();
    for archive in [
        fixture.paths.instances_root.clone(),
        fixture.paths.instances_root.join("custom-archives"),
        fixture.paths.games_root.join("archives"),
        fixture.paths.steamcmd_root.join("archives"),
        fixture.paths.logs_root.join("archives"),
        fixture.paths.modules_root.join("archives"),
        fixture.paths.migrations_root.join("archives"),
        fixture.paths.database_path.clone(),
        fixture.paths.settings_path.clone(),
    ] {
        let paths = StoragePaths {
            archives_root: archive.clone(),
            ..fixture.paths.clone()
        };
        assert!(
            validate_migration_archive_root(&paths).is_err(),
            "Invalid layout accepted: {}",
            archive.display()
        );
    }
}

#[test]
fn migration_volume_uses_the_external_parent_of_the_outermost_target() {
    let fixture = Fixture::new();
    let outer = fixture.root.join("replacement");
    // The root itself may be an ordinary file that the takeover will retain.
    fs::write(&outer, b"old root file").unwrap();
    let child = outer.join("instances/.trash");
    let targets = vec![outer.join("instances"), child.clone(), outer.clone()];
    assert_eq!(
        migration_target_parent(&child, &targets).unwrap(),
        fixture.root.as_path()
    );
    let mut paths = fixture.paths.clone();
    paths.games_root = outer.clone();
    paths.instances_root = outer.join("instances");
    paths.archives_root = child;
    validate_migration_archive_root(&paths).unwrap();
    assert_eq!(fs::read(outer).unwrap(), b"old root file");
}

#[cfg(windows)]
#[test]
fn migration_admission_rejects_an_archive_destination_on_another_available_volume() {
    let fixture = Fixture::new();
    let current = nearest_directory_identity(&fixture.root).unwrap();
    // This check is read-only even on the second drive: the destination is a
    // missing UUID leaf and no directory is created outside the owned fixture.
    let other = [r"C:\", r"D:\", r"E:\"].into_iter().find_map(|root| {
        let path = Path::new(root);
        crate::storage_migration_files::volume_root_identity(path)
            .ok()
            .filter(|identity| !current.same_volume(identity))
            .map(|_| path.to_owned())
    });
    let Some(other) = other else {
        eprintln!("No second local volume is available for the read-only migration volume probe.");
        return;
    };
    let mut paths = fixture.paths.clone();
    paths.archives_root = other.join(format!(
        "lgsm-uncreated-archive-probe-{}",
        uuid::Uuid::new_v4()
    ));
    assert!(!paths.archives_root.exists());
    let result = validate_migration_archive_root(&paths).unwrap_err();
    assert!(result.to_string().contains("same volume"), "{result}");
    assert!(!paths.archives_root.exists());
}

#[cfg(windows)]
#[test]
fn only_old_inner_junctions_are_ignored_while_live_archive_and_outer_links_stay_rejected() {
    let fixture = Fixture::new();
    let outside = fixture.root.join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("sentinel"), b"retained outside contents").unwrap();
    let create = |link: &Path| {
        let output = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(link)
            .arg(&outside)
            .output()
            .unwrap();
        assert!(output.status.success(), "junction fixture: {output:?}");
    };
    create(&fixture.paths.archives_root);
    let migration = validate_migration_archive_root(&fixture.paths);
    let ordinary = validate_archive_root(&fixture.paths);
    fs::remove_dir(&fixture.paths.archives_root).unwrap();
    assert!(migration.is_ok(), "{migration:?}");
    assert!(ordinary.is_err());

    let outer = fixture.root.join("outer-link");
    create(&outer);
    let mut paths = fixture.paths.clone();
    paths.instances_root = outer.clone();
    paths.archives_root = outer.join(".trash");
    let migration = validate_migration_archive_root(&paths);
    fs::remove_dir(&outer).unwrap();
    assert!(migration.is_err());
    assert_eq!(
        fs::read(outside.join("sentinel")).unwrap(),
        b"retained outside contents"
    );
}
