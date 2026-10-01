use std::fs;
use std::path::{Path, PathBuf};

use uuid::Uuid;

use super::{PrivateRuntimeProjection, prepare_private_runtime_projection};

#[test]
fn projection_copies_package_files_and_keeps_private_directories_empty() {
    let root = unique_test_root();
    let shared = root.join("shared");
    let instance = root.join("instance");
    let shared_saved = shared.join("RSDragonwilds/Saved");
    fs::create_dir_all(shared.join("Content/Paks")).unwrap();
    fs::create_dir_all(shared_saved.join("Config/WindowsServer")).unwrap();
    fs::write(shared.join("RSDragonwildsServer.exe"), b"shared executable").unwrap();
    fs::write(
        shared.join("Content/Paks/server.pak"),
        b"large shared payload",
    )
    .unwrap();
    fs::write(
        shared_saved.join("Config/WindowsServer/DedicatedServer.ini"),
        b"OwnerId=shared-sentinel\n",
    )
    .unwrap();

    let runtime = prepare_private_runtime_projection(
        &shared,
        &instance,
        &PrivateRuntimeProjection {
            private_directories: vec![PathBuf::from("RSDragonwilds/Saved")],
        },
        None,
        None,
    )
    .unwrap();

    let projected_executable = runtime.join("RSDragonwildsServer.exe");
    let projected_pak = runtime.join("Content/Paks/server.pak");
    assert_independent_file(
        &shared.join("RSDragonwildsServer.exe"),
        &projected_executable,
    );
    assert_independent_file(&shared.join("Content/Paks/server.pak"), &projected_pak);
    assert!(runtime.join("RSDragonwilds/Saved").is_dir());
    assert!(
        !runtime
            .join("RSDragonwilds/Saved/Config/WindowsServer/DedicatedServer.ini")
            .exists()
    );

    fs::write(
        runtime.join("RSDragonwilds/Saved/private.txt"),
        b"lane only",
    )
    .unwrap();
    assert!(!shared_saved.join("private.txt").exists());
    assert_eq!(
        fs::read_to_string(shared_saved.join("Config/WindowsServer/DedicatedServer.ini")).unwrap(),
        "OwnerId=shared-sentinel\n"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn projection_rejects_unsafe_private_directories_before_creating_runtime() {
    let root = unique_test_root();
    let shared = root.join("shared");
    let instance = root.join("instance");
    fs::create_dir_all(&shared).unwrap();

    let error = prepare_private_runtime_projection(
        &shared,
        &instance,
        &PrivateRuntimeProjection {
            private_directories: vec![PathBuf::from("../outside")],
        },
        None,
        None,
    )
    .unwrap_err();

    assert!(error.to_string().contains("outside"));
    assert!(!instance.join("runtime").exists());
    assert!(!instance.join("runtime.staging").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_present_runtime_requires_an_authentic_private_marker() {
    let root = unique_test_root();
    fs::create_dir_all(&root).unwrap();
    assert!(super::resolve_instance_private_runtime_root(&root).is_err());
    let runtime = root.join("runtime");
    fs::create_dir(&runtime).unwrap();
    let marker = runtime.join(super::PRIVATE_RUNTIME_MARKER);
    assert!(super::resolve_instance_private_runtime_root(&root).is_err());
    fs::write(&marker, b"invalid marker").unwrap();
    assert!(super::resolve_instance_private_runtime_root(&root).is_err());
    fs::write(&marker, b"managed\n").unwrap();
    assert_eq!(
        super::resolve_instance_private_runtime_root(&root).unwrap(),
        runtime.clone()
    );
    fs::remove_file(&marker).unwrap();
    assert!(super::resolve_instance_private_runtime_root(&root).is_err());
    fs::remove_dir_all(root).unwrap();
}
fn assert_independent_file(source: &Path, projected: &Path) {
    let original = fs::read(source).unwrap();
    fs::write(source, b"shared update").unwrap();
    assert_eq!(fs::read(projected).unwrap(), original);
    fs::write(projected, b"instance change").unwrap();
    assert_eq!(fs::read(source).unwrap(), b"shared update");
}

fn unique_test_root() -> PathBuf {
    std::env::temp_dir().join(format!("langame-private-runtime-{}", Uuid::new_v4()))
}
