use super::*;

struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "theforest-control-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(root.join("TheForestDedicatedServer_Data/Managed")).unwrap();
        fs::write(root.join(GAME_ASSEMBLY), b"fixture game assembly").unwrap();
        Self { root }
    }
    fn install(&self, payload: &[u8]) -> Result<(), String> {
        install_files(&self.root, payload, &digest(b"fixture game assembly"))
    }
    fn plugin(&self) -> PathBuf {
        self.root.join("DllMods").join(PLUGIN)
    }
    fn owner(&self) -> PathBuf {
        self.root.join("DllMods").join(OWNER)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn theforest_control_installs_and_reuses_only_exact_embedded_payload() {
    let fixture = Fixture::new();
    fixture.install(b"owned bridge").unwrap();
    fixture.install(b"owned bridge").unwrap();
    assert_eq!(fs::read(fixture.plugin()).unwrap(), b"owned bridge");
    let owner: Ownership = serde_json::from_slice(&fs::read(fixture.owner()).unwrap()).unwrap();
    assert_eq!(owner.plugin_sha256, Some(digest(b"owned bridge")));
    assert!(owner.pending_plugin_sha256.is_none());
}

#[test]
fn theforest_control_rejects_unowned_conflicting_dll_without_mutation() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.root.join("DllMods")).unwrap();
    fs::write(fixture.plugin(), b"user mod").unwrap();
    assert!(fixture.install(b"owned bridge").is_err());
    assert_eq!(fs::read(fixture.plugin()).unwrap(), b"user mod");
    assert!(!fixture.owner().exists());
}

#[test]
fn theforest_control_rejects_changed_owned_dll_without_overwriting_user_content() {
    let fixture = Fixture::new();
    fixture.install(b"old owned bridge").unwrap();
    fs::write(fixture.plugin(), b"user replacement").unwrap();
    let owner_before = fs::read(fixture.owner()).unwrap();
    assert!(fixture.install(b"new owned bridge").is_err());
    assert_eq!(fs::read(fixture.plugin()).unwrap(), b"user replacement");
    assert_eq!(fs::read(fixture.owner()).unwrap(), owner_before);
}

#[test]
fn theforest_control_updates_verified_owner_and_recovers_interrupted_publication() {
    let fixture = Fixture::new();
    fixture.install(b"old owned bridge").unwrap();
    fixture.install(b"new owned bridge").unwrap();
    assert_eq!(fs::read(fixture.plugin()).unwrap(), b"new owned bridge");
    for dll_already_published in [false, true] {
        let pending = Ownership {
            schema: 1,
            module_id: "theforest".into(),
            plugin_sha256: Some(digest(b"new owned bridge")),
            pending_plugin_sha256: Some(digest(b"next owned bridge")),
        };
        fs::write(fixture.owner(), serde_json::to_vec(&pending).unwrap()).unwrap();
        fs::write(
            fixture.plugin(),
            if dll_already_published {
                b"next owned bridge".as_slice()
            } else {
                b"new owned bridge".as_slice()
            },
        )
        .unwrap();
        fixture.install(b"next owned bridge").unwrap();
        assert_eq!(fs::read(fixture.plugin()).unwrap(), b"next owned bridge");
    }
}

#[test]
fn theforest_control_rejects_unknown_game_and_foreign_ownership() {
    let fixture = Fixture::new();
    assert!(install_files(&fixture.root, b"bridge", &digest(b"other game")).is_err());
    assert!(!fixture.root.join("DllMods").exists());
    fixture.install(b"owned bridge").unwrap();
    let foreign = Ownership {
        schema: 1,
        module_id: "other-game".into(),
        plugin_sha256: Some(digest(b"owned bridge")),
        pending_plugin_sha256: None,
    };
    fs::write(fixture.owner(), serde_json::to_vec(&foreign).unwrap()).unwrap();
    assert!(fixture.install(b"new bridge").is_err());
    assert_eq!(fs::read(fixture.plugin()).unwrap(), b"owned bridge");
}

#[test]
fn theforest_control_rejects_misplaced_saves_without_mutation() {
    let fixture = Fixture::new();
    let saves = fixture.root.join("saves");
    verify_save_layout(&saves).unwrap();
    for suffix in ["Multiplayer", "SinglePlayer"] {
        let misplaced = fixture.root.join(format!("saves{suffix}"));
        fs::create_dir(&misplaced).unwrap();
        verify_save_layout(&saves).unwrap();
        let checkpoint = misplaced.join("__RESUME__");
        fs::write(&checkpoint, b"retained native checkpoint").unwrap();
        assert!(verify_save_layout(&saves).is_err());
        assert_eq!(
            fs::read(&checkpoint).unwrap(),
            b"retained native checkpoint"
        );
        assert!(!saves.exists());
        fs::remove_file(checkpoint).unwrap();
        fs::remove_dir(&misplaced).unwrap();
        fs::write(&misplaced, b"unexpected non-directory").unwrap();
        assert!(verify_save_layout(&saves).is_err());
        assert_eq!(fs::read(&misplaced).unwrap(), b"unexpected non-directory");
        fs::remove_file(misplaced).unwrap();
    }
}

#[cfg(windows)]
#[test]
fn theforest_control_rejects_reparse_save_layout_without_touching_target() {
    use std::os::windows::process::CommandExt;
    let fixture = Fixture::new();
    let outside = Fixture::new();
    let root = dunce::canonicalize(&fixture.root).unwrap();
    let target = dunce::canonicalize(&outside.root).unwrap();
    let link = root.join("savesMultiplayer");
    let output = std::process::Command::new("cmd.exe")
        .args(["/D", "/C", "mklink", "/J"])
        .arg(&link)
        .arg(&target)
        .creation_flags(0x0800_0000)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "junction fixture failed: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(verify_save_layout(&root.join("saves")).is_err());
    assert_eq!(
        fs::read(target.join(GAME_ASSEMBLY)).unwrap(),
        b"fixture game assembly"
    );
    assert!(!root.join("saves").exists());
    fs::remove_dir(link).unwrap();
}

#[cfg(windows)]
#[test]
fn theforest_control_rejects_reparse_destination_without_touching_target() {
    use std::os::windows::process::CommandExt;
    let fixture = Fixture::new();
    let outside = Fixture::new();
    let root = dunce::canonicalize(&fixture.root).unwrap();
    let target = dunce::canonicalize(&outside.root).unwrap();
    let link = root.join("DllMods");
    let output = std::process::Command::new("cmd.exe")
        .args(["/D", "/C", "mklink", "/J"])
        .arg(&link)
        .arg(&target)
        .creation_flags(0x0800_0000)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "junction fixture failed: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(fixture.install(b"bridge").is_err());
    assert!(!target.join(PLUGIN).exists());
    fs::remove_dir(link).unwrap();
}
