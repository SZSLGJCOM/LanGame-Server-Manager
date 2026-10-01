use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

static TEST_ROOT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(super) fn unique_test_root() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("test clock")
        .as_nanos();
    let sequence = TEST_ROOT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("lgsm-{}-{stamp}-{sequence}", std::process::id()))
}

fn settings_with_steamcmd_root(root: &Path) -> AppSettings {
    AppSettings {
        archives_root: String::new(),
        servers_root: root.join("servers").to_string_lossy().into_owned(),
        games_root: root.join("games").to_string_lossy().into_owned(),
        modules_root: root.join("modules").to_string_lossy().into_owned(),
        steamcmd_root: root.to_string_lossy().into_owned(),
    }
}

#[test]
fn minecraft_file_publish_replaces_existing_file_without_staging_debris() {
    let root = unique_test_root();
    fs::create_dir_all(&root).expect("create publish test root");
    let server_jar = root.join("server.jar");
    fs::write(&server_jar, b"old jar").expect("write old jar");

    write_minecraft_server_file_atomically(&server_jar, b"new jar")
        .expect("publish replacement jar");

    assert_eq!(
        fs::read(&server_jar).expect("read published jar"),
        b"new jar"
    );
    assert_eq!(
        fs::read_dir(&root).expect("read publish root").count(),
        1,
        "successful publish must remove staging and rollback files"
    );
    fs::remove_dir_all(root).expect("remove publish test root");
}

#[test]
fn minecraft_file_publish_restores_previous_file_when_replacement_fails() {
    let root = unique_test_root();
    fs::create_dir_all(&root).expect("create rollback test root");
    let server_jar = root.join("server.jar");
    fs::write(&server_jar, b"old jar").expect("write old jar");

    let result = write_minecraft_server_file_atomically_with(
        &server_jar,
        b"new jar",
        |destination, _replacement, backup| {
            fs::rename(destination, backup)?;
            Err(std::io::Error::other("injected replacement failure"))
        },
    );

    assert!(matches!(
        result,
        Err(SteamCmdError::WriteMinecraftServerFile { .. })
    ));
    assert_eq!(
        fs::read(&server_jar).expect("read restored jar"),
        b"old jar"
    );
    assert_eq!(
        fs::read_dir(&root).expect("read rollback root").count(),
        1,
        "failed publish must restore the old jar and remove staging files"
    );
    fs::remove_dir_all(root).expect("remove rollback test root");
}

#[test]
fn staged_game_install_removal_path_requires_a_matching_sibling() {
    let root = unique_test_root();
    let published = root.join("rimworld");
    let valid = root.join(".rimworld.uninstall-42-100");

    validate_staged_game_install_removal_path(&published, &valid)
        .expect("matching sibling staging directory is safe");
    assert!(matches!(
        validate_staged_game_install_removal_path(&published, &root.join("other")),
        Err(SteamCmdError::UnsafeGameInstallRemovalPath { .. })
    ));
    assert!(matches!(
        validate_staged_game_install_removal_path(
            &published,
            &root.join("nested").join(".rimworld.uninstall-42-100")
        ),
        Err(SteamCmdError::UnsafeGameInstallRemovalPath { .. })
    ));
}

#[tokio::test]
async fn game_install_lifecycle_guard_removes_a_valid_staged_directory() {
    let root = unique_test_root();
    let published = root.join("rimworld");
    let staged = root.join(".rimworld.uninstall-42-100");
    fs::create_dir_all(&staged).unwrap();
    fs::write(staged.join("server.sentinel"), b"fixture").unwrap();
    let operation =
        acquire_game_install_lifecycle("staged-deletion-fixture", std::slice::from_ref(&published))
            .await
            .expect("acquire lifecycle operation");

    operation
        .remove_staged_directory(published, staged.clone())
        .await
        .expect("remove staged game directory");

    assert!(!staged.exists());
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn steamcmd_uninstall_rejects_unmarked_root() {
    let root = unique_test_root();
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("steamcmd.exe"), b"fixture").unwrap();
    fs::write(root.join("personal-file.txt"), b"keep").unwrap();
    let settings = settings_with_steamcmd_root(&root);
    let status = managed_steamcmd_status(&settings);
    assert_eq!(status.ownership, SteamCmdOwnership::External);
    assert!(!status.can_uninstall);
    assert!(matches!(
        remove_managed_steamcmd(&settings).await,
        Err(SteamCmdError::UnmanagedSteamCmdRoot { .. })
    ));
    assert!(root.join("steamcmd.exe").is_file());
    assert_eq!(fs::read(root.join("personal-file.txt")).unwrap(), b"keep");
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn steamcmd_uninstall_removes_matching_marked_root() {
    let root = unique_test_root();
    fs::create_dir_all(&root).unwrap();
    prepare_configured_steamcmd_root(&root).expect("claim empty root");
    fs::write(root.join("steamcmd.exe"), b"fixture").unwrap();
    let settings = settings_with_steamcmd_root(&root);
    let before = managed_steamcmd_status(&settings);
    assert_eq!(before.ownership, SteamCmdOwnership::Managed);
    assert!(before.can_uninstall);
    let status = remove_managed_steamcmd(&settings).await.unwrap();
    assert!(!root.exists());
    assert!(!status.executable_exists);
    assert_eq!(status.ownership, SteamCmdOwnership::None);
    assert!(!status.can_uninstall);
}

#[test]
fn install_probe_uses_package_verification_instead_of_instance_wrapper() {
    let root = unique_test_root();
    let games_root = root.join("games");
    let install_root = games_root.join("romestead");
    fs::create_dir_all(&install_root).unwrap();
    fs::write(install_root.join("Server.exe"), b"fixture").unwrap();
    let settings = AppSettings {
        archives_root: String::new(),
        servers_root: root.join("servers").to_string_lossy().into_owned(),
        games_root: games_root.to_string_lossy().into_owned(),
        modules_root: root.join("modules").to_string_lossy().into_owned(),
        steamcmd_root: root.join("steamcmd").to_string_lossy().into_owned(),
    };
    let install = InstallSpec {
        shared_game_dir: String::from("romestead"),
        download_url_windows: None,
        download_integrity_windows: None,
        source: None,
        verification_path: Some(String::from("Server.exe")),
        minecraft: None,
    };
    let process = ProcessSpec {
        environment_template: Default::default(),
        executable: String::from("start-romestead.bat"),
        args_template: Vec::new(),
        working_directory_template: Some(String::from("{{paths.install_root}}")),
        window_policy: Default::default(),
        host_surface: Default::default(),
        host_notes: None,
    };

    let probe =
        probe_module_install_state(&settings, "romestead", None, Some(&install), Some(&process));
    assert_eq!(probe.install_state, InstallState::Installed);
    assert!(!probe.executable_exists);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn steamcmd_retryable_file_lock_detection_matches_real_failure_excerpt() {
    let excerpt = "Error! App '1829350' state is 0x602 after update job.\nApp update canceled : Failed updating depot 1829351 while writing chunk, offset 66060288 (File Locked) (File locked)";
    assert!(steamcmd_output_is_retryable_file_lock(excerpt));
}

#[test]
fn steamcmd_retryable_file_lock_detection_ignores_unrelated_errors() {
    let excerpt = "Error! App '1829350' state is 0x202 after update job.";
    assert!(!steamcmd_output_is_retryable_file_lock(excerpt));
}

#[cfg(windows)]
#[test]
fn hidden_child_creation_flags_suppress_external_console_windows_without_detaching() {
    assert_eq!(hidden_child_creation_flags(), CREATE_NO_WINDOW);
    assert_eq!(hidden_child_creation_flags() & DETACHED_PROCESS, 0);
}

#[cfg(windows)]
#[test]
fn command_path_string_strips_verbatim_drive_prefix() {
    assert_eq!(
        command_path_string(Path::new(r"\\?\D:\LanGame Root\steamcmd")),
        String::from(r"D:\LanGame Root\steamcmd")
    );
}

#[cfg(windows)]
#[test]
fn command_path_string_strips_verbatim_unc_prefix() {
    assert_eq!(
        command_path_string(Path::new(r"\\?\UNC\server\share\steamcmd")),
        String::from(r"\\server\share\steamcmd")
    );
}

#[cfg(windows)]
#[test]
fn steamcmd_script_path_accepts_canonical_program_roots() {
    assert_eq!(
        steamcmd_script_path(Path::new(r"\\?\D:\LanGame Root\runtime")),
        "\"D:/LanGame Root/runtime\""
    );
    assert_eq!(
        steamcmd_script_path(Path::new(r"\\?\UNC\server\share\runtime")),
        "\"//server/share/runtime\""
    );
}

#[tokio::test]
async fn steamcmd_failure_context_uses_content_log_tail_for_file_lock_retries() {
    let temp_root = unique_test_root();
    let logs_root = temp_root.join("logs");
    std::fs::create_dir_all(&logs_root).expect("create logs root");
    std::fs::write(
        logs_root.join("content_log.txt"),
        "[2026-04-09 09:21:43] AppID 1829350 update canceled : Failed updating depot 1829351 while writing chunk, offset 1048576 (File Locked) (File locked) \\\"d:\\\\langame server manager\\\\target\\\\real-vrising-smoke-runs\\\\games\\\\vrising\\\\VRisingServer_Data\\\\Plugins\\\\x86_64\\\\vivoxsdk.dll\\\"\\n",
    )
    .expect("write content log");

    let combined = steamcmd_failure_context(
        "Error! App '1829350' state is 0x602 after update job.",
        Some(1),
        read_steamcmd_content_log_excerpt(&temp_root)
            .await
            .as_deref(),
    );
    assert!(combined.contains("Command exited with code 1."));
    assert!(combined.contains("state is 0x602"));
    assert!(combined.contains("File locked"));
    assert!(steamcmd_output_is_retryable_file_lock(&combined));

    std::fs::remove_dir_all(temp_root).ok();
}

#[test]
fn steamcmd_failure_context_omits_stale_content_log_when_not_updated() {
    let combined = steamcmd_failure_context("Error! Something failed.", Some(1), None);
    assert_eq!(
        combined,
        "Command exited with code 1.\nError! Something failed."
    );
}

#[test]
fn updated_content_log_excerpt_ignores_unchanged_tail() {
    let before = Some("line-a\nline-b");
    let after = Some("line-a\nline-b");
    assert_eq!(updated_content_log_excerpt(before, after), None);

    let changed = updated_content_log_excerpt(before, Some("line-c"));
    assert_eq!(changed, Some(String::from("line-c")));
}

#[test]
fn format_command_failure_excerpt_reports_exit_code_without_output() {
    assert_eq!(
        format_command_failure_excerpt(Some(-1073741571), ""),
        "Command exited with code -1073741571."
    );
}

#[test]
fn steamcmd_install_script_for_windows_modules_forces_windows_depots() {
    let module = ModuleDetails {
        summary: app_core::ModuleSummary {
            id: String::from("runescapedragonwilds"),
            name: String::from("RuneScape: Dragonwilds Dedicated Server"),
            version: String::from("0.1.0"),
            description: None,
            steam_app_id: Some(4_019_830),
            install_state: InstallState::NotInstalled,
            instance_program_count: 0,
            archived_program_count: 0,
            supported_platforms: vec![String::from("windows")],
        },
        schema_json: None,
        default_ports: vec![],
        install: None,
        process: None,
        workshop: None,
        mods: None,
        runtime: app_core::ModuleRuntimeSpec::default(),
    };

    let script_lines =
        steamcmd_install_script_lines(&module, 4_019_830, Path::new("D:/Games/Dragonwilds"), true);

    assert!(script_lines.contains(&String::from("@sSteamCmdForcePlatformType windows")));
    assert!(script_lines.contains(&String::from("@sSteamCmdForcePlatformBitness 64")));
    assert!(script_lines.contains(&String::from("app_update 4019830 validate")));
}

#[test]
fn probe_module_install_state_keeps_explicit_java_runtime_path() {
    let temp_root = unique_test_root();
    let settings = settings_with_steamcmd_root(&temp_root);
    let install_root = PathBuf::from(&settings.games_root).join("projectzomboid");
    let other_java = install_root
        .join("other-runtime")
        .join("bin")
        .join("java.exe");
    fs::create_dir_all(other_java.parent().expect("other Java parent"))
        .expect("create other runtime");
    fs::write(&other_java, []).expect("write other Java executable");
    let install = InstallSpec {
        shared_game_dir: String::from("projectzomboid"),
        download_url_windows: None,
        download_integrity_windows: None,
        source: None,
        verification_path: None,
        minecraft: None,
    };
    let mut process = ProcessSpec {
        environment_template: Default::default(),
        executable: String::from("jre64/bin/java.exe"),
        args_template: vec![],
        working_directory_template: None,
        window_policy: app_core::ProcessWindowPolicy::Background,
        host_surface: app_core::ProcessHostSurface::ManagedTerminal,
        host_notes: None,
    };
    let expected_java = install_root.join("jre64").join("bin").join("java.exe");

    for executable in ["jre64/bin/java.exe", r"jre64\bin\java.exe"] {
        process.executable = String::from(executable);
        let probe = probe_module_install_state(
            &settings,
            "projectzomboid",
            Some(380870),
            Some(&install),
            Some(&process),
        );
        assert_eq!(PathBuf::from(&probe.executable_path), expected_java);
        assert!(!probe.executable_exists);
        assert_eq!(probe.install_state, InstallState::Corrupted);
    }

    fs::create_dir_all(expected_java.parent().expect("bundled Java parent"))
        .expect("create bundled runtime");
    fs::write(&expected_java, []).expect("write bundled Java executable");
    let probe = probe_module_install_state(
        &settings,
        "projectzomboid",
        Some(380870),
        Some(&install),
        Some(&process),
    );
    assert_eq!(PathBuf::from(&probe.executable_path), expected_java);
    assert!(probe.executable_exists);
    assert_eq!(probe.install_state, InstallState::Installed);
    fs::remove_dir_all(&temp_root).expect("remove Java probe test root");
}

#[test]
fn probe_module_install_state_resolves_bare_java_command() {
    let temp_root = unique_test_root();
    let settings = settings_with_steamcmd_root(&temp_root);
    let install_root = PathBuf::from(&settings.games_root).join("java-server");
    let runtime_root = install_root.join("runtime").join("bin");
    fs::create_dir_all(&runtime_root).expect("create runtime");
    let install = InstallSpec {
        shared_game_dir: String::from("java-server"),
        download_url_windows: None,
        download_integrity_windows: None,
        source: None,
        verification_path: None,
        minecraft: None,
    };
    let mut process = ProcessSpec {
        environment_template: Default::default(),
        executable: String::new(),
        args_template: vec![],
        working_directory_template: None,
        window_policy: app_core::ProcessWindowPolicy::Background,
        host_surface: app_core::ProcessHostSurface::ManagedTerminal,
        host_notes: None,
    };

    for executable in ["java", "java.exe"] {
        let expected_java = runtime_root.join(executable);
        fs::write(&expected_java, []).expect("write Java executable");
        process.executable = String::from(executable);
        let probe = probe_module_install_state(
            &settings,
            "java-server",
            None,
            Some(&install),
            Some(&process),
        );
        assert_eq!(PathBuf::from(&probe.executable_path), expected_java);
        assert!(probe.executable_exists);
        assert_eq!(probe.install_state, InstallState::Installed);
    }

    fs::remove_dir_all(&temp_root).expect("remove bare Java probe test root");
}

#[test]
fn probe_module_install_state_prefers_platform_specific_executable_directory() {
    let temp_root = unique_test_root();
    let games_root = temp_root.join("games");
    let install_root = games_root.join("terraria");
    let linux_executable = install_root
        .join("1458")
        .join("Linux")
        .join("TerrariaServer.exe");
    let windows_executable = install_root
        .join("1458")
        .join("Windows")
        .join("TerrariaServer.exe");
    std::fs::create_dir_all(linux_executable.parent().expect("linux parent"))
        .expect("create linux parent");
    std::fs::create_dir_all(windows_executable.parent().expect("windows parent"))
        .expect("create windows parent");
    std::fs::write(&linux_executable, []).expect("write linux placeholder");
    std::fs::write(&windows_executable, []).expect("write windows placeholder");

    let settings = AppSettings {
        archives_root: String::new(),
        servers_root: temp_root.join("instances").to_string_lossy().into_owned(),
        games_root: games_root.to_string_lossy().into_owned(),
        modules_root: temp_root.join("modules").to_string_lossy().into_owned(),
        steamcmd_root: temp_root.join("steamcmd").to_string_lossy().into_owned(),
    };
    let module = ModuleDetails {
        summary: app_core::ModuleSummary {
            id: String::from("terraria"),
            name: String::from("Terraria"),
            version: String::from("0.1.0"),
            description: None,
            steam_app_id: Some(105600),
            install_state: InstallState::NotInstalled,
            instance_program_count: 0,
            archived_program_count: 0,
            supported_platforms: vec![String::from("windows")],
        },
        schema_json: None,
        default_ports: vec![],
        install: Some(InstallSpec {
            shared_game_dir: String::from("terraria"),
            download_url_windows: None,
            download_integrity_windows: None,
            source: None,
            verification_path: None,
            minecraft: None,
        }),
        process: Some(ProcessSpec {
            environment_template: Default::default(),
            executable: String::from("TerrariaServer.exe"),
            args_template: vec![],
            working_directory_template: None,
            window_policy: app_core::ProcessWindowPolicy::Background,
            host_surface: app_core::ProcessHostSurface::ManagedTerminal,
            host_notes: None,
        }),
        workshop: None,
        mods: None,
        runtime: app_core::ModuleRuntimeSpec::default(),
    };

    let probe = probe_module_install_state(
        &settings,
        &module.summary.id,
        module.summary.steam_app_id,
        module.install.as_ref(),
        module.process.as_ref(),
    );
    assert!(probe.executable_exists);

    #[cfg(windows)]
    {
        let expected_suffix = if xna_framework_is_installed() {
            r"\Windows\TerrariaServer.exe"
        } else {
            r"\Linux\TerrariaServer.exe"
        };
        assert!(
            probe.executable_path.ends_with(expected_suffix)
                || probe
                    .executable_path
                    .ends_with(&expected_suffix.replace('\\', "/")),
            "probe chose the wrong executable: {}",
            probe.executable_path
        );
    }

    #[cfg(not(windows))]
    assert!(
        probe
            .executable_path
            .ends_with(r"\Linux\TerrariaServer.exe")
            || probe.executable_path.ends_with("/Linux/TerrariaServer.exe"),
        "probe chose the wrong executable: {}",
        probe.executable_path
    );

    let _ = std::fs::remove_dir_all(temp_root);
}

#[test]
fn probe_terraria_cold_install_resolves_template_process_from_verification_payload() {
    let temp_root = unique_test_root();
    let games_root = temp_root.join("games");
    let install_root = games_root.join("terraria");
    let windows_executable = install_root
        .join("1458")
        .join("Windows")
        .join("TerrariaServer.exe");
    std::fs::create_dir_all(windows_executable.parent().expect("windows parent"))
        .expect("create windows parent");
    std::fs::write(&windows_executable, []).expect("write windows placeholder");

    let settings = AppSettings {
        archives_root: String::new(),
        servers_root: temp_root.join("instances").to_string_lossy().into_owned(),
        games_root: games_root.to_string_lossy().into_owned(),
        modules_root: temp_root.join("modules").to_string_lossy().into_owned(),
        steamcmd_root: temp_root.join("steamcmd").to_string_lossy().into_owned(),
    };
    let install = InstallSpec {
        shared_game_dir: String::from("terraria"),
        download_url_windows: Some(String::from("https://example.invalid/terraria.zip")),
        download_integrity_windows: None,
        source: None,
        verification_path: Some(String::from("1458/Windows/TerrariaServer.exe")),
        minecraft: None,
    };
    let process = ProcessSpec {
        environment_template: Default::default(),
        executable: String::from("{{terraria.server_executable}}"),
        args_template: vec![String::from("{{terraria.launch_args}}")],
        working_directory_template: Some(String::from("{{terraria.working_directory}}")),
        window_policy: app_core::ProcessWindowPolicy::Background,
        host_surface: app_core::ProcessHostSurface::ManagedTerminal,
        host_notes: None,
    };

    let probe =
        probe_module_install_state(&settings, "terraria", None, Some(&install), Some(&process));

    assert!(matches!(probe.install_state, InstallState::Installed));
    assert!(probe.executable_exists);
    assert_eq!(PathBuf::from(&probe.executable_path), windows_executable);

    let _ = std::fs::remove_dir_all(temp_root);
}

#[test]
fn probe_module_install_state_marks_downloading_payload_incomplete() {
    let temp_root = unique_test_root();
    let games_root = temp_root.join("games");
    let install_root = games_root.join("corekeeper");
    let downloading_executable = install_root
        .join("steamapps")
        .join("downloading")
        .join("1963720")
        .join("CoreKeeperServer.exe");
    std::fs::create_dir_all(downloading_executable.parent().expect("downloading parent"))
        .expect("create downloading parent");
    std::fs::write(&downloading_executable, []).expect("write downloading executable");
    std::fs::write(
        install_root
            .join("steamapps")
            .join("appmanifest_1963720.acf"),
        r#""AppState"
{
"appid"        "1963720"
"name"        "Core Keeper Dedicated Server"
"StateFlags"        "1026"
"buildid"        "0"
"BytesToDownload"        "179234560"
"BytesDownloaded"        "0"
"BytesToStage"        "576811465"
"BytesStaged"        "0"
}"#,
    )
    .expect("write manifest");

    let settings = AppSettings {
        archives_root: String::new(),
        servers_root: temp_root.join("instances").to_string_lossy().into_owned(),
        games_root: games_root.to_string_lossy().into_owned(),
        modules_root: temp_root.join("modules").to_string_lossy().into_owned(),
        steamcmd_root: temp_root.join("steamcmd").to_string_lossy().into_owned(),
    };
    let install = InstallSpec {
        shared_game_dir: String::from("corekeeper"),
        download_url_windows: None,
        download_integrity_windows: None,
        source: None,
        verification_path: None,
        minecraft: None,
    };
    let process = ProcessSpec {
        environment_template: Default::default(),
        executable: String::from("CoreKeeperServer.exe"),
        args_template: vec![],
        working_directory_template: None,
        window_policy: app_core::ProcessWindowPolicy::Background,
        host_surface: app_core::ProcessHostSurface::ManagedTerminal,
        host_notes: None,
    };

    let probe = probe_module_install_state(
        &settings,
        "corekeeper",
        Some(1_963_720),
        Some(&install),
        Some(&process),
    );

    assert!(matches!(probe.install_state, InstallState::Incomplete));
    assert!(!probe.executable_exists);
    assert_eq!(
        PathBuf::from(&probe.executable_path),
        install_root.join("CoreKeeperServer.exe")
    );
    assert!(
        probe
            .steam_manifest
            .as_ref()
            .map(|manifest| !manifest.complete && manifest.downloading_path_exists)
            .unwrap_or(false)
    );

    let _ = std::fs::remove_dir_all(temp_root);
}

#[test]
fn probe_module_install_state_marks_empty_downloading_folder_as_complete() {
    let temp_root = unique_test_root();
    let games_root = temp_root.join("games");
    let install_root = games_root.join("corekeeper");
    std::fs::create_dir_all(install_root.join("steamapps")).expect("create steamapps");
    std::fs::create_dir_all(
        install_root
            .join("steamapps")
            .join("downloading")
            .join("1963720"),
    )
    .expect("create empty downloading folder");
    std::fs::write(install_root.join("CoreKeeperServer.exe"), []).expect("write executable");
    std::fs::write(
        install_root
            .join("steamapps")
            .join("appmanifest_1963720.acf"),
        r#""AppState"
{
"appid"        "1963720"
"name"        "Core Keeper Dedicated Server"
"StateFlags"        "4"
"buildid"        "22516523"
"TargetBuildID"        "22516523"
"BytesToDownload"        "179234560"
"BytesDownloaded"        "179234560"
"BytesToStage"        "576811465"
"BytesStaged"        "576811465"
}"#,
    )
    .expect("write manifest");

    let settings = AppSettings {
        archives_root: String::new(),
        servers_root: temp_root.join("instances").to_string_lossy().into_owned(),
        games_root: games_root.to_string_lossy().into_owned(),
        modules_root: temp_root.join("modules").to_string_lossy().into_owned(),
        steamcmd_root: temp_root.join("steamcmd").to_string_lossy().into_owned(),
    };
    let install = InstallSpec {
        shared_game_dir: String::from("corekeeper"),
        download_url_windows: None,
        download_integrity_windows: None,
        source: None,
        verification_path: None,
        minecraft: None,
    };
    let process = ProcessSpec {
        environment_template: Default::default(),
        executable: String::from("CoreKeeperServer.exe"),
        args_template: vec![],
        working_directory_template: None,
        window_policy: app_core::ProcessWindowPolicy::Background,
        host_surface: app_core::ProcessHostSurface::ManagedTerminal,
        host_notes: None,
    };

    let probe = probe_module_install_state(
        &settings,
        "corekeeper",
        Some(1_963_720),
        Some(&install),
        Some(&process),
    );

    assert!(matches!(probe.install_state, InstallState::Installed));
    assert!(probe.executable_exists);
    assert!(
        probe
            .steam_manifest
            .as_ref()
            .map(|manifest| manifest.complete && !manifest.downloading_path_exists)
            .unwrap_or(false)
    );

    let _ = std::fs::remove_dir_all(temp_root);
}

#[test]
fn probe_module_install_state_reads_complete_manifest_build_id() {
    let temp_root = unique_test_root();
    let games_root = temp_root.join("games");
    let install_root = games_root.join("corekeeper");
    std::fs::create_dir_all(install_root.join("steamapps")).expect("create steamapps");
    std::fs::write(install_root.join("CoreKeeperServer.exe"), []).expect("write executable");
    std::fs::write(
        install_root
            .join("steamapps")
            .join("appmanifest_1963720.acf"),
        r#""AppState"
{
"appid"        "1963720"
"name"        "Core Keeper Dedicated Server"
"StateFlags"        "4"
"buildid"        "22516523"
"TargetBuildID"        "22516523"
"BytesToDownload"        "179234560"
"BytesDownloaded"        "179234560"
"BytesToStage"        "576811465"
"BytesStaged"        "576811465"
}"#,
    )
    .expect("write manifest");

    let settings = AppSettings {
        archives_root: String::new(),
        servers_root: temp_root.join("instances").to_string_lossy().into_owned(),
        games_root: games_root.to_string_lossy().into_owned(),
        modules_root: temp_root.join("modules").to_string_lossy().into_owned(),
        steamcmd_root: temp_root.join("steamcmd").to_string_lossy().into_owned(),
    };
    let install = InstallSpec {
        shared_game_dir: String::from("corekeeper"),
        download_url_windows: None,
        download_integrity_windows: None,
        source: None,
        verification_path: None,
        minecraft: None,
    };
    let process = ProcessSpec {
        environment_template: Default::default(),
        executable: String::from("CoreKeeperServer.exe"),
        args_template: vec![],
        working_directory_template: None,
        window_policy: app_core::ProcessWindowPolicy::Background,
        host_surface: app_core::ProcessHostSurface::ManagedTerminal,
        host_notes: None,
    };

    let probe = probe_module_install_state(
        &settings,
        "corekeeper",
        Some(1_963_720),
        Some(&install),
        Some(&process),
    );

    assert!(matches!(probe.install_state, InstallState::Installed));
    assert!(probe.executable_exists);
    assert_eq!(probe.current_version.as_deref(), Some("22516523"));

    let _ = std::fs::remove_dir_all(temp_root);
}

#[test]
fn probe_module_install_state_uses_verification_file_for_java_server_payload() {
    let temp_root = unique_test_root();
    let games_root = temp_root.join("games");
    let install_root = games_root.join("minecraft");
    std::fs::create_dir_all(&install_root).expect("create minecraft install root");
    std::fs::write(install_root.join("server.jar"), b"jar").expect("write server jar");
    std::fs::create_dir_all(install_root.join(".langame")).expect("create metadata dir");
    std::fs::write(
        minecraft_metadata_path(&install_root),
        serde_json::json!({
            "version_id": "1.21.10",
            "version_type": "release",
            "manifest_url": MINECRAFT_VERSION_MANIFEST_URL,
            "version_url": "https://example.invalid/version.json",
            "server_url": "https://example.invalid/server.jar",
            "server_sha1": "0000000000000000000000000000000000000000",
            "server_size": 3,
            "server_jar": "server.jar",
            "downloaded_at_unix_ms": 1
        })
        .to_string(),
    )
    .expect("write minecraft metadata");

    let settings = AppSettings {
        archives_root: String::new(),
        servers_root: temp_root.join("instances").to_string_lossy().into_owned(),
        games_root: games_root.to_string_lossy().into_owned(),
        modules_root: temp_root.join("modules").to_string_lossy().into_owned(),
        steamcmd_root: temp_root.join("steamcmd").to_string_lossy().into_owned(),
    };
    let install = InstallSpec {
        shared_game_dir: String::from("minecraft"),
        download_url_windows: None,
        download_integrity_windows: None,
        source: Some(InstallSource::MinecraftJava),
        verification_path: Some(String::from("server.jar")),
        minecraft: Some(MinecraftJavaInstallSpec {
            version: String::from("latest_release"),
            manifest_url: Some(String::from(MINECRAFT_VERSION_MANIFEST_URL)),
            server_jar: String::from("server.jar"),
            java_policy: String::from("mojang_version_metadata"),
            default_distribution: String::from("vanilla"),
            distributions: Vec::new(),
        }),
    };
    let process = ProcessSpec {
        environment_template: Default::default(),
        executable: String::from("java.exe"),
        args_template: vec![
            String::from("-jar"),
            String::from("{{paths.install_root}}/server.jar"),
            String::from("nogui"),
        ],
        working_directory_template: None,
        window_policy: app_core::ProcessWindowPolicy::Background,
        host_surface: app_core::ProcessHostSurface::ManagedTerminal,
        host_notes: None,
    };

    let probe =
        probe_module_install_state(&settings, "minecraft", None, Some(&install), Some(&process));

    assert!(matches!(probe.install_state, InstallState::Installed));
    assert_eq!(probe.current_version.as_deref(), Some("1.21.10"));
    assert!(probe.diagnostics.iter().any(|line| {
        line.contains("Required server file is present") && line.contains("server.jar")
    }));

    let _ = std::fs::remove_dir_all(temp_root);
}

#[test]
fn steamcmd_status_reports_configured_root_when_managed_install_exists() {
    let temp_root = unique_test_root();
    let configured_root = temp_root.join("managed-steamcmd");
    std::fs::create_dir_all(&configured_root).expect("create configured root");
    prepare_configured_steamcmd_root(&configured_root).expect("claim configured root");
    std::fs::write(configured_root.join("steamcmd.exe"), []).expect("write steamcmd placeholder");

    let settings = AppSettings {
        archives_root: String::new(),
        servers_root: temp_root.join("instances").to_string_lossy().into_owned(),
        games_root: temp_root.join("games").to_string_lossy().into_owned(),
        modules_root: temp_root.join("modules").to_string_lossy().into_owned(),
        steamcmd_root: configured_root.to_string_lossy().into_owned(),
    };

    let status = steamcmd_status(&settings);
    assert!(status.executable_exists);
    assert_eq!(status.source, SteamCmdSource::Configured);
    assert_eq!(PathBuf::from(&status.root), configured_root);
    assert_eq!(status.root, status.configured_root);
    assert_eq!(status.ownership, SteamCmdOwnership::Managed);
    assert!(status.can_uninstall);

    let _ = std::fs::remove_dir_all(temp_root);
}

#[test]
fn pick_existing_steamcmd_candidate_skips_configured_root_and_uses_detected_install() {
    let temp_root = unique_test_root();
    let configured_root = temp_root.join("managed-steamcmd");
    let discovered_root = temp_root.join("SteamCMD");
    std::fs::create_dir_all(&discovered_root).expect("create discovered root");
    std::fs::write(discovered_root.join("steamcmd.exe"), []).expect("write discovered steamcmd");

    let candidate = discovery::pick_existing_steamcmd_candidate(
        &configured_root,
        &[configured_root.clone(), discovered_root.clone()],
    )
    .expect("discovered candidate");
    assert_eq!(candidate.source, SteamCmdSource::Discovered);
    assert_eq!(candidate.root, discovered_root);

    let _ = std::fs::remove_dir_all(temp_root);
}

#[test]
fn configured_steamcmd_status_ignores_discovered_runtime_for_managed_execution() {
    let temp_root = unique_test_root();
    let configured_root = temp_root.join("managed-steamcmd");
    let discovered_root = temp_root.join("SteamCMD");
    std::fs::create_dir_all(&discovered_root).expect("create discovered root");
    std::fs::write(discovered_root.join("steamcmd.exe"), []).expect("write discovered steamcmd");

    let settings = AppSettings {
        archives_root: String::new(),
        servers_root: temp_root.join("instances").to_string_lossy().into_owned(),
        games_root: temp_root.join("games").to_string_lossy().into_owned(),
        modules_root: temp_root.join("modules").to_string_lossy().into_owned(),
        steamcmd_root: configured_root.to_string_lossy().into_owned(),
    };

    let discovered = discovery::pick_existing_steamcmd_candidate(
        &configured_root,
        std::slice::from_ref(&discovered_root),
    )
    .expect("discovered steamcmd candidate");
    assert_eq!(discovered.source, SteamCmdSource::Discovered);
    assert_eq!(discovered.root, discovered_root);

    let managed = managed_steamcmd_status(&settings);
    assert_eq!(managed.source, SteamCmdSource::Configured);
    assert!(!managed.executable_exists);
    assert_eq!(PathBuf::from(&managed.root), configured_root);
    assert_eq!(managed.ownership, SteamCmdOwnership::None);
    assert!(!managed.can_uninstall);

    let _ = std::fs::remove_dir_all(temp_root);
}

#[test]
fn workshop_download_does_not_report_success_when_content_is_missing() {
    let root = unique_test_root();
    let primary = root.join("primary/steamapps/workshop/content/322330");
    let alternate = root.join("alternate/steamapps/workshop/content/322330");
    fs::create_dir_all(primary.join("378160973")).unwrap();
    fs::write(
        primary.join("378160973").join("modinfo.lua"),
        "name = 'Example'",
    )
    .unwrap();
    fs::write(primary.join("123456789"), "not a Workshop directory").unwrap();
    fs::write(primary.parent().unwrap().parent().unwrap().join("appworkshop_322330.acf"),
        r#""AppWorkshop" { "appid" "322330" "WorkshopItemsInstalled" { "378160973" { "manifest" "111" "size" "16" } } }"#).unwrap();

    let items = inspect_workshop_item_paths(
        322_330,
        &primary,
        &alternate,
        vec![String::from("378160973"), String::from("123456789")],
    )
    .unwrap();
    assert!(verify_downloaded_workshop_items(&items[..1], "download output").is_ok());
    let error = verify_downloaded_workshop_items(&items, "download output").unwrap_err();
    match error {
        SteamCmdError::SteamCmdCommandFailed { output_excerpt } => {
            assert!(output_excerpt.contains("123456789"));
            assert!(!output_excerpt.contains("378160973"));
            assert!(output_excerpt.contains("download output"));
        }
        other => panic!("unexpected download verification error: {other}"),
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn workshop_inspection_finds_dst_ugc_across_clusters_and_shards() {
    let root = unique_test_root();
    let install_root = root.join("dontstarve");
    let settings = settings_with_steamcmd_root(&root.join("steamcmd"));
    let fixtures = [
        ("main", "Master", "3739491677"),
        ("main", "Caves", "3734727477"),
        ("another-cluster", "Caves", "222222222"),
    ];
    for (cluster, shard, id) in fixtures {
        let content = install_root
            .join("ugc_mods")
            .join(cluster)
            .join(shard)
            .join("content/322330")
            .join(id);
        fs::create_dir_all(&content).unwrap();
        fs::write(content.join("modinfo.lua"), "name = 'Fixture'").unwrap();
    }

    let snapshot = inspect_workshop_items(
        &settings,
        322_330,
        &install_root,
        &[String::from("441378551")],
    )
    .unwrap();
    assert_eq!(snapshot.items.len(), 4);
    assert_eq!(snapshot.items[0].item_id, "441378551");
    assert!(!snapshot.items[0].installed);
    for (cluster, shard, id) in fixtures {
        let item = snapshot
            .items
            .iter()
            .find(|item| item.item_id == id)
            .unwrap();
        assert!(item.installed);
        assert_eq!(
            Path::new(&item.path),
            install_root
                .join("ugc_mods")
                .join(cluster)
                .join(shard)
                .join("content/322330")
                .join(id)
        );
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn workshop_inspection_finds_instance_isolated_dst_ugc() {
    let root = unique_test_root();
    let install_root = root.join("dontstarve");
    let settings = settings_with_steamcmd_root(&root.join("steamcmd"));
    let content_root = root.join("instances/dst-one/data/ugc/Master/content/322330");
    let item_root = content_root.join("3739491677");
    fs::create_dir_all(&item_root).unwrap();
    fs::write(item_root.join("modinfo.lua"), "name = 'Fixture'").unwrap();

    let snapshot = inspect_workshop_items_with_dst_ugc_roots(
        &settings,
        322_330,
        &install_root,
        std::slice::from_ref(&content_root),
        &[],
    )
    .unwrap();
    let item = snapshot
        .items
        .iter()
        .find(|item| item.item_id == "3739491677")
        .expect("isolated UGC item");
    assert!(item.installed);
    assert_eq!(Path::new(&item.path), item_root);
    assert!(
        snapshot
            .searched_roots
            .iter()
            .any(|path| Path::new(path) == content_root)
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn workshop_inventory_preserves_cache_priority_and_separates_download_verification() {
    let root = unique_test_root();
    let install_root = root.join("dontstarve");
    let steamcmd_root = root.join("steamcmd");
    let settings = settings_with_steamcmd_root(&steamcmd_root);
    let (primary, alternate) =
        resolve_workshop_content_roots(&install_root, &steamcmd_root, 322_330);
    let ugc = install_root.join("ugc_mods/main/Master/content/322330");
    for directory in [
        primary.join("3739491677"),
        alternate.join("3739491677"),
        install_root.join("mods/workshop-3739491677"),
        ugc.join("3739491677"),
        ugc.join("3734727477"),
    ] {
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("modinfo.lua"), "name = 'Fixture'").unwrap();
    }
    fs::write(primary.parent().unwrap().parent().unwrap().join("appworkshop_322330.acf"),
        r#""AppWorkshop" { "appid" "322330" "WorkshopItemsInstalled" { "3739491677" { "manifest" "111" "size" "16" } } }"#).unwrap();
    let snapshot = inspect_workshop_items(&settings, 322_330, &install_root, &[]).unwrap();
    let cached = snapshot
        .items
        .iter()
        .find(|item| item.item_id == "3739491677")
        .unwrap();
    assert_eq!(Path::new(&cached.path), primary.join("3739491677"));
    let ugc_only = snapshot
        .items
        .iter()
        .find(|item| item.item_id == "3734727477")
        .unwrap();
    assert!(ugc_only.installed);
    let downloaded = inspect_workshop_item_paths(
        322_330,
        &primary,
        &alternate,
        vec![String::from("3734727477")],
    )
    .unwrap();
    assert!(!downloaded[0].expected_path_exists);
    assert!(verify_downloaded_workshop_items(&downloaded, "SteamCMD output").is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn workshop_inspection_finds_native_dst_mods_without_treating_game_files_as_mods() {
    let root = unique_test_root();
    let install_root = root.join("dontstarve");
    let settings = settings_with_steamcmd_root(&root.join("steamcmd"));
    let native_mod = install_root.join("mods/workshop-3739491677");
    for directory in [
        native_mod.clone(),
        install_root.join("mods/scripts"),
        install_root.join("mods/3734727477"),
        install_root.join("mods/workshop-base"),
    ] {
        fs::create_dir_all(directory).unwrap();
    }
    fs::write(native_mod.join("modinfo.lua"), "name = 'Native fixture'").unwrap();
    fs::write(install_root.join("mods/modsettings.lua"), "return {}").unwrap();
    fs::write(
        install_root.join("mods/workshop-441378551"),
        "not a mod directory",
    )
    .unwrap();

    let snapshot = inspect_workshop_items(&settings, 322_330, &install_root, &[]).unwrap();
    assert_eq!(snapshot.items.len(), 1);
    assert_eq!(snapshot.items[0].item_id, "3739491677");
    assert!(snapshot.items[0].installed);
    assert_eq!(Path::new(&snapshot.items[0].path), native_mod);
    assert!(
        snapshot
            .searched_roots
            .iter()
            .any(|path| Path::new(path) == install_root.join("mods"))
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn workshop_inspection_does_not_use_dst_ugc_for_other_games() {
    let root = unique_test_root();
    let install_root = root.join("other-game");
    let settings = settings_with_steamcmd_root(&root.join("steamcmd"));
    fs::create_dir_all(install_root.join("ugc_mods/main/Master/content/322330/3739491677"))
        .unwrap();
    fs::create_dir_all(install_root.join("mods/workshop-3739491677")).unwrap();
    let snapshot = inspect_workshop_items(
        &settings,
        108_600,
        &install_root,
        &[String::from("3739491677")],
    )
    .unwrap();
    assert_eq!(snapshot.searched_roots.len(), 2);
    assert_eq!(snapshot.items.len(), 1);
    assert!(!snapshot.items[0].installed);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn workshop_inspection_reports_excessive_dst_clusters_instead_of_partial_inventory() {
    let root = unique_test_root();
    let install_root = root.join("dontstarve");
    let settings = settings_with_steamcmd_root(&root.join("steamcmd"));
    for index in 0..129 {
        fs::create_dir_all(
            install_root
                .join("ugc_mods")
                .join(format!("cluster-{index}")),
        )
        .unwrap();
    }
    let error = inspect_workshop_items(&settings, 322_330, &install_root, &[]).unwrap_err();
    assert!(error.to_string().contains("128-entry inspection limit"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn workshop_inspection_checks_install_and_steamcmd_content_roots() {
    let temp_root = unique_test_root();
    let install_root = temp_root.join("games").join("projectzomboid");
    let steamcmd_root = temp_root.join("steamcmd");
    let consumer_app_id = 108_600;
    let relative_workshop_root = Path::new("steamapps")
        .join("workshop")
        .join("content")
        .join(consumer_app_id.to_string());
    let primary_workshop_root = install_root.join(&relative_workshop_root);
    let fallback_workshop_root = steamcmd_root.join(&relative_workshop_root);
    let fallback_item_path = fallback_workshop_root.join("2945221351");
    let primary_inventory_item_path = primary_workshop_root.join("3111144777");
    std::fs::create_dir_all(&primary_workshop_root).expect("create primary Workshop root");
    std::fs::create_dir_all(&fallback_item_path).expect("create fallback Workshop item");
    std::fs::create_dir_all(&primary_inventory_item_path)
        .expect("create unrequested primary Workshop item");
    for (content_root, id) in [
        (&primary_workshop_root, "3111144777"),
        (&fallback_workshop_root, "2945221351"),
    ] {
        fs::write(content_root.join(id).join("payload.bin"), b"fixture").unwrap();
        fs::write(content_root.parent().unwrap().parent().unwrap().join("appworkshop_108600.acf"),
            format!(r#""AppWorkshop" {{ "appid" "108600" "WorkshopItemsInstalled" {{ "{id}" {{ "manifest" "111" "size" "7" }} }} }}"#)).unwrap();
    }

    let settings = settings_with_steamcmd_root(&steamcmd_root);
    let result = inspect_workshop_items(
        &settings,
        consumer_app_id,
        &install_root,
        &[String::from("2945221351"), String::from("3000065999")],
    )
    .expect("inspect Workshop items");

    assert_eq!(result.items.len(), 3);
    assert!(result.items[0].installed);
    assert_eq!(PathBuf::from(&result.items[0].path), fallback_item_path);
    assert!(!result.items[1].installed);
    assert_eq!(
        PathBuf::from(&result.items[1].path),
        primary_workshop_root.join("3000065999")
    );
    assert_eq!(result.items[2].item_id, "3111144777");
    assert!(result.items[2].installed);
    assert_eq!(
        PathBuf::from(&result.items[2].path),
        primary_inventory_item_path
    );

    let inventory = inspect_workshop_items(&settings, consumer_app_id, &install_root, &[])
        .expect("enumerate Workshop inventory");
    assert_eq!(
        inventory
            .items
            .iter()
            .map(|item| item.item_id.as_str())
            .collect::<Vec<_>>(),
        vec!["2945221351", "3111144777"]
    );
    assert!(inventory.items.iter().all(|item| item.installed));

    let _ = std::fs::remove_dir_all(temp_root);
}

#[tokio::test]
async fn direct_download_probe_failure_restores_complete_previous_install() {
    let temp_root = unique_test_root();
    let install_root = temp_root.join("game");
    let staging_root = temp_root.join(".game.stage");
    let rollback_root = temp_root.join(".game.rollback");
    let rejected_root = temp_root.join(".game.rejected");
    let publish_phase_path = temp_root.join(".game.publish.state");
    let archive_path = temp_root.join(".game.download.zip");
    fs::create_dir_all(&install_root).expect("create interrupted published root");
    fs::write(install_root.join("new.txt"), b"new").expect("write interrupted payload");
    fs::create_dir_all(&staging_root).expect("create staging root");
    fs::write(staging_root.join("partial.txt"), b"partial").expect("write staged payload");
    fs::create_dir_all(&rollback_root).expect("create rollback root");
    fs::write(rollback_root.join("old.txt"), b"old").expect("write rollback payload");
    fs::write(&publish_phase_path, b"rollback_pending").expect("write publish phase");
    fs::write(&archive_path, b"partial zip").expect("write partial archive");

    recover_direct_download_install(
        &install_root,
        &staging_root,
        &rollback_root,
        &rejected_root,
        &publish_phase_path,
        &archive_path,
        true,
    )
    .await
    .expect("restore complete rollback after failed probe");

    assert_eq!(fs::read(install_root.join("old.txt")).unwrap(), b"old");
    assert!(!install_root.join("new.txt").exists());
    assert!(!staging_root.exists());
    assert!(!rollback_root.exists());
    assert!(!rejected_root.exists());
    assert!(!publish_phase_path.exists());
    assert!(!archive_path.exists());
    fs::remove_dir_all(temp_root).unwrap();
}

#[test]
fn direct_download_publish_script_keeps_rollback_until_rust_verification() {
    let script = direct_download_publish_script(
        Path::new("C:/games/server"),
        Path::new("C:/games/.server.stage"),
        Path::new("C:/games/.server.rollback"),
        Path::new("C:/games/.server.publish.state"),
        Path::new("C:/games/.server.stage/Server.exe"),
        Path::new("C:/games/.server.zip"),
        &Default::default(),
    );

    assert!(script.contains("throw 'Rollback path already exists.'"));
    let phase_write = script
        .find("[IO.File]::WriteAllText($phase, 'rollback_pending')")
        .expect("publish phase is recorded");
    let rollback_move = script
        .find("Move-Item -LiteralPath $root -Destination $rollback")
        .expect("current root is retained as rollback");
    assert!(phase_write < rollback_move);
    assert!(!script.contains("Remove-Item -LiteralPath $rollback"));
    assert!(!script.contains("Invoke-WebRequest"));
}

#[cfg(windows)]
#[tokio::test]
async fn fresh_direct_install_creates_root_before_entering_publish_helper() {
    let temp_root = unique_test_root();
    let games_root = temp_root.join("games");
    let install_root = games_root.join("direct-contract");
    let settings = AppSettings {
        archives_root: String::new(),
        servers_root: temp_root.join("instances").to_string_lossy().into_owned(),
        games_root: games_root.to_string_lossy().into_owned(),
        modules_root: temp_root.join("modules").to_string_lossy().into_owned(),
        steamcmd_root: temp_root.join("steamcmd").to_string_lossy().into_owned(),
    };
    let module = ModuleDetails {
        summary: app_core::ModuleSummary {
            id: String::from("direct-contract"),
            name: String::from("Direct Contract Fixture"),
            version: String::from("1.0.0"),
            description: None,
            steam_app_id: None,
            install_state: InstallState::NotInstalled,
            instance_program_count: 0,
            archived_program_count: 0,
            supported_platforms: vec![String::from("windows")],
        },
        schema_json: None,
        default_ports: vec![],
        install: Some(InstallSpec {
            shared_game_dir: String::from("direct-contract"),
            download_url_windows: Some(String::from("https://example.invalid/server.zip")),
            download_integrity_windows: None,
            source: None,
            // The direct helper rejects this before starting PowerShell or network I/O.
            verification_path: Some(String::from("../outside.exe")),
            minecraft: None,
        }),
        process: Some(ProcessSpec {
            environment_template: Default::default(),
            executable: String::from("Server.exe"),
            args_template: vec![],
            working_directory_template: None,
            window_policy: app_core::ProcessWindowPolicy::Background,
            host_surface: app_core::ProcessHostSurface::ManagedTerminal,
            host_notes: None,
        }),
        workshop: None,
        mods: None,
        runtime: app_core::ModuleRuntimeSpec::default(),
    };

    let result = install_or_update_module(&settings, &module, false).await;

    assert!(matches!(
        result,
        Err(SteamCmdError::InvalidInstallRelativePath { .. })
    ));
    assert!(install_root.is_dir());
    assert_eq!(fs::read_dir(&install_root).unwrap().count(), 0);
    fs::remove_dir_all(temp_root).unwrap();
}

#[tokio::test]
async fn direct_download_recovery_never_consumes_unowned_rollback_collision() {
    let temp_root = unique_test_root();
    let install_root = temp_root.join("game");
    let staging_root = temp_root.join("stage");
    let rollback_root = temp_root.join("rollback");
    let rejected_root = temp_root.join("rejected");
    let phase_path = temp_root.join("missing-phase.state");
    let archive_path = temp_root.join("download.zip");
    fs::create_dir_all(&install_root).unwrap();
    fs::write(install_root.join("current.txt"), b"current").unwrap();
    fs::create_dir_all(&rollback_root).unwrap();
    fs::write(rollback_root.join("unowned.txt"), b"unowned").unwrap();

    let result = recover_direct_download_install(
        &install_root,
        &staging_root,
        &rollback_root,
        &rejected_root,
        &phase_path,
        &archive_path,
        false,
    )
    .await;

    assert!(matches!(
        result,
        Err(SteamCmdError::InstallRollbackFailed { .. })
    ));
    assert_eq!(
        fs::read(install_root.join("current.txt")).unwrap(),
        b"current"
    );
    assert_eq!(
        fs::read(rollback_root.join("unowned.txt")).unwrap(),
        b"unowned"
    );
    fs::remove_dir_all(temp_root).unwrap();
}

#[tokio::test]
async fn verified_install_survives_rollback_cleanup_failure() {
    let temp_root = unique_test_root();
    let install_root = temp_root.join("game");
    let rollback_root = temp_root.join("rollback-is-a-file");
    fs::create_dir_all(&install_root).unwrap();
    fs::write(install_root.join("verified.txt"), b"verified").unwrap();
    fs::write(&rollback_root, b"cleanup must be best effort").unwrap();

    schedule_verified_directory_cleanup(vec![rollback_root.clone()]);
    tokio::time::sleep(Duration::from_millis(50)).await;

    assert_eq!(
        fs::read(install_root.join("verified.txt")).unwrap(),
        b"verified"
    );
    assert!(rollback_root.exists());
    fs::remove_dir_all(temp_root).unwrap();
}

fn prepare_jre_publish_state(root: &Path) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let jre_root = root.join("jre");
    let rollback_root = root.join(".jre-rollback");
    let rejected_root = root.join(".jre-rejected");
    let staging_root = root.join(".jre-stage");
    fs::create_dir_all(&jre_root).unwrap();
    fs::write(jre_root.join("new.txt"), b"new jre").unwrap();
    fs::create_dir_all(&rollback_root).unwrap();
    fs::write(rollback_root.join("old.txt"), b"old jre").unwrap();
    fs::create_dir_all(&staging_root).unwrap();
    (jre_root, rollback_root, rejected_root, staging_root)
}

#[tokio::test]
async fn jre_validation_timeout_restores_previous_runtime_and_original_error() {
    let temp_root = unique_test_root();
    fs::create_dir_all(&temp_root).unwrap();
    let (jre_root, rollback_root, rejected_root, staging_root) =
        prepare_jre_publish_state(&temp_root);

    let result = finish_published_jre(
        Err(SteamCmdError::OperationTimedOut {
            operation: "JRE test",
            timeout_seconds: 1,
        }),
        21,
        &jre_root,
        &rollback_root,
        &rejected_root,
        &staging_root,
    )
    .await;

    assert!(matches!(
        result,
        Err(SteamCmdError::OperationTimedOut {
            operation: "JRE test",
            ..
        })
    ));
    assert_eq!(fs::read(jre_root.join("old.txt")).unwrap(), b"old jre");
    assert!(!jre_root.join("new.txt").exists());
    fs::remove_dir_all(temp_root).unwrap();
}

#[tokio::test]
async fn jre_missing_or_wrong_version_restores_previous_runtime() {
    for (case, validation) in [("missing", None), ("wrong", Some(17))] {
        let temp_root = unique_test_root().join(case);
        fs::create_dir_all(&temp_root).unwrap();
        let (jre_root, rollback_root, rejected_root, staging_root) =
            prepare_jre_publish_state(&temp_root);

        let result = finish_published_jre(
            Ok(validation),
            21,
            &jre_root,
            &rollback_root,
            &rejected_root,
            &staging_root,
        )
        .await;

        assert!(matches!(
            result,
            Err(SteamCmdError::MinecraftJrePreparation { .. })
        ));
        assert_eq!(fs::read(jre_root.join("old.txt")).unwrap(), b"old jre");
        assert!(!jre_root.join("new.txt").exists());
        fs::remove_dir_all(temp_root).unwrap();
    }
}

#[tokio::test]
async fn steamcmd_output_forwarder_backpressures_when_the_channel_is_full() {
    use tokio::io::AsyncWriteExt;

    let (mut writer, reader) = tokio::io::duplex(128);
    let (sender, mut receiver) = mpsc::channel(1);
    let mut forwarder = spawn_line_forwarder(reader, sender);

    writer
        .write_all(b"first line\nsecond line\n")
        .await
        .expect("write output lines");
    writer.shutdown().await.expect("close output writer");
    tokio::time::timeout(Duration::from_secs(1), async {
        while receiver.len() != 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("first line fills the bounded channel");
    assert!(
        !forwarder.is_finished(),
        "the second line must wait while the bounded channel is full"
    );

    assert_eq!(
        receiver
            .recv()
            .await
            .expect("first message")
            .expect("first line"),
        "first line"
    );
    assert_eq!(
        receiver
            .recv()
            .await
            .expect("second message")
            .expect("second line"),
        "second line"
    );
    assert!(receiver.recv().await.is_none());
    forwarder.join().await.expect("join output forwarder");
}

#[tokio::test]
async fn steamcmd_output_forwarder_bounds_a_line_without_a_newline() {
    use tokio::io::AsyncWriteExt;

    let (mut writer, reader) = tokio::io::duplex(1024);
    let (sender, mut receiver) = mpsc::channel(1);
    let mut forwarder = spawn_line_forwarder(reader, sender);
    let oversized_line = vec![b'x'; STEAMCMD_OUTPUT_LINE_LIMIT_BYTES * 3];

    let writer = tokio::spawn(async move {
        writer
            .write_all(&oversized_line)
            .await
            .expect("write oversized line");
        writer.shutdown().await.expect("close output writer");
    });
    let mut last = None;
    while let Some(line) = receiver.recv().await {
        let line = line.expect("bounded line");
        assert!(line.len() <= STEAMCMD_OUTPUT_LINE_LIMIT_BYTES);
        last = Some(line);
    }
    assert!(
        last.expect("bounded message")
            .ends_with(STEAMCMD_OUTPUT_LINE_TRUNCATED_SUFFIX)
    );
    writer.await.unwrap();
    forwarder.join().await.expect("join output forwarder");
}

#[test]
fn steamcmd_output_line_limit_is_preserved_after_lossy_utf8_conversion() {
    let invalid = vec![0xff; STEAMCMD_OUTPUT_LINE_LIMIT_BYTES];
    let line = finish_bounded_output_line(&invalid, false);

    assert!(line.len() <= STEAMCMD_OUTPUT_LINE_LIMIT_BYTES);
    assert!(line.ends_with(STEAMCMD_OUTPUT_LINE_TRUNCATED_SUFFIX));
}
