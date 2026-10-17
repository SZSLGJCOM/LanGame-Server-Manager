use super::*;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lgsm-runtime-environment-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        assert_eq!(self.0.parent(), Some(std::env::temp_dir().as_path()));
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn minecraft(settings: &AppSettings, root: &Path) -> ModuleDetails {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap();
    let descriptor = discover_modules(workspace.join("modules"))
        .unwrap()
        .into_iter()
        .find(|module| module.summary.id == "minecraft")
        .unwrap();
    map_module_details_with_install_state(settings, &descriptor, Some(&root.to_string_lossy()))
}

#[test]
fn repaired_java_changes_entrypoint_without_changing_game_program_or_install_contract() {
    let fixture = Fixture::new();
    let game = fixture.0.join("game");
    fs::create_dir_all(&game).unwrap();
    fs::write(game.join("server.jar"), b"pinned server bytes").unwrap();
    let settings = AppSettings::default();
    let mut module = minecraft(&settings, &game);
    // server.jar is the package's install verification file. Its presence can
    // report Installed even when the separately required Java entry is absent.
    assert!(
        !game
            .join(&module.process.as_ref().unwrap().executable)
            .is_file()
    );
    let java = fixture.0.join("managed-java.exe");
    fs::write(&java, b"probe-verified fixture").unwrap();
    apply_portable_executable(
        &settings,
        &game,
        &mut module,
        &app_steamcmd::PortableRuntime {
            executable: Some(java.clone()),
            environment: Default::default(),
        },
    )
    .unwrap();
    assert!(matches!(
        module.summary.install_state,
        InstallState::Installed
    ));
    assert_eq!(
        module.process.as_ref().unwrap().executable,
        java.to_string_lossy()
    );
    assert_eq!(
        fs::read(game.join("server.jar")).unwrap(),
        b"pinned server bytes"
    );
    assert!(!game.join("jre").exists());
}

#[test]
fn valid_java_cannot_hide_a_missing_server_jar() {
    let fixture = Fixture::new();
    let game = fixture.0.join("game");
    fs::create_dir_all(&game).unwrap();
    let java = fixture.0.join("java.exe");
    fs::write(&java, b"verified runtime fixture").unwrap();
    let settings = AppSettings::default();
    let mut module = minecraft(&settings, &game);
    apply_portable_executable(
        &settings,
        &game,
        &mut module,
        &app_steamcmd::PortableRuntime {
            executable: Some(java),
            environment: Default::default(),
        },
    )
    .unwrap();
    assert!(!matches!(
        module.summary.install_state,
        InstallState::Installed
    ));
    assert!(ensure_module_installed_for_start(&module).is_err());
}

#[test]
fn scripts_have_their_declared_binary_inspected_without_evaluation_or_path_escape() {
    let settings = AppSettings::default();
    let fixture = Fixture::new();
    let mut module = minecraft(&settings, &fixture.0);
    module.install.as_mut().unwrap().verification_path = Some("Server.exe".into());
    assert_eq!(
        runtime_executables(&module, &fixture.0, &[]),
        [fixture.0.join("Server.exe")]
    );
    for unsafe_path in ["../outside.exe", "C:/outside.exe", "server.jar"] {
        module.install.as_mut().unwrap().verification_path = Some(unsafe_path.into());
        assert!(runtime_executables(&module, &fixture.0, &[]).is_empty());
    }
}
