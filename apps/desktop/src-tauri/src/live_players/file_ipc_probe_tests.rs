#![cfg(windows)]

use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::response::parse;
use super::transport::exchange;
use super::{BridgeError, Game};

struct OwnedServer(Option<app_runtime::SpawnedProcess>);

impl Drop for OwnedServer {
    fn drop(&mut self) {
        if let Some(server) = self.0.as_mut()
            && let Err(error) = app_runtime::stop_spawned_process(server)
        {
            eprintln!("Failed to stop owned player-query probe: {error}");
            assert!(std::thread::panicking(), "owned probe cleanup failed");
        }
    }
}

fn now_ms() -> u64 {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap()
}

pub(super) fn run_owned_empty_probe(game: Game, root: PathBuf, args: Vec<String>) {
    assert!(root.is_absolute());
    assert!(root.join(".lgsm-isolated-player-probe").is_file());
    assert!(root.join("langame_player_query").is_dir());
    assert!(
        root.join(game.project_directory())
            .join("Binaries/Win64/ue4ss/Mods/LgsmPlayerQuery/enabled.txt")
            .is_file()
    );
    let plan = app_core::LaunchPlan {
        environment: Default::default(),
        instance_id: format!("{}-player-probe", game.module_id()),
        instance_name: format!("Isolated {} player probe", game.module_id()),
        module_id: game.module_id().into(),
        install_root: root.to_string_lossy().into_owned(),
        install_state: app_core::InstallState::Installed,
        uses_private_runtime: true,
        working_directory: root.to_string_lossy().into_owned(),
        executable_path: root
            .join(if game == Game::Scum {
                "SCUM/Binaries/Win64/SCUMServer.exe"
            } else {
                game.bootstrap_executable()
            })
            .to_string_lossy()
            .into_owned(),
        executable_exists: true,
        ready_to_launch: true,
        validation_issues: Vec::new(),
        args,
        command_line: String::new(),
        window_policy: app_core::ProcessWindowPolicy::Background,
        uses_script_entrypoint: false,
        requires_admin: game == Game::Scum,
        host_surface: app_core::ProcessHostSurface::ManagedTerminal,
        host_notes: None,
        performance_policy: app_core::RuntimePerformancePolicy::default(),
        performance_preview: app_core::RuntimePerformancePolicyPreview::default(),
    };
    let log = root.join(format!("player-query-production-probe-{}.log", now_ms()));
    let mut owned = OwnedServer(Some(app_runtime::spawn_launch_plan(&plan, &log).unwrap()));
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
    let pid = spawned.pid;
    let identity = spawned.process_identity.clone();
    assert_eq!(
        std::fs::canonicalize(
            game.install_root(std::path::Path::new(&identity.image_path))
                .unwrap()
        )
        .unwrap(),
        std::fs::canonicalize(&root).unwrap()
    );
    let deadline = Instant::now() + Duration::from_secs(90);
    let mut completed = 0;
    let mut sequence = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    while completed < 2 {
        sequence += 1;
        let nonce = format!("{sequence:032x}");
        let observed_at = now_ms();
        let body = match exchange(game, pid, &identity, &nonce, observed_at) {
            Ok(body) => body,
            Err(error) if completed == 0 && Instant::now() < deadline => {
                println!("Waiting for owned player-query loader: {error:?}");
                std::thread::sleep(Duration::from_millis(250));
                continue;
            }
            Err(error) => panic!("owned player-query exchange failed: {error:?}"),
        };
        let snapshot = match parse(game, &plan.instance_id, &nonce, observed_at, &body) {
            Ok(snapshot) => snapshot,
            Err(snapshot) if completed == 0 && Instant::now() < deadline => {
                println!("Waiting for owned player-query world: {:?}", snapshot.issue);
                std::thread::sleep(Duration::from_millis(250));
                continue;
            }
            Err(snapshot) => panic!("owned player-query response failed: {:?}", snapshot.issue),
        };
        assert!(snapshot.public_snapshot.complete);
        assert_eq!(snapshot.public_snapshot.current_players, Some(0));
        assert!(snapshot.public_snapshot.entries.is_empty());
        assert!(snapshot.private_action_bindings.is_empty());
        assert_eq!(snapshot.public_snapshot.snapshot_id, nonce);
        completed += 1;
    }
    app_runtime::stop_spawned_process(owned.0.as_mut().unwrap()).unwrap();
    owned.0 = None;
    assert!(matches!(
        exchange(
            game,
            pid,
            &identity,
            &format!("{:032x}", sequence + 1),
            now_ms()
        ),
        Err(BridgeError::Process)
    ));
    println!(
        "Verified production bootstrap ownership, two nonce-matched empty snapshots, and stopped-process rejection."
    );
}
