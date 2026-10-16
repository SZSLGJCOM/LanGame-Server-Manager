#![cfg(windows)]

use std::path::PathBuf;

use super::Game;
use super::probe_support::run_owned_empty_probe;

#[test]
#[ignore = "requires a marked bare LGSM_DRAGONWILDS_PROBE_ROOT and pinned LGSM_DRAGONWILDS_LOADER_ARCHIVE"]
fn isolated_dragonwilds_automatic_loader_install_survives_native_restart() {
    let root = PathBuf::from(std::env::var_os("LGSM_DRAGONWILDS_PROBE_ROOT").unwrap());
    let archive = PathBuf::from(std::env::var_os("LGSM_DRAGONWILDS_LOADER_ARCHIVE").unwrap());
    crate::commands::prepare_dragonwilds_player_query_probe(&root, &archive).unwrap();
    isolated_dragonwilds_bootstrap_reads_two_current_empty_snapshots_and_rejects_stopped_process();
    crate::commands::verify_dragonwilds_player_query_probe(&root).unwrap();
    // The native server removes AdminPassword from its saved INI. Validate the
    // initial isolated credentials once, then restart the native saved state.
    run_dragonwilds_empty_probe(root.clone());
    crate::commands::verify_dragonwilds_player_query_probe(&root).unwrap();
}

#[test]
#[ignore = "requires LGSM_DRAGONWILDS_PROBE_ROOT pointing to a marked isolated Dragonwilds installation with the verified player-query extension"]
fn isolated_dragonwilds_bootstrap_reads_two_current_empty_snapshots_and_rejects_stopped_process() {
    let root = PathBuf::from(std::env::var_os("LGSM_DRAGONWILDS_PROBE_ROOT").unwrap());
    assert!(root.is_absolute());
    assert!(root.join(".lgsm-isolated-player-probe").is_file());
    let config = std::fs::read_to_string(
        root.join("RSDragonwilds/Saved/Config/WindowsServer/DedicatedServer.ini"),
    )
    .unwrap();
    let section = config
        .split_once("[/Script/Dominion.DedicatedServerSettings]")
        .expect("dedicated server settings section")
        .1;
    let section = section.split("\n[").next().unwrap();
    for key in ["OwnerId", "AdminPassword", "WorldPassword"] {
        let values: Vec<_> = section
            .lines()
            .filter_map(|line| {
                let (name, value) = line.split_once('=')?;
                (name.trim() == key).then(|| value.trim().trim_matches('"'))
            })
            .collect();
        assert_eq!(
            values.len(),
            1,
            "one explicit isolation credential is required for {key}"
        );
        assert!(
            values[0].len() >= 16,
            "nonempty isolation credential is required"
        );
        if key == "OwnerId" {
            assert!(values[0].encode_utf16().count() <= 32);
        }
    }
    run_dragonwilds_empty_probe(root);
}

fn run_dragonwilds_empty_probe(root: PathBuf) {
    run_owned_empty_probe(
        Game::Dragonwilds,
        root,
        [
            "-Port=17993",
            "-QueryPort=27993",
            "-MULTIHOME=127.0.0.1",
            "-unattended",
            "-NoSplash",
            "-stdout",
            "-FullStdOutLogOutput",
        ]
        .map(str::to_owned)
        .to_vec(),
    );
}

#[test]
#[ignore = "requires an explicitly marked isolated Dragonwilds installation containing a stopped native world copy"]
fn isolated_dragonwilds_old_world_autosaves_and_survives_orderly_restart() {
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    struct OwnedWorldProbe(Option<app_runtime::SpawnedProcess>);
    impl Drop for OwnedWorldProbe {
        fn drop(&mut self) {
            if let Some(server) = self.0.as_mut()
                && let Err(error) = app_runtime::stop_spawned_process(server)
            {
                eprintln!("Failed to recover owned Dragonwilds world probe: {error}");
                assert!(std::thread::panicking(), "owned world probe cleanup failed");
            }
        }
    }

    let root = PathBuf::from(
        std::env::var_os("LGSM_DRAGONWILDS_PROBE_ROOT").expect("supply the isolated installation"),
    );
    assert!(root.is_absolute());
    assert!(root.join(".lgsm-isolated-player-probe").is_file());
    let expected_world = std::env::var("LGSM_DRAGONWILDS_EXPECTED_WORLD_NAME")
        .expect("supply the existing isolated world's name");
    assert!(!expected_world.is_empty() && !expected_world.chars().any(char::is_control));
    let expected_factor: f32 = std::env::var("LGSM_DRAGONWILDS_EXPECTED_BUILDING_FACTOR")
        .expect("supply the isolated world's expected building factor")
        .parse()
        .expect("building factor must be a number");
    assert!(expected_factor.is_finite() && (0.0..=3.0).contains(&expected_factor));
    let config = std::fs::read_to_string(
        root.join("RSDragonwilds/Saved/Config/WindowsServer/DedicatedServer.ini"),
    )
    .unwrap();
    let section = config
        .split_once("[/Script/Dominion.DedicatedServerSettings]")
        .expect("dedicated server settings section")
        .1
        .split("\n[")
        .next()
        .unwrap();
    for key in [
        "OwnerId",
        "AdminPassword",
        "WorldPassword",
        "DefaultWorldName",
    ] {
        let values: Vec<_> = section
            .lines()
            .filter_map(|line| {
                let (name, value) = line.split_once('=')?;
                (name.trim() == key).then(|| value.trim().trim_matches('"'))
            })
            .collect();
        assert_eq!(
            values.len(),
            1,
            "one explicit isolation setting is required for {key}"
        );
        if key == "DefaultWorldName" {
            assert_eq!(values[0], expected_world);
        } else {
            assert!(
                values[0].len() >= 16,
                "a strong isolation credential is required"
            );
            if key == "OwnerId" {
                assert!(values[0].encode_utf16().count() <= 32);
            }
        }
    }
    let saves = root.join("RSDragonwilds/Saved/SaveGames");
    let has_world_save = || {
        std::fs::read_dir(&saves).unwrap().any(|entry| {
            let path = entry.unwrap().path();
            path.extension()
                .is_some_and(|value| value.eq_ignore_ascii_case("sav"))
                && std::fs::symlink_metadata(path)
                    .is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0)
        })
    };
    assert!(
        has_world_save(),
        "supply an existing world copy before launching"
    );
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let load_marker = format!("Loading configured world [{expected_world}] from save slot [");
    let rule_marker = format!(
        "Applied custom setting: Difficulty.Progression.BuildingMaterialCostScale : {expected_factor:.2}"
    );

    for pass in 0..2 {
        let native_log = root.join(format!("world-compatibility-{nonce}-{pass}.log"));
        assert!(!native_log.exists(), "each boot must have a new native log");
        let plan = app_core::LaunchPlan {
            environment: Default::default(),
            instance_id: format!("dragonwilds-world-probe-{nonce}"),
            instance_name: "Isolated Dragonwilds world probe".into(),
            module_id: "runescapedragonwilds".into(),
            install_root: root.to_string_lossy().into_owned(),
            install_state: app_core::InstallState::Installed,
            uses_private_runtime: true,
            working_directory: root.to_string_lossy().into_owned(),
            executable_path: root
                .join("RSDragonwildsServer.exe")
                .to_string_lossy()
                .into_owned(),
            executable_exists: true,
            ready_to_launch: true,
            validation_issues: Vec::new(),
            args: vec![
                "-log".into(),
                "-NewConsole".into(),
                "-Port=17993".into(),
                "-QueryPort=27993".into(),
                "-MULTIHOME=127.0.0.1".into(),
                "-unattended".into(),
                "-NoSplash".into(),
                "-stdout".into(),
                "-FullStdOutLogOutput".into(),
                format!("-abslog={}", native_log.display()),
            ],
            command_line: String::new(),
            window_policy: app_core::ProcessWindowPolicy::Background,
            uses_script_entrypoint: false,
            requires_admin: false,
            host_surface: app_core::ProcessHostSurface::ManagedTerminal,
            host_notes: None,
            performance_policy: app_core::RuntimePerformancePolicy::default(),
            performance_preview: app_core::RuntimePerformancePolicyPreview::default(),
        };
        let output = root.join(format!("world-compatibility-output-{nonce}-{pass}.log"));
        let mut owned = OwnedWorldProbe(Some(
            app_runtime::spawn_launch_plan(&plan, &output).unwrap(),
        ));
        let spawned = owned.0.as_mut().unwrap();
        assert!(
            app_runtime::stabilize_spawned_process(
                &plan.executable_path,
                spawned,
                Duration::from_millis(300),
            )
            .unwrap()
            .is_none()
        );
        assert_eq!(
            std::fs::canonicalize(
                Game::Dragonwilds
                    .install_root(std::path::Path::new(&spawned.process_identity.image_path),)
                    .unwrap()
            )
            .unwrap(),
            std::fs::canonicalize(&root).unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(180);
        loop {
            let log = match std::fs::read_to_string(&native_log) {
                Ok(log) => log,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
                Err(error) => panic!("cannot read this boot's native log: {error}"),
            };
            let ready = log.contains(
                "LogNetSessionSettings: Setting [\"ReadyToJoin\"] written with key[x0] value[1]",
            );
            let loaded = log.contains("Load: Success") && log.contains(&load_marker);
            let rule_applied = log.contains(&rule_marker);
            if ready && loaded && rule_applied {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "native boot {pass} timed out: ready={ready}, existing_world_loaded={loaded}, rule_applied={rule_applied}; inspect {}",
                native_log.display()
            );
            assert!(
                app_runtime::process_matches_identity(spawned.pid, &spawned.process_identity,)
                    .unwrap(),
                "owned native server exited before acceptance"
            );
            std::thread::sleep(Duration::from_millis(250));
        }
        // The native game does not save on engine quit. Observe an actual
        // periodic save on the first boot, then load those bytes on the second.
        if pass == 0 {
            let before_autosave =
                std::fs::read(saves.join(format!("{expected_world}.sav"))).unwrap();
            let deadline = Instant::now() + Duration::from_secs(360);
            loop {
                let log = std::fs::read_to_string(&native_log).unwrap();
                if log.contains(&format!("Save to slot {expected_world}: Success"))
                    || log.contains(&format!(
                        "Save completed SUCCESSFULLY (slot: {expected_world})"
                    ))
                {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "native periodic save did not complete; inspect {}",
                    native_log.display()
                );
                assert!(
                    app_runtime::process_matches_identity(spawned.pid, &spawned.process_identity)
                        .unwrap()
                );
                std::thread::sleep(Duration::from_millis(250));
            }
            assert_ne!(
                std::fs::read(saves.join(format!("{expected_world}.sav"))).unwrap(),
                before_autosave
            );
        }
        let saved_before_stop = std::fs::read(saves.join(format!("{expected_world}.sav"))).unwrap();
        // This API reports a timeout/error while retaining ownership. A failure
        // may use the guard for cleanup, but cannot pass through a force fallback.
        let before_stop = std::fs::read_to_string(&native_log).unwrap();
        let exit =
            app_runtime::stop_spawned_unreal_process_gracefully(spawned, Duration::from_secs(20))
                .expect("native graceful shutdown must complete without force fallback");
        assert_eq!(exit, Some(0), "native graceful stop must exit successfully");
        assert!(spawned.child.is_none() && spawned.hidden_desktop.is_none());
        let after_stop = std::fs::read_to_string(&native_log).unwrap();
        let shutdown_log = after_stop
            .strip_prefix(&before_stop)
            .expect("the current native log must be preserved through shutdown");
        assert!(
            shutdown_log.contains("PreExit Game.")
                && shutdown_log
                    .contains("Draining save requests before world teardown (world teardown)")
                && shutdown_log.contains("LogExit: Exiting."),
            "native orderly teardown must finish; inspect {}",
            native_log.display()
        );
        assert_eq!(
            std::fs::read(saves.join(format!("{expected_world}.sav"))).unwrap(),
            saved_before_stop,
            "orderly quit must preserve the last completed native save"
        );
        assert!(
            has_world_save(),
            "native shutdown must preserve a world save"
        );
        owned.0 = None;
    }
}
