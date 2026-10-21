use super::*;

struct Root(std::path::PathBuf);
impl Root {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lgsm-dragonwilds-control-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        // This fixture owns exactly the fresh UUID child it created.
        if self.0.parent() == Some(std::env::temp_dir().as_path())
            && self
                .0
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("lgsm-dragonwilds-control-")
        {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
}

#[test]
fn dragonwilds_control_install_is_idempotent_and_upgrades_only_owned_files() {
    let root = Root::new();
    install(&root.0, b"script-one", b"MZ-bridge-one").unwrap();
    assert!(is_complete(&root.0, b"script-one", b"MZ-bridge-one").unwrap());
    assert!(!is_complete(&root.0, b"script-next", b"MZ-bridge-next").unwrap());
    let receipt = fs::read(root.0.join(OWNER)).unwrap();
    install(&root.0, b"script-one", b"MZ-bridge-one").unwrap();
    assert_eq!(fs::read(root.0.join(OWNER)).unwrap(), receipt);
    install(&root.0, b"script-two", b"MZ-bridge-two").unwrap();
    assert_eq!(
        fs::read(root.0.join(MOD).join("Scripts/main.lua")).unwrap(),
        b"script-two"
    );
    assert_eq!(
        fs::read(root.0.join(MOD).join("native/LgsmPlayerControl.dll")).unwrap(),
        b"MZ-bridge-two"
    );
}

#[test]
fn dragonwilds_control_install_preserves_foreign_paths_and_tampered_owned_files() {
    let foreign = Root::new();
    fs::create_dir_all(foreign.0.join(MOD)).unwrap();
    fs::write(foreign.0.join(MOD).join("user-data"), b"preserve").unwrap();
    assert!(install(&foreign.0, b"script", b"MZ-bridge").is_err());
    assert!(!foreign.0.join(OWNER).exists());
    assert_eq!(
        fs::read(foreign.0.join(MOD).join("user-data")).unwrap(),
        b"preserve"
    );

    let root = Root::new();
    install(&root.0, b"script", b"MZ-bridge").unwrap();
    let receipt = fs::read(root.0.join(OWNER)).unwrap();
    fs::write(root.0.join(MOD).join("Scripts/main.lua"), b"user-modified").unwrap();
    assert!(is_complete(&root.0, b"script", b"MZ-bridge").is_err());
    assert!(install(&root.0, b"updated", b"MZ-next").is_err());
    assert_eq!(fs::read(root.0.join(OWNER)).unwrap(), receipt);
    assert_eq!(
        fs::read(root.0.join(MOD).join("Scripts/main.lua")).unwrap(),
        b"user-modified"
    );
}

#[test]
fn dragonwilds_control_install_recovers_an_owned_partial_publication_across_app_builds() {
    let root = Root::new();
    install(&root.0, b"script-one", b"MZ-bridge-one").unwrap();
    let mut owner: Ownership =
        serde_json::from_slice(&fs::read(root.0.join(OWNER)).unwrap()).unwrap();
    owner.pending_files = Some(std::mem::take(&mut owner.files));
    fs::write(root.0.join(OWNER), serde_json::to_vec(&owner).unwrap()).unwrap();
    assert!(is_complete(&root.0, b"script-one", b"MZ-bridge-one").is_err());
    fs::remove_file(root.0.join(MOD).join("Scripts/main.lua")).unwrap();
    fs::remove_file(root.0.join(MOD).join("enabled.txt")).unwrap();
    install(&root.0, b"script-two", b"MZ-different-app-build").unwrap();
    let completed: Ownership =
        serde_json::from_slice(&fs::read(root.0.join(OWNER)).unwrap()).unwrap();
    assert!(completed.pending_files.is_none());
    assert_eq!(completed.files.len(), 4);
    assert_eq!(
        fs::read(root.0.join(MOD).join("native/LgsmPlayerControl.dll")).unwrap(),
        b"MZ-different-app-build"
    );
}

#[test]
fn dragonwilds_control_install_refuses_nonfiles_and_unknown_intent_paths() {
    let root = Root::new();
    install(&root.0, b"script", b"MZ-bridge").unwrap();
    let mut owner: Ownership =
        serde_json::from_slice(&fs::read(root.0.join(OWNER)).unwrap()).unwrap();
    owner.files.insert("../unowned".into(), hash(b"data"));
    fs::write(root.0.join(OWNER), serde_json::to_vec(&owner).unwrap()).unwrap();
    assert!(install(&root.0, b"updated", b"MZ-updated").is_err());
    assert_eq!(
        fs::read(root.0.join(MOD).join("Scripts/main.lua")).unwrap(),
        b"script"
    );
}

#[test]
fn dragonwilds_control_session_is_fresh_pinned_and_invalid_existing_data_is_preserved() {
    let root = Root::new();
    install(&root.0, b"script", b"MZ-bridge").unwrap();
    let (key, first) = start_session(&root.0).unwrap();
    assert_eq!(key, "LGSM_DRAGON_SAVE_RUN");
    assert!(super::super::nonce(&first));
    let (observed, pins) = session(&root.0).unwrap();
    assert_eq!(first, observed);
    #[cfg(windows)]
    assert!(start_session(&root.0).is_err());
    drop(pins);
    let (_, next) = start_session(&root.0).unwrap();
    assert_ne!(first, next);
    let path = root.0.join(DIRECTORY).join(SESSION);
    fs::write(&path, b"operator data").unwrap();
    assert!(start_session(&root.0).is_err());
    assert_eq!(fs::read(path).unwrap(), b"operator data");
}

#[test]
fn dragonwilds_control_upgrade_deploys_license_without_adopting_a_foreign_file() {
    for foreign in [false, true] {
        let root = Root::new();
        install(&root.0, b"script", b"MZ-bridge").unwrap();
        let mut owner: Ownership =
            serde_json::from_slice(&fs::read(root.0.join(OWNER)).unwrap()).unwrap();
        owner.files.remove("LICENSE-MinHook.txt");
        fs::write(root.0.join(OWNER), serde_json::to_vec(&owner).unwrap()).unwrap();
        let notice = root.0.join(MOD).join("LICENSE-MinHook.txt");
        if foreign {
            fs::write(&notice, b"external content").unwrap();
        } else {
            fs::remove_file(&notice).unwrap();
        }
        let result = install(&root.0, b"script-next", b"MZ-bridge-next");
        if foreign {
            assert!(result.is_err());
            assert_eq!(fs::read(notice).unwrap(), b"external content");
        } else {
            result.unwrap();
            assert_eq!(fs::read(notice).unwrap(), NOTICE);
            assert!(is_complete(&root.0, b"script-next", b"MZ-bridge-next").unwrap());
        }
    }
}
