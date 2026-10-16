use super::*;
use crate::live_players::valheim_control::{AccessLists, ControlSnapshot, ValidatedPeer};
use app_storage::PlayerAccessMutationOperation::{Add, Remove};

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
