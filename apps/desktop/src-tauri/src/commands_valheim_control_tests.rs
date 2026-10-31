use super::*;
use crate::live_players::valheim_control::{AccessLists, ControlSnapshot, ValidatedPeer};
use app_storage::PlayerAccessMutationOperation::{Add, Remove};

#[test]
fn native_profiles_require_complete_reviewed_assembly_tuples() {
    let profiles = [
        [
            "ef8c8d3a1f45694ef7a701e8e2a25bf8a3874c951a2639c81dfe614ce3aa5fa3",
            "0d66bd029bf753e2721304c775126b34af329bfcd6e632d8d38edaa96b5cf1a5",
            "8953c24a2f2af7ae257cc64fb36dde9d102c871c29b4cdd97d0c3c001de5bd5a",
        ],
        [
            "0dfc7e81436f822121148efed859ba1e661d58b9484bd45e3b6f33b6dd86bfad",
            "63e114e7cfdcc38e4f4391c18b91f774290274d2b9200e6d85d757cbe3673e18",
            "8953c24a2f2af7ae257cc64fb36dde9d102c871c29b4cdd97d0c3c001de5bd5a",
        ],
        [
            "648f3f8ed7b44c3d3cd612acd782743d9e03af7e60b7fe72d5827523e95505fd",
            "9658619e34e456b88639f6cc6e7d0fd186432f63bf1033e3e04e98e8fac2be9e",
            "1924e4c348616fbd754a8f688d3dc65cde5a741cfef57e80474fcdd0121013b5",
        ],
    ];
    let accepted = |profile: [&str; 3]| supported_profile(profile[0], profile[1], profile[2]);
    for profile in profiles {
        assert!(accepted(profile), "Reviewed native profile rejected");
        for other in profiles {
            for field in 0..3 {
                if other[field] != profile[field] {
                    let mut mixed = profile;
                    mixed[field] = other[field];
                    assert!(!accepted(mixed), "Mixed native profile accepted: {field}");
                }
            }
        }
        for field in 0..3 {
            for unknown in [
                "",
                "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            ] {
                let mut changed = profile;
                changed[field] = unknown;
                assert!(
                    !accepted(changed),
                    "Unknown native profile accepted: {field}"
                );
            }
        }
    }
}

#[tokio::test]
async fn unsupported_valheim_start_retires_owned_plugin_without_a_new_session() {
    use super::super::commands_managed_save_control::tests::Fixture;
    for installed in [false, true] {
        let fixture = Fixture::new();
        let (storage, details) = fixture.unsupported_instance("valheim");
        let runtime = fixture.0.join("runtime");
        let session = runtime.join(io::DIRECTORY).join(io::SESSION);
        let mut previous_receipt = None;
        if installed {
            install_owned_plugin(&runtime, b"previous plugin").unwrap();
            let mut owner = read_owner(&runtime).unwrap().unwrap();
            owner.pending_plugin_sha256 = Some(io::digest(b"pending plugin"));
            std::fs::write(runtime.join(io::PLUGIN), b"pending plugin").unwrap();
            let bytes = serde_json::to_vec(&owner).unwrap();
            std::fs::write(runtime.join(io::OWNER), &bytes).unwrap();
            previous_receipt = Some(bytes);
            std::fs::write(&session, b"existing session").unwrap();
            assert!(is_prepared(&details).unwrap());
        }
        let foreign = runtime.join("operator-mod.dll");
        std::fs::write(&foreign, b"operator mod").unwrap();
        let state = crate::state::DesktopState::default();
        let operation = state
            .begin_storage_context_operation("optional save test")
            .unwrap();
        for _ in 0..2 {
            assert!(
                ensure_for_start(&storage, &details, &operation)
                    .await
                    .unwrap_or_else(|error| panic!("installed={installed}: {error}"))
                    .is_none()
            );
            assert!(!runtime.join(io::PLUGIN).exists());
            assert!(!is_prepared(&details).unwrap());
            assert_eq!(
                std::fs::read(runtime.join(io::OWNER)).ok(),
                previous_receipt
            );
            assert_eq!(
                std::fs::read(&session).ok(),
                installed.then(|| b"existing session".to_vec())
            );
            assert_eq!(std::fs::read(&foreign).unwrap(), b"operator mod");
        }
        install_owned_plugin(&runtime, b"new reviewed plugin").unwrap();
        assert!(is_prepared(&details).unwrap());
        assert_eq!(
            std::fs::read(runtime.join(io::PLUGIN)).unwrap(),
            b"new reviewed plugin"
        );
    }
}

#[tokio::test]
async fn unsupported_valheim_start_preserves_foreign_modified_and_invalid_files() {
    use super::super::commands_managed_save_control::tests::Fixture;
    for problem in ["foreign", "modified", "invalid_owner", "missing_native"] {
        let fixture = Fixture::new();
        let (storage, details) = fixture.unsupported_instance("valheim");
        let runtime = fixture.0.join("runtime");
        if problem == "foreign" {
            std::fs::create_dir_all(runtime.join("BepInEx/plugins")).unwrap();
            std::fs::write(runtime.join(io::PLUGIN), b"operator mod").unwrap();
        } else {
            install_owned_plugin(&runtime, b"owned plugin").unwrap();
            match problem {
                "modified" => std::fs::write(runtime.join(io::PLUGIN), b"operator mod").unwrap(),
                "invalid_owner" => {
                    std::fs::write(runtime.join(io::OWNER), b"invalid receipt").unwrap()
                }
                _ => std::fs::remove_file(runtime.join(NATIVE_PREPARATION_FILES[2])).unwrap(),
            }
        }
        let before = [io::PLUGIN, io::OWNER].map(|path| std::fs::read(runtime.join(path)).ok());
        let state = crate::state::DesktopState::default();
        let operation = state
            .begin_storage_context_operation("optional save test")
            .unwrap();
        assert!(
            ensure_for_start(&storage, &details, &operation)
                .await
                .is_err(),
            "{problem}"
        );
        assert_eq!(
            [io::PLUGIN, io::OWNER].map(|path| std::fs::read(runtime.join(path)).ok()),
            before
        );
    }
}

#[test]
fn valheim_preparation_requires_its_owned_plugin_to_remain_present() {
    use super::super::commands_managed_save_control::tests::Fixture;
    let fixture = Fixture::new();
    let (_, details) = fixture.unsupported_instance("valheim");
    let runtime = fixture.0.join("runtime");
    install_owned_plugin(&runtime, b"owned plugin").unwrap();
    assert!(is_prepared(&details).unwrap());
    std::fs::remove_file(runtime.join(io::PLUGIN)).unwrap();
    assert!(!is_prepared(&details).unwrap());
    std::fs::write(runtime.join(io::PLUGIN), b"operator mod").unwrap();
    assert!(is_prepared(&details).is_err());
}

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "lgsm-valheim-control-install-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn preparation_preserves_unowned_plugins_and_directories() {
    for name in [io::PLUGIN, io::DIRECTORY] {
        let root = Scratch::new();
        let path = root.0.join(name);
        if name == io::PLUGIN {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, b"user plugin").unwrap();
        } else {
            std::fs::create_dir(&path).unwrap();
        }
        assert!(install_owned_plugin(&root.0, b"our plugin").is_err());
        assert!(!root.0.join(io::OWNER).exists());
        assert!(path.exists());
        if name == io::PLUGIN {
            assert_eq!(std::fs::read(path).unwrap(), b"user plugin");
        }
    }
}

#[test]
fn owned_preparation_recovers_and_never_replaces_a_modified_plugin() {
    let root = Scratch::new();
    install_owned_plugin(&root.0, b"first").unwrap();
    install_owned_plugin(&root.0, b"second").unwrap();
    assert_eq!(std::fs::read(root.0.join(io::PLUGIN)).unwrap(), b"second");
    let mut owner = read_owner(&root.0).unwrap().unwrap();
    assert_eq!(owner.plugin_sha256, Some(io::digest(b"second")));
    owner.pending_plugin_sha256 = Some(io::digest(b"third"));
    io::publish(
        &root.0.join(io::OWNER),
        &serde_json::to_vec(&owner).unwrap(),
    )
    .unwrap();
    std::fs::write(root.0.join(io::PLUGIN), b"third").unwrap();
    install_owned_plugin(&root.0, b"third").unwrap();
    assert!(
        read_owner(&root.0)
            .unwrap()
            .unwrap()
            .pending_plugin_sha256
            .is_none()
    );
    std::fs::write(root.0.join(io::PLUGIN), b"external replacement").unwrap();
    assert!(install_owned_plugin(&root.0, b"fourth").is_err());
    assert_eq!(
        std::fs::read(root.0.join(io::PLUGIN)).unwrap(),
        b"external replacement"
    );
}

#[test]
fn preparation_rejects_wrong_leaf_types_before_changing_owned_files() {
    for name in [io::DIRECTORY, io::PLUGIN, io::OWNER] {
        let root = Scratch::new();
        install_owned_plugin(&root.0, b"first").unwrap();
        let owner_before = std::fs::read(root.0.join(io::OWNER)).unwrap();
        let plugin_before = std::fs::read(root.0.join(io::PLUGIN)).unwrap();
        let path = root.0.join(name);
        if name == io::DIRECTORY {
            std::fs::remove_dir(&path).unwrap();
            std::fs::write(&path, b"user control file").unwrap();
        } else {
            std::fs::remove_file(&path).unwrap();
            std::fs::create_dir(&path).unwrap();
        }
        assert!(inspect_plugin_destination(&root.0).is_err(), "{name}");
        assert!(
            refresh_prepared_plugin(&root.0, b"replacement").is_err(),
            "{name}"
        );
        assert!(
            install_owned_plugin(&root.0, b"replacement").is_err(),
            "{name}"
        );
        if name != io::OWNER {
            assert_eq!(std::fs::read(root.0.join(io::OWNER)).unwrap(), owner_before);
        } else {
            assert!(path.is_dir());
            assert!(std::fs::read_dir(&path).unwrap().next().is_none());
            assert!(read_owner(&root.0).is_err());
        }
        if name != io::PLUGIN {
            assert_eq!(
                std::fs::read(root.0.join(io::PLUGIN)).unwrap(),
                plugin_before
            );
        } else {
            assert!(path.is_dir());
            assert!(std::fs::read_dir(&path).unwrap().next().is_none());
        }
        if name == io::DIRECTORY {
            assert_eq!(std::fs::read(path).unwrap(), b"user control file");
        }
    }
}

#[cfg(windows)]
fn create_test_junction(link: &Path, target: &Path) {
    use std::os::windows::process::CommandExt;
    let parent = dunce::canonicalize(link.parent().unwrap()).unwrap();
    let link = parent.join(link.file_name().unwrap());
    let target = dunce::canonicalize(target).unwrap();
    let output = std::process::Command::new("cmd.exe")
        .args(["/c", "mklink", "/J"])
        .arg(&link)
        .arg(&target)
        .creation_flags(0x0800_0000)
        .output()
        .expect("create isolated junction fixture");
    assert!(
        output.status.success(),
        "create isolated junction: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(windows)]
#[test]
fn preparation_rejects_an_owned_control_junction_before_upgrading_the_plugin() {
    let root = Scratch::new();
    let outside = Scratch::new();
    install_owned_plugin(&root.0, b"first").unwrap();
    let owner_before = std::fs::read(root.0.join(io::OWNER)).unwrap();
    let directory = root.0.join(io::DIRECTORY);
    std::fs::remove_dir(&directory).unwrap();
    std::fs::write(outside.0.join("user-data"), b"preserve outside").unwrap();
    create_test_junction(&directory, &outside.0);
    assert!(inspect_plugin_destination(&root.0).is_err());
    assert!(refresh_prepared_plugin(&root.0, b"replacement").is_err());
    assert!(install_owned_plugin(&root.0, b"replacement").is_err());
    assert_eq!(std::fs::read(root.0.join(io::OWNER)).unwrap(), owner_before);
    assert_eq!(std::fs::read(root.0.join(io::PLUGIN)).unwrap(), b"first");
    assert_eq!(
        std::fs::read(outside.0.join("user-data")).unwrap(),
        b"preserve outside"
    );
    assert_eq!(std::fs::read_dir(&outside.0).unwrap().count(), 1);
    assert!(io::reparse(&std::fs::symlink_metadata(&directory).unwrap()));
    std::fs::remove_dir(directory).unwrap();
}

#[cfg(windows)]
#[test]
fn preparation_preserves_dangling_plugin_control_and_owner_entries() {
    for name in [io::PLUGIN, io::DIRECTORY, io::OWNER] {
        let root = Scratch::new();
        let outside = Scratch::new();
        let path = root.0.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        create_test_junction(&path, &outside.0);
        std::fs::remove_dir(&outside.0).unwrap();
        assert!(
            !path.try_exists().unwrap(),
            "fixture must have a missing target"
        );
        assert!(io::reparse(&std::fs::symlink_metadata(&path).unwrap()));
        if name == io::OWNER {
            assert!(read_owner(&root.0).is_err());
        }
        assert!(inspect_plugin_destination(&root.0).is_err(), "{name}");
        assert!(
            install_owned_plugin(&root.0, b"replacement").is_err(),
            "{name}"
        );
        assert!(io::reparse(&std::fs::symlink_metadata(&path).unwrap()));
        assert!(!outside.0.exists());
        if name != io::OWNER {
            assert!(matches!(std::fs::symlink_metadata(root.0.join(io::OWNER)),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound));
        }
        std::fs::remove_dir(path).unwrap();
    }
}

#[cfg(unix)]
#[test]
fn preparation_preserves_dangling_symbolic_leaf_entries() {
    for name in [io::PLUGIN, io::DIRECTORY, io::OWNER] {
        let root = Scratch::new();
        let path = root.0.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink("missing-target", &path).unwrap();
        assert!(!path.try_exists().unwrap());
        if name == io::OWNER {
            assert!(read_owner(&root.0).is_err());
        }
        assert!(inspect_plugin_destination(&root.0).is_err(), "{name}");
        assert!(
            install_owned_plugin(&root.0, b"replacement").is_err(),
            "{name}"
        );
        assert!(
            std::fs::symlink_metadata(&path)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        if name != io::OWNER {
            assert!(matches!(std::fs::symlink_metadata(root.0.join(io::OWNER)),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound));
        }
        std::fs::remove_file(path).unwrap();
    }
}

#[cfg(windows)]
#[test]
fn preparation_rejects_a_junction_before_creating_any_plugin_directory() {
    let root = Scratch::new();
    let outside = Scratch::new();
    let link = root.0.join("BepInEx");
    let status = std::process::Command::new("cmd.exe")
        .args(["/c", "mklink", "/J"])
        .arg(&link)
        .arg(&outside.0)
        .output()
        .unwrap();
    assert!(status.status.success());
    assert!(inspect_plugin_destination(&root.0).is_err());
    assert!(install_owned_plugin(&root.0, b"our plugin").is_err());
    assert!(!outside.0.join("plugins").exists());
    assert!(!root.0.join(io::OWNER).exists());
    std::fs::remove_dir(link).unwrap();
}

fn preservation_sentinels(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    [
        "config/instance.json",
        "saves/adminlist.txt",
        "saves/worlds_local/World.fwl2",
        "winhttp.dll",
        "doorstop_config.ini",
    ]
    .into_iter()
    .map(|name| {
        let path = root.join(name);
        let bytes = format!("preserve unrelated {name}\0bytes").into_bytes();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        (path, bytes)
    })
    .collect()
}

fn assert_sentinels(files: &[(PathBuf, Vec<u8>)]) {
    for (path, bytes) in files {
        assert_eq!(&std::fs::read(path).unwrap(), bytes);
    }
}

fn assert_current_plugin(root: &Path, payload: &[u8]) {
    assert_eq!(std::fs::read(root.join(io::PLUGIN)).unwrap(), payload);
    let owner: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join(io::OWNER)).unwrap()).unwrap();
    assert_eq!(
        owner,
        serde_json::json!({"protocol":1,"module_id":"valheim","plugin_sha256":io::digest(payload)})
    );
}

#[test]
fn prepared_refresh_upgrades_owned_bytes_and_leaves_identical_payloads_untouched() {
    let root = Scratch::new();
    install_owned_plugin(&root.0, b"owned A").unwrap();
    let sentinels = preservation_sentinels(&root.0);
    refresh_prepared_plugin(&root.0, b"current C").unwrap();
    assert_current_plugin(&root.0, b"current C");
    assert_sentinels(&sentinels);
    let fixed = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
    let before = [io::OWNER, io::PLUGIN].map(|name| {
        let path = root.0.join(name);
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(fixed)
            .unwrap();
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        let bytes = std::fs::read(&path).unwrap();
        (path, modified, bytes)
    });
    refresh_prepared_plugin(&root.0, b"current C").unwrap();
    for (path, modified, bytes) in before {
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            modified,
            "identical owned payload must not be republished"
        );
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
    assert_sentinels(&sentinels);
}

#[test]
fn prepared_refresh_recovers_pending_old_or_new_bytes_and_clears_a_completed_intent() {
    for (installed, next) in [
        (b"owned A".as_slice(), b"current C".as_slice()),
        (b"pending B".as_slice(), b"current C".as_slice()),
        (b"pending B".as_slice(), b"pending B".as_slice()),
    ] {
        let root = Scratch::new();
        install_owned_plugin(&root.0, b"owned A").unwrap();
        let sentinels = preservation_sentinels(&root.0);
        let mut owner = read_owner(&root.0).unwrap().unwrap();
        owner.pending_plugin_sha256 = Some(io::digest(b"pending B"));
        io::publish(
            &root.0.join(io::OWNER),
            &serde_json::to_vec(&owner).unwrap(),
        )
        .unwrap();
        std::fs::write(root.0.join(io::PLUGIN), installed).unwrap();
        refresh_prepared_plugin(&root.0, next).unwrap();
        assert_current_plugin(&root.0, next);
        assert_sentinels(&sentinels);
    }
    let root = Scratch::new();
    install_owned_plugin(&root.0, b"owned A").unwrap();
    let sentinels = preservation_sentinels(&root.0);
    let mut owner = read_owner(&root.0).unwrap().unwrap();
    owner.pending_plugin_sha256 = Some(io::digest(b"pending B"));
    let owner_bytes = serde_json::to_vec(&owner).unwrap();
    io::publish(&root.0.join(io::OWNER), &owner_bytes).unwrap();
    std::fs::write(root.0.join(io::PLUGIN), b"unknown D").unwrap();
    assert!(refresh_prepared_plugin(&root.0, b"current C").is_err());
    assert_eq!(std::fs::read(root.0.join(io::OWNER)).unwrap(), owner_bytes);
    assert_eq!(
        std::fs::read(root.0.join(io::PLUGIN)).unwrap(),
        b"unknown D"
    );
    assert_sentinels(&sentinels);
}

#[test]
fn prepared_refresh_requires_every_existing_installation_component() {
    for missing in [
        "all",
        io::OWNER,
        io::PLUGIN,
        io::DIRECTORY,
        "BepInEx/plugins",
        "BepInEx",
    ] {
        let root = Scratch::new();
        if missing != "all" {
            install_owned_plugin(&root.0, b"owned A").unwrap();
            match missing {
                io::OWNER | io::PLUGIN => std::fs::remove_file(root.0.join(missing)).unwrap(),
                io::DIRECTORY => std::fs::remove_dir(root.0.join(missing)).unwrap(),
                _ => {
                    std::fs::remove_file(root.0.join(io::PLUGIN)).unwrap();
                    std::fs::remove_dir(root.0.join("BepInEx/plugins")).unwrap();
                    if missing == "BepInEx" {
                        std::fs::remove_dir(root.0.join(missing)).unwrap();
                    }
                }
            }
        }
        let sentinels = preservation_sentinels(&root.0);
        let existing = [io::OWNER, io::PLUGIN].map(|name| std::fs::read(root.0.join(name)).ok());
        let directories =
            [io::DIRECTORY, "BepInEx/plugins", "BepInEx"].map(|name| root.0.join(name).is_dir());
        assert!(
            refresh_prepared_plugin(&root.0, b"current C").is_err(),
            "{missing}"
        );
        assert_eq!(
            [io::OWNER, io::PLUGIN].map(|name| std::fs::read(root.0.join(name)).ok()),
            existing
        );
        assert_eq!(
            [io::DIRECTORY, "BepInEx/plugins", "BepInEx"].map(|name| root.0.join(name).is_dir()),
            directories
        );
        assert_sentinels(&sentinels);
    }
}

#[test]
fn prepared_refresh_preserves_modified_oversized_and_unusable_payloads() {
    for invalid in [
        "modified",
        "oversized-installed",
        "empty-payload",
        "oversized-payload",
    ] {
        let root = Scratch::new();
        install_owned_plugin(&root.0, b"owned A").unwrap();
        let sentinels = preservation_sentinels(&root.0);
        let oversized = vec![b'x'; 1024 * 1024 + 1];
        if invalid == "modified" {
            std::fs::write(root.0.join(io::PLUGIN), b"external edit").unwrap();
        }
        if invalid == "oversized-installed" {
            std::fs::write(root.0.join(io::PLUGIN), &oversized).unwrap();
        }
        let payload = match invalid {
            "empty-payload" => b"".as_slice(),
            "oversized-payload" => &oversized,
            _ => b"current C".as_slice(),
        };
        let owner = std::fs::read(root.0.join(io::OWNER)).unwrap();
        let plugin = std::fs::read(root.0.join(io::PLUGIN)).unwrap();
        assert!(
            refresh_prepared_plugin(&root.0, payload).is_err(),
            "{invalid}"
        );
        assert_eq!(std::fs::read(root.0.join(io::OWNER)).unwrap(), owner);
        let actual = std::fs::read(root.0.join(io::PLUGIN)).unwrap();
        assert_eq!(actual.len(), plugin.len());
        assert_eq!(io::digest(&actual), io::digest(&plugin));
        assert_sentinels(&sentinels);
    }

    let root = Scratch::new();
    let path = root.0.join("owned-cas");
    std::fs::write(&path, b"original").unwrap();
    let expected = std::fs::read(&path).unwrap();
    for modified in [b"changed!".as_slice(), b"changed and longer".as_slice()] {
        std::fs::write(&path, modified).unwrap();
        assert!(replace_prepared_file(&path, &expected, b"replacement").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), modified);
    }
    std::fs::remove_file(&path).unwrap();
    assert!(replace_prepared_file(&path, &expected, b"replacement").is_err());
    assert!(!path.exists());
    std::fs::write(&path, &expected).unwrap();
    replace_prepared_file(&path, &expected, b"replacement").unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"replacement");
}

fn snapshot(admin: bool) -> ControlSnapshot {
    ControlSnapshot {
        players: vec![ValidatedPeer {
            session_key: "a".repeat(32),
            name: "Raven".into(),
            host_id: "123".into(),
            platform_id: "Steam_123".into(),
            admin,
        }],
        access: AccessLists {
            admin: vec![],
            banned: vec![],
            permitted: vec![],
        },
        capabilities: std::collections::HashSet::new(),
        world_save: None,
    }
}

#[test]
fn role_success_requires_exact_native_list_and_online_privilege() {
    let mut native = snapshot(false);
    assert!(verify_role_state("admin_list", "Steam_123", Add, &native).is_err());
    native.access.admin.push("Steam_123".into());
    assert!(verify_role_state("admin_list", "Steam_123", Add, &native).is_err());
    native.players[0].admin = true;
    assert!(verify_role_state("admin_list", "Steam_123", Add, &native).is_ok());
    assert!(verify_role_state("admin_list", "steam_123", Add, &native).is_err());
    native.access.admin.clear();
    assert!(verify_role_state("admin_list", "Steam_123", Remove, &native).is_err());
    native.players[0].admin = false;
    assert!(verify_role_state("admin_list", "Steam_123", Remove, &native).is_ok());
    native.access.permitted.push("Steam_123".into());
    assert!(verify_role_state("permitted_list", "Steam_123", Add, &native).is_ok());
    assert!(verify_role_state("permitted_list", "Steam_123", Remove, &native).is_err());
    native.access.permitted.clear();
    assert!(verify_role_state("permitted_list", "Steam_123", Remove, &native).is_ok());
    assert!(verify_role_state("banned_list", "Steam_123", Add, &native).is_err());
}
