use super::*;

struct StartupRoot(std::path::PathBuf);
impl StartupRoot {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lgsm-dragonwilds-startup-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(root.join("runtime")).unwrap();
        std::fs::write(root.join("runtime/.langame-private-runtime"), b"managed\n").unwrap();
        Self(root)
    }

    fn details(&self) -> InstanceDetails {
        serde_json::from_value(serde_json::json!({
            "summary":{"id":"dragon-save","name":"Dragon save","module_id":"runescapedragonwilds",
                "status":"Stopped","bind_ip":"127.0.0.1","port_count":0,"autostart":false},
            "config_file_path":self.0.join("config/instance.json"),"saves_path":self.0.join("data/worlds"),
            "auto_backup_on_stop":false,"backup_retention_count":0,"settings_json":"{}","ports":[],"active_run":null
        })).unwrap()
    }
}
impl Drop for StartupRoot {
    fn drop(&mut self) {
        assert_eq!(self.0.parent(), Some(std::env::temp_dir().as_path()));
        assert!(
            self.0
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("lgsm-dragonwilds-startup-")
        );
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn dragonwilds_stopped_start_creates_the_missing_native_save_session() {
    let root = StartupRoot::new();
    let runtime = root.0.join("runtime");
    let (script, bridge) = bundled_payload();
    installation::install(&runtime, script, bridge).unwrap();
    assert!(installation::session(&runtime).is_err());
    let details = root.details();
    let (key, run) = startup_environment(&details).unwrap();
    assert_eq!(key, "LGSM_DRAGON_SAVE_RUN");
    assert!(nonce(&run));
    let (observed, pins) = installation::session(&runtime).unwrap();
    assert_eq!(observed, run);
    drop(pins);
    for image in [
        runtime.join("RSDragonwildsServer.exe"),
        runtime.join("RSDragonwilds/Binaries/Win64/RSDragonwildsServer-Win64-Shipping.exe"),
    ] {
        let native_root = Game::Dragonwilds.install_root(&image).unwrap();
        assert_eq!(native_root, runtime);
        let (native_run, pins) = installation::session(&native_root).unwrap();
        assert_eq!(native_run, run);
        drop(pins);
    }
    let (_, next) = startup_environment(&details).unwrap();
    assert_ne!(run, next);
}

#[test]
fn dragonwilds_start_preserves_external_control_and_missing_private_marker() {
    let root = StartupRoot::new();
    let runtime = root.0.join("runtime");
    let details = root.details();
    assert!(
        startup_environment(&details)
            .unwrap_err()
            .contains("must be prepared")
    );
    let (script, bridge) = bundled_payload();
    installation::install(&runtime, script, bridge).unwrap();
    let script_path =
        runtime.join("RSDragonwilds/Binaries/Win64/ue4ss/Mods/LgsmPlayerControl/Scripts/main.lua");
    std::fs::write(&script_path, b"external modification").unwrap();
    assert!(startup_environment(&details).is_err());
    assert_eq!(
        std::fs::read(&script_path).unwrap(),
        b"external modification"
    );
    assert!(installation::session(&runtime).is_err());
    std::fs::remove_file(runtime.join(".langame-private-runtime")).unwrap();
    assert!(startup_environment(&details).is_err());
    assert!(installation::session(&runtime).is_err());
}
