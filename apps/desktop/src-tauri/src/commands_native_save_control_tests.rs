use super::*;
use std::sync::{Arc, mpsc};

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

fn packet(nonce: &str, status: &str, issue: &str) -> Vec<u8> {
    format!("LGSM_SAVE_1\nmodule=corekeeper\nrun={}\nnonce={nonce}\npid=42\nstatus={status}\nissue={issue}\n","a".repeat(32)).into_bytes()
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

#[test]
fn terminal_write_failures_are_module_scoped_and_cannot_claim_completion() {
    use transport::{Reply, reply};
    let run = "a".repeat(32);
    let nonce = "b".repeat(32);
    for (module, issue) in [("vrising", "vrising_write_failed")] {
        let frame = |status: &str| {
            String::from_utf8(packet(&nonce, status, issue))
                .unwrap()
                .replace("module=corekeeper", &format!("module={module}"))
                .into_bytes()
        };
        let game = profile(module).unwrap();
        assert_eq!(
            reply(&frame("failed"), game, &run, &nonce, 42).unwrap(),
            Reply::Failed(issue.into())
        );
        for status in [
            "completed",
            "dispatching",
            "dispatched",
            "unknown",
            "rejected",
        ] {
            assert!(reply(&frame(status), game, &run, &nonce, 42).is_err());
        }
        assert!(reply(&frame("failed"), game, &run, &nonce, 43).is_err());
        assert!(
            reply(
                &packet(&nonce, "failed", issue),
                profile("corekeeper").unwrap(),
                &run,
                &nonce,
                42
            )
            .is_err()
        );
    }
}
pub(super) struct Fixture(pub(super) std::path::PathBuf);
impl Fixture {
    pub(super) fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "langame-managed-save-test-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir(&root).unwrap();
        Self(root)
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
