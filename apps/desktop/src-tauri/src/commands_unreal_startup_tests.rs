use super::*;
use std::fs;

struct Fixture(PathBuf);

impl Fixture {
    fn new(module: &str, project: &str, executable: &str) -> (Self, InstanceDetails) {
        let root = std::env::temp_dir().join(format!("lgsm-unreal-start-{}", uuid::Uuid::new_v4()));
        let runtime = root.join("runtime");
        let binaries = runtime.join(project).join("Binaries/Win64");
        fs::create_dir_all(&binaries).unwrap();
        fs::write(runtime.join(".langame-private-runtime"), b"managed\n").unwrap();
        fs::write(
            binaries.join(executable),
            b"unknown game image; never executed",
        )
        .unwrap();
        if module == "conanexiles" {
            fs::write(binaries.join("Dreamworld.dll"), b"unknown native provider").unwrap();
        }
        let details = serde_json::from_value(serde_json::json!({
            "summary": {"id":"unreal-start", "name":"Unreal startup", "module_id":module,
                "status":"Stopped", "bind_ip":"127.0.0.1", "port_count":0, "autostart":false},
            "config_file_path":root.join("config/instance.json"),
            "saves_path":root.join("data/worlds"), "auto_backup_on_stop":false,
            "backup_retention_count":0, "settings_json":"{}", "ports":[], "active_run":null
        }))
        .unwrap();
        (Self(root), details)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        assert_eq!(self.0.parent(), Some(std::env::temp_dir().as_path()));
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[tokio::test]
async fn unsupported_unreal_start_keeps_clean_games_native_without_downloading() {
    let state = DesktopState::default();
    let operation = state
        .begin_storage_context_operation("unknown Unreal build")
        .unwrap();
    for (module, project, executable) in [
        (
            "abioticfactor",
            "AbioticFactor",
            "AbioticFactorServer-Win64-Shipping.exe",
        ),
        ("nightingale", "NWX", "NWXServer-Win64-Shipping.exe"),
        ("returntomoria", "Moria", "MoriaServer-Win64-Shipping.exe"),
        (
            "conanexiles",
            "ConanSandbox",
            "ConanSandboxServer-Win64-Shipping.exe",
        ),
        ("scum", "SCUM", "SCUMServer.exe"),
        ("windrose", "R5", "WindroseServer-Win64-Shipping.exe"),
    ] {
        let (fixture, details) = Fixture::new(module, project, executable);
        let result = ensure_unreal(&details, &operation).await;
        assert!(matches!(result, Ok(None)), "{module}: {result:?}");
        let runtime = fixture.0.join("runtime");
        assert!(!runtime.join(project).join("Binaries/Win64/ue4ss").exists());
        assert!(!runtime.join("langame_world_save").exists());
        // Explicit save installation remains strict; only native launch is optional.
        assert!(crate::unreal_loader::installed(&runtime, module).is_err());
    }
}

#[tokio::test]
async fn unsupported_unreal_start_still_requires_native_program_files() {
    let state = DesktopState::default();
    let operation = state
        .begin_storage_context_operation("invalid Unreal program")
        .unwrap();
    let (fixture, moria) =
        Fixture::new("returntomoria", "Moria", "MoriaServer-Win64-Shipping.exe");
    fs::remove_file(
        fixture
            .0
            .join("runtime/Moria/Binaries/Win64/MoriaServer-Win64-Shipping.exe"),
    )
    .unwrap();
    assert!(ensure_unreal(&moria, &operation).await.is_err());
    let (fixture, details) = Fixture::new(
        "conanexiles",
        "ConanSandbox",
        "ConanSandboxServer-Win64-Shipping.exe",
    );
    let runtime = fixture.0.join("runtime");
    fs::remove_file(runtime.join("ConanSandbox/Binaries/Win64/Dreamworld.dll")).unwrap();
    assert!(ensure_unreal(&details, &operation).await.is_err());
    assert!(!runtime.join("ConanSandbox/Binaries/Win64/ue4ss").exists());
}
