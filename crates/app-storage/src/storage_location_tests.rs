use super::{LOCATION_FILE, MAX_LOCATION_BYTES, StorageError, StoragePaths, resolve_paths};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};

use serde_json::{Value, json};
use uuid::Uuid;

struct Fixture {
    root: PathBuf,
    user: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("lgsm-storage-location-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        Self {
            user: root.join("user"),
            root,
        }
    }

    fn candidate(&self, name: &str) -> PathBuf {
        self.root.join(name).join("LanGame")
    }

    fn pointer(&self) -> PathBuf {
        self.user.join(LOCATION_FILE)
    }

    fn initialize(&self) -> StoragePaths {
        resolve_paths(&self.user, || Ok(vec![self.candidate("selected")])).unwrap()
    }

    fn read_pointer(&self) -> Value {
        serde_json::from_slice(&fs::read(self.pointer()).unwrap()).unwrap()
    }

    fn write_pointer(&self, value: &Value) {
        fs::write(self.pointer(), serde_json::to_vec(value).unwrap()).unwrap();
    }

    fn reject_without_reselection(&self) -> StorageError {
        let pointer = fs::read(self.pointer()).unwrap();
        let error = resolve_paths(&self.user, || {
            panic!("must not reselect a saved data location")
        })
        .unwrap_err();
        assert_eq!(fs::read(self.pointer()).unwrap(), pointer);
        error
    }

    fn remove_directory(&self, path: &Path) {
        assert!(path.starts_with(&self.root) && path != self.root);
        fs::remove_dir_all(path).unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.root) {
            if !std::thread::panicking() {
                panic!(
                    "cannot remove owned fixture {}: {error}",
                    self.root.display()
                );
            }
            eprintln!(
                "cannot remove owned fixture {}: {error}",
                self.root.display()
            );
        }
    }
}

#[test]
fn first_start_creates_complete_layout_and_publishes_matching_settings_and_pointer() {
    let fixture = Fixture::new();
    let runtime = fixture.candidate("selected");
    assert!(!runtime.exists());
    let paths = fixture.initialize();
    assert_eq!(
        paths.app_data_root.parent(),
        Some(runtime.join("app-data/ServerManager").as_path())
    );
    Uuid::parse_str(paths.app_data_root.file_name().unwrap().to_str().unwrap()).unwrap();
    assert_eq!(
        paths.settings_path,
        paths.app_data_root.join("settings.json")
    );
    assert_eq!(paths.database_path, paths.app_data_root.join("db/lgs.db"));
    assert_eq!(paths.logs_root, paths.app_data_root.join("logs"));
    assert_eq!(paths.games_root, runtime.join("server-files"));
    assert_eq!(paths.instances_root, runtime.join("instances"));
    assert_eq!(paths.archives_root, runtime.join("instances/.trash"));
    assert_eq!(paths.steamcmd_root, runtime.join("cmd/steamcmd"));
    for directory in [
        &paths.app_data_root,
        paths.database_path.parent().unwrap(),
        &paths.logs_root,
        &paths.games_root,
        &paths.instances_root,
        &paths.archives_root,
        &paths.steamcmd_root,
    ] {
        assert!(
            directory.is_dir(),
            "missing directory: {}",
            directory.display()
        );
    }
    assert!(paths.database_path.is_file());
    let settings: Value = serde_json::from_slice(&fs::read(&paths.settings_path).unwrap()).unwrap();
    assert_eq!(settings["servers_root"], json!(runtime.join("instances")));
    assert_eq!(settings["games_root"], json!(runtime.join("server-files")));
    assert_eq!(
        settings["steamcmd_root"],
        json!(runtime.join("cmd/steamcmd"))
    );
    assert_eq!(settings, serde_json::to_value(paths.settings()).unwrap());
    assert_eq!(
        fixture.read_pointer(),
        json!({"version": 1, "runtime_root": runtime, "app_data_root": paths.app_data_root})
    );
    assert!(fs::read_dir(&runtime).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".lgsm-write-probe-")
    }));
}

#[tokio::test(flavor = "current_thread")]
async fn first_start_inside_a_runtime_creates_a_database_that_reopens_with_its_schema() {
    let fixture = Fixture::new();
    let paths = fixture.initialize();
    let bootstrap = crate::bootstrap_storage_with_paths(paths.clone()).unwrap();
    let status = crate::initialize_database(&bootstrap.paths).await.unwrap();
    assert!(status.database_exists && status.migrations_applied);
    assert!(status.schema_version > 0);
    assert_eq!(status.database_path, paths.database_path.to_string_lossy());
    let reopened = resolve_paths(&fixture.user, || panic!("must reuse initialized data")).unwrap();
    assert_eq!(reopened.database_path, paths.database_path);
}

#[test]
fn subsequent_start_keeps_the_registered_location_without_enumerating_candidates() {
    let fixture = Fixture::new();
    let first = fixture.initialize();
    fs::write(&first.database_path, b"existing database bytes").unwrap();
    let settings = fs::read(&first.settings_path).unwrap();
    let pointer = fs::read(fixture.pointer()).unwrap();
    let next = resolve_paths(&fixture.user, || panic!("saved selection must win")).unwrap();
    assert_eq!(next.app_data_root, first.app_data_root);
    assert_eq!(next.database_path, first.database_path);
    assert_eq!(next.instances_root, first.instances_root);
    assert_eq!(
        fs::read(&next.database_path).unwrap(),
        b"existing database bytes"
    );
    assert_eq!(fs::read(&next.settings_path).unwrap(), settings);
    assert_eq!(fs::read(fixture.pointer()).unwrap(), pointer);
}

#[test]
fn a_candidate_that_is_a_file_falls_back_without_modifying_that_file() {
    let fixture = Fixture::new();
    let blocked = fixture.candidate("blocked");
    fs::create_dir_all(blocked.parent().unwrap()).unwrap();
    fs::write(&blocked, b"owned by another purpose").unwrap();
    let fallback = fixture.candidate("fallback");
    let paths = resolve_paths(&fixture.user, || {
        Ok(vec![blocked.clone(), fallback.clone()])
    })
    .unwrap();
    assert_eq!(paths.instances_root, fallback.join("instances"));
    assert_eq!(fixture.read_pointer()["runtime_root"], json!(fallback));
    assert_eq!(fs::read(blocked).unwrap(), b"owned by another purpose");
}

#[test]
fn an_unusable_business_directory_falls_back_before_publishing_a_location() {
    let fixture = Fixture::new();
    let blocked = fixture.candidate("blocked");
    fs::create_dir_all(&blocked).unwrap();
    fs::write(blocked.join("instances"), b"preserve this file").unwrap();
    let fallback = fixture.candidate("fallback");
    let paths = resolve_paths(&fixture.user, || {
        Ok(vec![blocked.clone(), fallback.clone()])
    })
    .unwrap();
    assert_eq!(paths.instances_root, fallback.join("instances"));
    assert!(paths.database_path.is_file());
    assert_eq!(fixture.read_pointer()["runtime_root"], json!(fallback));
    assert_eq!(
        fs::read(blocked.join("instances")).unwrap(),
        b"preserve this file"
    );
}

#[test]
fn no_candidates_or_only_failed_candidates_return_an_error_without_a_pointer() {
    for blocked_count in [0, 2] {
        let fixture = Fixture::new();
        let mut roots = Vec::new();
        for index in 0..blocked_count {
            let path = fixture.candidate(&format!("blocked-{index}"));
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, b"preserve").unwrap();
            roots.push(path);
        }
        let error = resolve_paths(&fixture.user, || Ok(roots.clone())).unwrap_err();
        assert!(
            matches!(&error, StorageError::CreatePath { path, .. } if path == &fixture.pointer())
        );
        assert!(error.to_string().contains("No writable local drive"));
        assert!(!fixture.pointer().exists());
        for path in roots {
            assert_eq!(fs::read(path).unwrap(), b"preserve");
        }
    }
}

#[cfg(windows)]
#[test]
fn junction_business_roots_fall_back_without_touching_their_targets() {
    use std::os::windows::process::CommandExt;

    struct Junction(PathBuf);
    impl Drop for Junction {
        fn drop(&mut self) {
            if let Err(error) = fs::remove_dir(&self.0)
                && error.kind() != io::ErrorKind::NotFound
            {
                if !std::thread::panicking() {
                    panic!("cannot unlink owned fixture junction: {error}");
                }
                eprintln!("cannot unlink owned fixture junction: {error}");
            }
        }
    }

    for directory in ["instances", "server-files"] {
        let fixture = Fixture::new();
        let blocked = fixture.candidate("blocked");
        let fallback = fixture.candidate("fallback");
        let outside = fixture.root.join("outside");
        fs::create_dir_all(&blocked).unwrap();
        fs::create_dir(&outside).unwrap();
        let sentinel = outside.join("retained-save.bin");
        fs::write(&sentinel, b"external save must remain unchanged").unwrap();
        let junction = Junction(blocked.join(directory));
        let created = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(junction.0.to_string_lossy().replace('/', "\\"))
            .arg(outside.to_string_lossy().replace('/', "\\"))
            .stdin(std::process::Stdio::null())
            .creation_flags(0x0800_0000)
            .output()
            .unwrap();
        assert!(
            created.status.success(),
            "junction fixture failed: stdout={} stderr={}",
            String::from_utf8_lossy(&created.stdout),
            String::from_utf8_lossy(&created.stderr)
        );
        let result = resolve_paths(&fixture.user, || {
            Ok(vec![blocked.clone(), fallback.clone()])
        });
        // Remove only the link before any assertion or recursive fixture cleanup.
        drop(junction);
        let paths = result.unwrap();
        assert_eq!(paths.instances_root, fallback.join("instances"));
        assert_eq!(paths.games_root, fallback.join("server-files"));
        assert!(paths.app_data_root.starts_with(&fallback));
        assert!(paths.database_path.is_file());
        assert!(paths.settings_path.is_file());
        assert_eq!(fixture.read_pointer()["runtime_root"], json!(fallback));
        assert_eq!(
            fs::read(&sentinel).unwrap(),
            b"external save must remain unchanged"
        );
        let remaining: Vec<_> = fs::read_dir(&outside)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(
            remaining,
            vec![sentinel],
            "no files may be added to {directory}'s target"
        );
    }
}

#[test]
fn candidate_discovery_failure_is_reported_without_publishing_a_pointer() {
    let fixture = Fixture::new();
    let error = resolve_paths(&fixture.user, || {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "discovery denied",
        ))
    })
    .unwrap_err();
    assert!(matches!(error, StorageError::ReadPath { source, .. }
        if source.kind() == io::ErrorKind::PermissionDenied));
    assert!(!fixture.pointer().exists());
}

#[test]
fn existing_settings_or_database_preserve_the_user_root_and_old_local_runtime() {
    for existing in ["settings.json", "db/lgs.db"] {
        let fixture = Fixture::new();
        let local_runtime = fixture.user.join("runtime");
        fs::create_dir_all(&local_runtime).unwrap();
        let retained = fixture.user.join(existing);
        fs::create_dir_all(retained.parent().unwrap()).unwrap();
        fs::write(&retained, b"existing user bytes must not be replaced").unwrap();
        let paths = resolve_paths(&fixture.user, || panic!("existing data must not move")).unwrap();
        assert_eq!(paths.app_data_root, fixture.user);
        assert_eq!(paths.settings_path, fixture.user.join("settings.json"));
        assert_eq!(paths.database_path, fixture.user.join("db/lgs.db"));
        assert_eq!(paths.instances_root, local_runtime.join("instances"));
        assert_eq!(paths.games_root, local_runtime.join("server-files"));
        assert_eq!(paths.steamcmd_root, local_runtime.join("cmd/steamcmd"));
        assert_eq!(
            fs::read(retained).unwrap(),
            b"existing user bytes must not be replaced"
        );
        assert!(!fixture.pointer().exists());
        assert!(!fixture.user.join("storage-location.lock").exists());
    }
}

#[test]
fn malformed_or_oversized_pointer_is_preserved_and_never_reselected() {
    for bytes in [
        b"{ malformed".to_vec(),
        vec![b' '; MAX_LOCATION_BYTES as usize + 1],
    ] {
        let fixture = Fixture::new();
        fixture.initialize();
        fs::write(fixture.pointer(), bytes).unwrap();
        let error = fixture.reject_without_reselection();
        assert!(matches!(
            error,
            StorageError::InvalidConfigJson { .. } | StorageError::ReadPath { .. }
        ));
    }
}

#[test]
fn invalid_pointer_schema_or_paths_cannot_select_another_location() {
    for case in [
        "version",
        "unknown",
        "missing",
        "relative",
        "parent",
        "root-name",
        "outside",
        "id",
    ] {
        let fixture = Fixture::new();
        fixture.initialize();
        let mut pointer = fixture.read_pointer();
        match case {
            "version" => pointer["version"] = json!(2),
            "unknown" => pointer["extra"] = json!(true),
            "missing" => {
                pointer.as_object_mut().unwrap().remove("runtime_root");
            }
            "relative" => pointer["runtime_root"] = json!("relative/LanGame"),
            "parent" => pointer["runtime_root"] = json!(fixture.root.join("unused/../LanGame")),
            "root-name" => pointer["runtime_root"] = json!(fixture.root.join("Other")),
            "outside" => {
                pointer["app_data_root"] = json!(fixture.root.join(Uuid::new_v4().to_string()))
            }
            "id" => {
                pointer["app_data_root"] = json!(
                    fixture
                        .candidate("selected")
                        .join("app-data/ServerManager/not-an-id")
                )
            }
            _ => unreachable!(),
        }
        fixture.write_pointer(&pointer);
        let error = fixture.reject_without_reselection();
        assert!(
            matches!(
                error,
                StorageError::InvalidConfigJson { .. } | StorageError::ReadPath { .. }
            ),
            "{case}"
        );
    }
}

#[test]
fn missing_registered_root_private_directory_settings_or_database_is_not_recreated() {
    for missing in ["runtime", "private", "settings", "database"] {
        let fixture = Fixture::new();
        let paths = fixture.initialize();
        let path = match missing {
            "runtime" => fixture.candidate("selected"),
            "private" => paths.app_data_root,
            "settings" => paths.settings_path,
            "database" => paths.database_path,
            _ => unreachable!(),
        };
        if path.is_dir() {
            fixture.remove_directory(&path);
        } else {
            fs::remove_file(&path).unwrap();
        }
        let error = fixture.reject_without_reselection();
        assert!(matches!(error, StorageError::ReadPath { .. }), "{missing}");
        assert!(!path.exists(), "missing {missing} must not be recreated");
    }
}

#[test]
fn concurrent_first_starts_publish_one_location_and_share_its_files() {
    let fixture = Fixture::new();
    let barrier = Arc::new(Barrier::new(4));
    let selections = Arc::new(AtomicUsize::new(0));
    let workers: Vec<_> = (0..4)
        .map(|index| {
            let user = fixture.user.clone();
            let candidate = fixture.candidate(&format!("candidate-{index}"));
            let barrier = Arc::clone(&barrier);
            let selections = Arc::clone(&selections);
            std::thread::spawn(move || {
                barrier.wait();
                resolve_paths(&user, || {
                    selections.fetch_add(1, Ordering::SeqCst);
                    Ok(vec![candidate])
                })
            })
        })
        .collect();
    let joined: Vec<_> = workers.into_iter().map(|worker| worker.join()).collect();
    let results: Vec<_> = joined.into_iter().map(Result::unwrap).collect();
    let paths: Vec<_> = results.into_iter().map(Result::unwrap).collect();
    assert_eq!(selections.load(Ordering::SeqCst), 1);
    for other in &paths[1..] {
        assert_eq!(other.app_data_root, paths[0].app_data_root);
        assert_eq!(other.database_path, paths[0].database_path);
        assert_eq!(other.settings_path, paths[0].settings_path);
        assert_eq!(other.instances_root, paths[0].instances_root);
    }
    assert!(paths[0].database_path.is_file());
    assert_eq!(
        fixture.read_pointer()["app_data_root"],
        json!(paths[0].app_data_root)
    );
    let settings: Value =
        serde_json::from_slice(&fs::read(&paths[0].settings_path).unwrap()).unwrap();
    assert_eq!(settings, serde_json::to_value(paths[0].settings()).unwrap());
}
