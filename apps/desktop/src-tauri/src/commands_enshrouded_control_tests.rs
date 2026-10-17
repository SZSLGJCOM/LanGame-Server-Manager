use super::*;

struct Scratch(std::path::PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "lgsm-enshrouded-install-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn unknown_executable_never_installs_a_hook() {
    let root = Scratch::new();
    fs::write(root.0.join("enshrouded_server.exe"), b"unknown build").unwrap();
    install(&root.0, b"our extension").unwrap();
    assert!(!root.0.join(DLL).exists());
    assert!(!root.0.join(io::OWNER).exists());
}

#[test]
fn user_loader_is_never_replaced() {
    let root = Scratch::new();
    fs::write(root.0.join(DLL), b"user loader").unwrap();
    install_verified(&root.0, b"our extension").unwrap();
    assert_eq!(fs::read(root.0.join(DLL)).unwrap(), b"user loader");
    assert!(!root.0.join(io::OWNER).exists());
}

#[test]
fn an_updated_unknown_executable_retires_only_the_owned_proxy() {
    let root = Scratch::new();
    install_verified(&root.0, b"our extension").unwrap();
    fs::write(
        root.0.join("enshrouded_server.exe"),
        b"updated unknown build",
    )
    .unwrap();
    install(&root.0, b"our extension").unwrap();
    assert!(!root.0.join(DLL).exists());
    assert!(root.0.join(io::OWNER).exists());
    fs::write(root.0.join(DLL), b"user replacement").unwrap();
    install(&root.0, b"our extension").unwrap();
    assert_eq!(fs::read(root.0.join(DLL)).unwrap(), b"user replacement");
}

#[test]
fn interrupted_owned_publication_is_recovered_before_enable() {
    let root = Scratch::new();
    install_verified(&root.0, b"our extension").unwrap();
    let temporary = root.0.join(io::DIRECTORY).join("players.json.tmp");
    fs::write(&temporary, b"interrupted packet").unwrap();
    install_verified(&root.0, b"our extension").unwrap();
    assert!(!temporary.exists());
    assert_eq!(
        fs::read(root.0.join(io::DIRECTORY).join("enable")).unwrap(),
        io::ENABLE
    );
}

#[test]
fn owned_installation_is_recoverable_and_idempotent() {
    let root = Scratch::new();
    install_verified(&root.0, b"first extension").unwrap();
    install_verified(&root.0, b"next extension").unwrap();
    install_verified(&root.0, b"next extension").unwrap();
    assert_eq!(fs::read(root.0.join(DLL)).unwrap(), b"next extension");
    assert_eq!(
        fs::read(root.0.join(io::DIRECTORY).join("enable")).unwrap(),
        io::ENABLE
    );
    let owner: Ownership =
        serde_json::from_slice(&fs::read(root.0.join(io::OWNER)).unwrap()).unwrap();
    assert_eq!(owner.plugin_sha256, Some(io::digest(b"next extension")));
    assert!(owner.pending_plugin_sha256.is_none());
    fs::write(root.0.join(DLL), b"external change").unwrap();
    install_verified(&root.0, b"next extension").unwrap();
    assert_eq!(fs::read(root.0.join(DLL)).unwrap(), b"external change");
}

#[test]
fn unowned_control_directory_is_preserved() {
    let root = Scratch::new();
    fs::create_dir(root.0.join(io::DIRECTORY)).unwrap();
    fs::write(root.0.join(io::DIRECTORY).join("keep"), b"data").unwrap();
    assert!(install_verified(&root.0, b"extension").is_err());
    assert_eq!(
        fs::read(root.0.join(io::DIRECTORY).join("keep")).unwrap(),
        b"data"
    );
    assert!(!root.0.join(DLL).exists());
}

#[test]
fn optional_preparation_failure_preserves_a_user_loader_and_disables_moderation() {
    let root = Scratch::new();
    fs::write(root.0.join(DLL), b"user loader").unwrap();
    // Missing executable makes preparation fail. The optional setup reports the
    // issue as a value, preserving the ordinary game-start control flow.
    let issue = prepare_optional_install(&root.0, &"f".repeat(64), None);
    assert!(issue.is_some());
    assert_eq!(fs::read(root.0.join(DLL)).unwrap(), b"user loader");
    let state: serde_json::Value = serde_json::from_slice(
        &fs::read(root.0.join(".langame-enshrouded-control-state.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(state["extension_status"], "not_installed");
}

#[test]
fn optional_preparation_failure_retires_only_the_verified_owned_proxy() {
    let root = Scratch::new();
    install_verified(&root.0, b"our extension").unwrap();
    assert!(prepare_optional_install(&root.0, &"f".repeat(64), None).is_some());
    assert!(!root.0.join(DLL).exists());
    assert!(root.0.join(io::OWNER).exists());
}
