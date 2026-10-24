use super::*;
use std::sync::{Arc, mpsc};

#[tokio::test]
async fn save_loader_timeout_preserves_drained_failure_and_waits_for_cleanup() {
    use super::super::commands_mods::mod_online::ThunderstorePreparationFailure;
    use std::sync::atomic::{AtomicBool, Ordering};
    let root = Fixture::new();
    fs::write(root.0.join("valheim_server.exe"), b"MZfixture server").unwrap();
    for (message, reason) in [
        ("loader ownership conflict", None),
        ("local publication failed", None),
        ("owned cleanup failed", None),
        ("preparation cancelled", None),
        (
            "remote request failed",
            Some(app_core::RuntimeModUpdateReason::Request),
        ),
    ] {
        let cancellation = app_steamcmd::InstallCancellation::new();
        let drained = AtomicBool::new(false);
        let result = await_loader_preparation(
            &cancellation,
            async {
                cancellation.cancelled().await;
                tokio::task::yield_now().await;
                drained.store(true, Ordering::SeqCst);
                Err::<(), _>(match reason {
                    Some(reason) => ThunderstorePreparationFailure::remote(message.into(), reason),
                    None => message.to_owned().into(),
                })
            },
            Duration::ZERO,
        )
        .await;
        assert!(cancellation.is_cancelled());
        assert!(
            drained.load(Ordering::SeqCst),
            "caller returned before cleanup finished"
        );
        let error = result.expect_err("a deadline cannot admit a package");
        assert_eq!(error.message, message);
        assert_eq!(error.reason(), reason);
        let fallback = finish_optional_loader_download::<()>(&root.0, "valheim", Err(error));
        if reason.is_none() {
            assert_eq!(
                fallback.unwrap_err(),
                message,
                "even an empty loader directory cannot weaken a local error"
            );
        } else {
            assert_eq!(fallback.unwrap(), None);
        }
    }
}

#[tokio::test]
async fn save_loader_timeout_discards_late_package_before_native_fallback() {
    use std::sync::atomic::{AtomicBool, Ordering};
    struct Package(Arc<AtomicBool>);
    impl Drop for Package {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    let cancellation = app_steamcmd::InstallCancellation::new();
    let dropped = Arc::new(AtomicBool::new(false));
    let result = await_loader_preparation(
        &cancellation,
        async {
            cancellation.cancelled().await;
            Ok(Package(dropped.clone()))
        },
        Duration::ZERO,
    )
    .await;
    assert!(
        dropped.load(Ordering::SeqCst),
        "late package ownership must be dropped before fallback"
    );
    let error = result
        .err()
        .expect("late success must not install a package");
    assert_eq!(
        error.reason(),
        Some(app_core::RuntimeModUpdateReason::Network)
    );
    let root = Fixture::new();
    fs::write(root.0.join("valheim_server.exe"), b"MZfixture server").unwrap();
    fs::write(root.0.join("winhttp.dll"), b"operator proxy").unwrap();
    assert!(finish_optional_loader_download::<()>(&root.0, "valheim", Err(error)).is_err());
    assert_eq!(
        fs::read(root.0.join("winhttp.dll")).unwrap(),
        b"operator proxy"
    );
}

#[test]
fn optional_save_loader_remote_failure_allows_only_an_inactive_native_runtime() {
    use super::super::commands_mods::mod_online::ThunderstorePreparationFailure;
    for module in ["valheim", "vrising"] {
        let root = Fixture::new();
        let image = if module == "valheim" {
            "valheim_server.exe"
        } else {
            "VRisingServer.exe"
        };
        fs::write(root.0.join(image), b"MZfixture server").unwrap();
        let remote = || {
            ThunderstorePreparationFailure::remote(
                "fixture remote download failure".into(),
                app_core::RuntimeModUpdateReason::Network,
            )
        };
        assert_eq!(
            finish_optional_loader_download::<()>(&root.0, module, Err(remote())).unwrap(),
            None
        );
        fs::write(root.0.join("winhttp.dll"), b"operator proxy").unwrap();
        assert!(finish_optional_loader_download::<()>(&root.0, module, Err(remote())).is_err());
        assert_eq!(
            fs::read(root.0.join("winhttp.dll")).unwrap(),
            b"operator proxy"
        );
    }
}

#[test]
fn optional_save_loader_local_and_cancelled_failures_remain_errors() {
    use super::super::commands_mods::mod_online::ThunderstorePreparationFailure;
    let root = Fixture::new();
    fs::write(root.0.join("valheim_server.exe"), b"MZfixture server").unwrap();
    for error in [
        ThunderstorePreparationFailure::from("loader ownership conflict".to_owned()),
        ThunderstorePreparationFailure::cancelled(),
    ] {
        let message = error.message.clone();
        assert_eq!(
            finish_optional_loader_download::<()>(&root.0, "valheim", Err(error)).unwrap_err(),
            message
        );
    }
    assert_eq!(
        finish_optional_loader_download(&root.0, "valheim", Ok(7)).unwrap(),
        Some(7)
    );
}

#[tokio::test]
async fn save_loader_timeout_can_skip_only_its_own_completed_cancellation() {
    use super::super::commands_mods::mod_online::ThunderstorePreparationFailure;
    let root = Fixture::new();
    fs::write(root.0.join("valheim_server.exe"), b"MZfixture server").unwrap();
    let cancellation = app_steamcmd::InstallCancellation::new();
    let completed = std::sync::atomic::AtomicBool::new(false);
    let result = await_loader_preparation(
        &cancellation,
        async {
            cancellation.cancelled().await;
            completed.store(true, std::sync::atomic::Ordering::SeqCst);
            Err::<(), _>(ThunderstorePreparationFailure::cancelled())
        },
        Duration::ZERO,
    )
    .await;
    assert!(completed.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(
        finish_optional_loader_download(&root.0, "valheim", result).unwrap(),
        None
    );

    // User/provider cancellation before the deadline is still terminal.
    let result = await_loader_preparation(
        &cancellation,
        async { Err::<(), _>(ThunderstorePreparationFailure::cancelled()) },
        Duration::from_secs(30),
    )
    .await;
    let error = result.expect_err("independent cancellation must remain an error");
    assert!(error.is_cancelled());
    assert!(finish_optional_loader_download::<()>(&root.0, "valheim", Err(error)).is_err());
}

#[test]
fn managed_startup_hooks_preserve_existing_plan_hooks_including_windows_key_aliases() {
    for key in [
        "DOTNET_STARTUP_HOOKS",
        "dotnet_startup_hooks",
        "DotNet_Startup_Hooks",
    ] {
        let existing = BTreeMap::from([(key.to_owned(), "D:/operator/plugin.dll".to_owned())]);
        assert!(validate_startup_hook_environment(existing.iter()).is_err());
        assert_eq!(existing[key], "D:/operator/plugin.dll");
    }
    assert!(
        validate_startup_hook_environment(
            BTreeMap::from([("DOTNET_STARTUP_HOOKS".into(), "".into())]).iter()
        )
        .is_ok()
    );
    assert!(
        validate_startup_hook_environment(
            BTreeMap::from([("OTHER".into(), "value".into())]).iter()
        )
        .is_ok()
    );
}

#[cfg(windows)]
#[test]
fn managed_startup_paths_reach_real_managed_file_boundaries() {
    let fixture = Fixture::new();
    let canonical = fs::canonicalize(&fixture.0).unwrap();
    assert!(canonical.to_str().unwrap().starts_with(r"\\?\"));
    let compatible = super::managed_startup_path(&canonical);
    for path in [
        r"\\?\UNC\server\share\control",
        r"\\?\C:\operator games\trailing .",
        r"\\?\C:\operator games\COM1.dll",
    ] {
        assert_eq!(super::managed_startup_path(Path::new(path)), path);
    }
    let framework = app_runtime::windows_system_directory()
        .unwrap()
        .parent()
        .unwrap()
        .join("Microsoft.NET/Framework64/v4.0.30319");
    let executable = fixture.0.join("ManagedPathProbe.exe");
    let mut arguments = vec![
        "/nologo".to_owned(),
        "/noconfig".to_owned(),
        "/langversion:5".to_owned(),
        "/warnaserror+".to_owned(),
        "/target:exe".to_owned(),
        format!("/out:{}", executable.display()),
    ];
    for assembly in [
        "System.dll",
        "System.Core.dll",
        "System.Runtime.Serialization.dll",
        "System.Xml.dll",
    ] {
        arguments.push(format!("/reference:{}", framework.join(assembly).display()));
    }
    for (name, source) in [
        (
            "SaveFiles.cs",
            include_str!("../../../../modules/corekeeper/control/SaveFiles.cs"),
        ),
        (
            "SaveProtocol.cs",
            include_str!("../../../../modules/corekeeper/control/SaveProtocol.cs"),
        ),
        (
            "ControlFiles.cs",
            include_str!("../../../../modules/valheim/control/ControlFiles.cs"),
        ),
        (
            "ControlProtocol.cs",
            include_str!("../../../../modules/valheim/control/ControlProtocol.cs"),
        ),
        (
            "NativeAccessPins.cs",
            include_str!("../../../../modules/valheim/control/NativeAccessPins.cs"),
        ),
        (
            "ValheimNativeControl.cs",
            include_str!("../../../../modules/valheim/control/ValheimNativeControl.cs"),
        ),
        (
            "ValheimWorldSave.cs",
            include_str!("../../../../modules/valheim/control/ValheimWorldSave.cs"),
        ),
        (
            "ManagedPathProbe.cs",
            r#"
using System;
using LanGame.SaveControl;
using Lgsm.Valheim.Control;
class ManagedPathProbe {
    static int Main(string[] args) {
        try {
            int rejected = 0;
            try { using (new SaveFiles(args[0])) { } }
            catch (SaveFailure error) {
                if (error.Code != "invalid_control_root") throw;
                rejected++;
            }
            try { using (new ControlFiles(args[0])) { } }
            catch (ControlFailure error) {
                if (error.Code != "invalid_control_root") throw;
                rejected++;
            }
            if (rejected != 2) throw new Exception("Verbatim roots bypassed the managed boundary");
            using (new SaveFiles(args[1])) { }
            using (new ControlFiles(args[1])) { }
            Console.WriteLine("2 verbatim roots rejected; 2 equivalent local roots accepted");
            return 0;
        } catch (Exception error) { Console.Error.WriteLine(error); return 1; }
    }
}
"#,
        ),
    ] {
        let path = fixture.0.join(name);
        fs::write(&path, source).unwrap();
        arguments.push(path.to_string_lossy().into_owned());
    }
    let compiled = app_runtime::capture_windows_utility(
        &framework.join("csc.exe"),
        &arguments.iter().map(String::as_str).collect::<Vec<_>>(),
        Duration::from_secs(30),
    )
    .unwrap();
    assert!(
        compiled.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&compiled.stdout),
        String::from_utf8_lossy(&compiled.stderr)
    );
    let observed = app_runtime::capture_windows_utility(
        &executable,
        &[canonical.to_str().unwrap(), &compatible],
        Duration::from_secs(30),
    )
    .unwrap();
    assert!(
        observed.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&observed.stdout),
        String::from_utf8_lossy(&observed.stderr)
    );
}

fn packet(nonce: &str, status: &str, issue: &str) -> Vec<u8> {
    format!("LGSM_SAVE_1\nmodule=corekeeper\nrun={}\nnonce={nonce}\npid=42\nstatus={status}\nissue={issue}\n","a".repeat(32)).into_bytes()
}

#[test]
fn native_acknowledgements_keep_request_and_disk_completion_distinct_for_every_profile() {
    for module in ["corekeeper", "vrising", "sonsoftheforest", "barotrauma"] {
        let game = profile(module).unwrap();
        let run = "a".repeat(32);
        let nonce = "b".repeat(32);
        for (status, expected) in [
            ("dispatched", WorldSaveCompletion::Requested),
            ("completed", WorldSaveCompletion::Confirmed),
        ] {
            let bytes = String::from_utf8(packet(&nonce, status, "none"))
                .unwrap()
                .replace("module=corekeeper", &format!("module={module}"));
            let reply = transport::reply(bytes.as_bytes(), game, &run, &nonce, 42).unwrap();
            assert_eq!(
                transport::completion(reply).unwrap(),
                Some(expected),
                "{module}: {status}"
            );
        }
    }
}

#[test]
fn native_pending_old_and_failed_replies_do_not_become_acknowledgements() {
    let game = profile("corekeeper").unwrap();
    let run = "a".repeat(32);
    let nonce = "b".repeat(32);
    for status in ["dispatching", "dispatched"] {
        let returned_nonce = if status == "dispatching" {
            nonce.clone()
        } else {
            "c".repeat(32)
        };
        let reply = transport::reply(
            &packet(&returned_nonce, status, "none"),
            game,
            &run,
            &nonce,
            42,
        )
        .unwrap();
        assert_eq!(transport::completion(reply).unwrap(), None);
    }
    for (status, issue) in [
        ("unknown", "native_outcome_unknown"),
        ("rejected", "save_busy"),
    ] {
        let reply =
            transport::reply(&packet(&nonce, status, issue), game, &run, &nonce, 42).unwrap();
        let error = transport::completion(reply).unwrap_err();
        assert!(error.contains(if status == "unknown" {
            "not resent"
        } else {
            "save_busy"
        }));
    }
}

#[test]
fn native_admission_is_pending_and_correlated_not_a_disk_receipt() {
    use transport::{Reply, reply};
    let game = profile("corekeeper").unwrap();
    let run = "a".repeat(32);
    let nonce = "b".repeat(32);
    assert_eq!(
        reply(
            &packet(&nonce, "dispatching", "none"),
            game,
            &run,
            &nonce,
            42
        )
        .unwrap(),
        Reply::Dispatching
    );
    assert_eq!(
        reply(
            &packet(&nonce, "dispatched", "none"),
            game,
            &run,
            &nonce,
            42
        )
        .unwrap(),
        Reply::Dispatched
    );
    assert_eq!(
        reply(
            &packet(&nonce, "unknown", "native_outcome_unknown"),
            game,
            &run,
            &nonce,
            42
        )
        .unwrap(),
        Reply::Unknown
    );
    assert_eq!(
        reply(
            &packet(&"c".repeat(32), "dispatched", "none"),
            game,
            &run,
            &nonce,
            42
        )
        .unwrap(),
        Reply::Old
    );
    assert!(
        reply(
            &packet(&nonce, "dispatched", "none"),
            game,
            &run,
            &nonce,
            43
        )
        .is_err()
    );
    assert!(
        reply(
            &packet(&nonce, "dispatched", "save_busy"),
            game,
            &run,
            &nonce,
            42
        )
        .is_err()
    );
    assert_eq!(
        reply(&packet(&nonce, "completed", "none"), game, &run, &nonce, 42).unwrap(),
        Reply::Completed
    );
    assert!(
        reply(
            &packet(&nonce, "completed", "save_busy"),
            game,
            &run,
            &nonce,
            42
        )
        .is_err()
    );
    assert!(reply(&packet(&nonce, "completed", "none"), game, &run, &nonce, 43).is_err());
    let mut duplicate = packet(&nonce, "dispatched", "none");
    duplicate.extend_from_slice(b"issue=none\n");
    assert!(reply(&duplicate, game, &run, &nonce, 42).is_err());
}

pub(in crate::commands) struct Fixture(pub(in crate::commands) std::path::PathBuf);
impl Fixture {
    pub(in crate::commands) fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "langame-managed-save-test-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir(&root).unwrap();
        Self(root)
    }

    pub(in crate::commands) fn unsupported_instance(
        &self,
        module: &str,
    ) -> (StorageBootstrap, InstanceDetails) {
        let runtime = self.0.join("runtime");
        std::fs::create_dir_all(&runtime).unwrap();
        std::fs::create_dir_all(self.0.join("config")).unwrap();
        std::fs::write(runtime.join(".langame-private-runtime"), b"managed\n").unwrap();
        let (image, files) = if module == "valheim" {
            (
                "valheim_server.exe",
                super::super::commands_valheim_control::NATIVE_PREPARATION_FILES.to_vec(),
            )
        } else {
            let game = profile(module).unwrap();
            (
                game.image,
                payload::native_files(game)
                    .unwrap()
                    .into_iter()
                    .map(|(path, _)| path)
                    .collect(),
            )
        };
        for relative in files.into_iter().chain([image]) {
            let path = runtime.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"unexecuted unsupported native image").unwrap();
        }
        let storage = app_storage::bootstrap_storage_with_paths(app_storage::StoragePaths {
            app_data_root: self.0.join("app-data"),
            settings_path: self.0.join("app-data/settings.json"),
            database_path: self.0.join("app-data/db/probe.db"),
            logs_root: self.0.join("logs"),
            modules_root: self.0.join("modules"),
            migrations_root: self.0.join("migrations"),
            steamcmd_root: self.0.join("steamcmd"),
            games_root: self.0.join("games"),
            instances_root: self.0.join("instances"),
            archives_root: self.0.join("instances/.trash"),
        })
        .unwrap();
        let details = serde_json::from_value(serde_json::json!({
            "summary":{"id":"optional-save","name":"Optional save","module_id":module,
                "status":"Stopped","bind_ip":"127.0.0.1","port_count":0,"autostart":false},
            "config_file_path":self.0.join("config/instance.json"),
            "saves_path":self.0.join("data/worlds"),"auto_backup_on_stop":false,
            "backup_retention_count":0,"settings_json":"{}","ports":[],"active_run":null
        }))
        .unwrap();
        (storage, details)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let root = &self.0;
        assert!(root.parent() == Some(std::env::temp_dir().as_path()));
        let _ = std::fs::remove_dir_all(root);
    }
}
#[test]
fn unknown_plugin_and_control_directory_are_preserved() {
    let fixture = Fixture::new();
    let game = profile("vrising").unwrap();
    std::fs::create_dir_all(fixture.0.join("BepInEx/plugins")).unwrap();
    std::fs::write(fixture.0.join(game.plugin), b"user mod").unwrap();
    assert!(payload::inspect(&fixture.0, game).is_err());
    assert_eq!(
        std::fs::read(fixture.0.join(game.plugin)).unwrap(),
        b"user mod"
    );
    std::fs::remove_file(fixture.0.join(game.plugin)).unwrap();
    std::fs::create_dir(fixture.0.join(DIRECTORY)).unwrap();
    assert!(payload::inspect(&fixture.0, game).is_err());
}
#[test]
fn owner_rejects_unknown_targets_and_accepts_known_interrupted_identity() {
    let fixture = Fixture::new();
    let game = profile("vrising").unwrap();
    std::fs::create_dir_all(fixture.0.join("BepInEx/plugins")).unwrap();
    std::fs::write(fixture.0.join(game.plugin), b"owned update").unwrap();
    let mut owner = Ownership {
        protocol: 1,
        module_id: game.module.into(),
        source_sha256: payload::source_hash(game),
        files: BTreeMap::new(),
        pending_files: BTreeMap::from([(game.plugin.into(), io::digest(b"owned update"))]),
    };
    std::fs::write(fixture.0.join(OWNER), serde_json::to_vec(&owner).unwrap()).unwrap();
    assert!(payload::inspect(&fixture.0, game).is_ok());
    owner.files.insert("other.dll".into(), "bad".into());
    std::fs::write(fixture.0.join(OWNER), serde_json::to_vec(&owner).unwrap()).unwrap();
    assert!(payload::inspect(&fixture.0, game).is_err());
}

#[derive(Clone)]
struct TestLease(Arc<()>);
impl crate::state::StorageContextTaskLease for TestLease {}
#[tokio::test]
async fn cancelled_save_waiter_retains_instance_and_storage_until_worker_finishes() {
    let lock = Arc::new(tokio::sync::Mutex::new(()));
    let mutation = lock.clone().lock_owned().await;
    let lease = TestLease(Arc::new(()));
    let alive = Arc::downgrade(&lease.0);
    let (entered, started) = tokio::sync::oneshot::channel();
    let (release, wait) = mpsc::channel();
    let task = tokio::spawn(async move {
        spawn_worker(&lease, Some(mutation), move || {
            let _ = entered.send(());
            wait.recv_timeout(Duration::from_secs(3)).unwrap();
        })
        .await
    });
    started.await.unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(lock.try_lock().is_err());
    assert!(alive.upgrade().is_some());
    release.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while lock.try_lock().is_err() || alive.upgrade().is_some() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
