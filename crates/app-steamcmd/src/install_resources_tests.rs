use std::process::{Child, Command, Stdio};
use std::time::Duration;

use super::*;
use crate::{InstallCancellation, acquire_game_lifecycle, acquire_steamcmd_operation};

fn deadline() -> InstallDeadline {
    InstallDeadline::new("resource lock regression", Duration::from_secs(2))
}

fn blocked_deadline() -> InstallDeadline {
    InstallDeadline::new("blocked resource regression", Duration::from_millis(80))
}

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = crate::tests::unique_test_root();
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    fn module(&self, suffix: &str) -> String {
        format!("{}-{suffix}", self.0.file_name().unwrap().to_string_lossy())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn unrelated_modules_progress_while_same_module_and_overlapping_paths_wait() {
    let fixture = Fixture::new();
    let first = fixture.0.join("first");
    let second = fixture.0.join("second");
    let module = fixture.module("first");
    let _held = acquire_game_lifecycle(
        crate::test_install_lock_context(),
        &module,
        std::slice::from_ref(&first),
        deadline(),
    )
    .await
    .unwrap();
    let other = acquire_game_lifecycle(
        crate::test_install_lock_context(),
        &fixture.module("other"),
        std::slice::from_ref(&second),
        deadline(),
    )
    .await
    .unwrap();
    drop(other);
    for (id, root) in [
        (module.clone(), second.clone()),
        (fixture.module("alias"), first.clone()),
        (fixture.module("child"), first.join("nested")),
        (fixture.module("parent"), fixture.0.clone()),
    ] {
        assert!(matches!(
            acquire_game_lifecycle(
                crate::test_install_lock_context(),
                &id,
                &[root],
                blocked_deadline()
            )
            .await,
            Err(SteamCmdError::OperationTimedOut { .. })
        ));
    }
    // A failed multi-lock acquisition must release its module lease too.
    let _released = acquire_game_lifecycle(
        crate::test_install_lock_context(),
        &fixture.module("child"),
        &[second],
        deadline(),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn scoped_guards_reject_wrong_module_and_uncovered_target() {
    let fixture = Fixture::new();
    let root = fixture.0.join("game");
    let module = fixture.module("scope");
    let held = acquire_game_lifecycle(
        crate::test_install_lock_context(),
        &module,
        &[root.clone(), root.join("nested")],
        deadline(),
    )
    .await
    .unwrap();
    held.ensure_scope(&module, &root.join("nested")).unwrap();
    assert!(held.ensure_scope("different", &root).is_err());
    assert!(
        held.ensure_scope(&module, &fixture.0.join("other"))
            .is_err()
    );
    assert!(
        ResourceLocks::acquire(
            crate::test_install_lock_context(),
            &[&module],
            &[],
            deadline()
        )
        .await
        .is_err()
    );
}

#[test]
fn resource_identity_is_stable_when_missing_directories_are_created() {
    let fixture = Fixture::new();
    let root = fixture.0.join("missing").join("program");
    let before = canonical_resource_path(&root).unwrap();
    fs::create_dir_all(&root).unwrap();
    assert_eq!(before, canonical_resource_path(&root).unwrap());
    #[cfg(windows)]
    assert_eq!(
        before,
        canonical_resource_path(&PathBuf::from(root.to_string_lossy().to_uppercase())).unwrap()
    );
    assert!(canonical_resource_path(&fixture.0.join("..").join("other")).is_err());
}

#[tokio::test]
async fn steam_runtime_is_serialized_without_blocking_unrelated_game_work() {
    let fixture = Fixture::new();
    let settings = app_core::AppSettings {
        steamcmd_root: fixture.0.join("steamcmd").to_string_lossy().into_owned(),
        ..Default::default()
    };
    fs::create_dir_all(&settings.steamcmd_root).unwrap();
    fs::write(
        Path::new(&settings.steamcmd_root).join("steamcmd.exe"),
        b"fixture",
    )
    .unwrap();
    let _steam =
        acquire_steamcmd_operation(crate::test_install_lock_context(), &settings, deadline())
            .await
            .unwrap();
    let _game = acquire_game_lifecycle(
        crate::test_install_lock_context(),
        &fixture.module("archive"),
        &[fixture.0.join("archive")],
        deadline(),
    )
    .await
    .unwrap();
    assert!(matches!(
        acquire_steamcmd_operation(
            crate::test_install_lock_context(),
            &settings,
            blocked_deadline()
        )
        .await,
        Err(SteamCmdError::OperationTimedOut { .. })
    ));
    let other = app_core::AppSettings {
        steamcmd_root: fixture
            .0
            .join("other-steamcmd")
            .to_string_lossy()
            .into_owned(),
        ..settings
    };
    fs::create_dir_all(&other.steamcmd_root).unwrap();
    fs::write(
        Path::new(&other.steamcmd_root).join("steamcmd.exe"),
        b"fixture",
    )
    .unwrap();
    let _other = acquire_steamcmd_operation(crate::test_install_lock_context(), &other, deadline())
        .await
        .unwrap();
}

#[tokio::test]
async fn cancellation_releases_partial_resource_acquisition() {
    let fixture = Fixture::new();
    let root = fixture.0.join("game");
    let _held = ResourceLocks::acquire(
        crate::test_install_lock_context(),
        &[],
        std::slice::from_ref(&root),
        deadline(),
    )
    .await
    .unwrap();
    let cancellation = InstallCancellation::new();
    let token = cancellation.clone();
    let module = fixture.module("cancel");
    let waiting_module = module.clone();
    let waiter = tokio::spawn(async move {
        token
            .scope(acquire_game_lifecycle(
                crate::test_install_lock_context(),
                &waiting_module,
                &[root],
                deadline(),
            ))
            .await
    });
    tokio::task::yield_now().await;
    cancellation.cancel();
    assert!(matches!(
        waiter.await.unwrap(),
        Err(SteamCmdError::InstallCancelled { .. })
    ));
    let _next = acquire_game_lifecycle(
        crate::test_install_lock_context(),
        &module,
        &[fixture.0.join("other")],
        deadline(),
    )
    .await
    .unwrap();
}

const CHILD_ROOT: &str = "LANGAME_RESOURCE_LOCK_TEST_ROOT";
const CHILD_LOCK_ROOT: &str = "LANGAME_RESOURCE_LOCK_NAMESPACE";

#[tokio::test]
async fn overlapping_steam_and_game_roots_are_rejected_without_waiting() {
    let fixture = Fixture::new();
    for (game, steam) in [
        (fixture.0.join("same"), fixture.0.join("same")),
        (fixture.0.join("parent"), fixture.0.join("parent/steam")),
        (fixture.0.join("runtime/game"), fixture.0.join("runtime")),
    ] {
        fs::create_dir_all(&steam).unwrap();
        fs::write(steam.join("steamcmd.exe"), b"fixture").unwrap();
        let settings = app_core::AppSettings {
            steamcmd_root: steam.to_string_lossy().into_owned(),
            ..Default::default()
        };
        let guard = acquire_game_lifecycle(
            crate::test_install_lock_context(),
            &fixture.module("overlap"),
            &[game],
            deadline(),
        )
        .await
        .unwrap();
        let error = tokio::time::timeout(
            Duration::from_millis(200),
            guard.acquire_steamcmd(&settings),
        )
        .await
        .expect("overlap fails before attempting to acquire our own lock")
        .err()
        .expect("overlapping roots must be rejected");
        assert!(error.to_string().contains("overlap"));
        assert!(error.to_string().contains("separate"));
    }
}

#[tokio::test]
async fn recovery_acquires_overlapping_modules_as_one_resource_union() {
    let fixture = Fixture::new();
    let parent = fixture.0.join("program");
    let child = parent.join("nested");
    let first = fixture.module("batch-first");
    let second = fixture.module("batch-second");
    let resources = [
        (first.clone(), vec![parent.clone()]),
        (second.clone(), vec![child.clone(), parent.clone()]),
    ];
    let guard = tokio::time::timeout(
        Duration::from_secs(2),
        crate::acquire_game_install_lifecycles(crate::test_install_lock_context(), &resources),
    )
    .await
    .expect("batch overlaps cannot wait on the batch itself")
    .unwrap();
    guard.ensure_scope(&first, &parent).unwrap();
    guard.ensure_scope(&second, &child).unwrap();
    assert!(guard.ensure_scope("unrelated-module", &parent).is_err());
    for (module, root) in [
        (first, fixture.0.join("elsewhere")),
        (fixture.module("competitor"), child),
    ] {
        assert!(matches!(
            acquire_game_lifecycle(
                crate::test_install_lock_context(),
                &module,
                &[root],
                blocked_deadline()
            )
            .await,
            Err(SteamCmdError::OperationTimedOut { .. })
        ));
    }
}

struct ChildHolder(Child);
impl Drop for ChildHolder {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test]
async fn captured_context_preserves_cross_process_scope_when_environment_changes() {
    let fixture = Fixture::new();
    let user_root = fixture.0.join("user");
    let context = crate::InstallLockContext::new(user_root.clone());
    let changed_environment = fixture.0.join("changed-environment");
    fs::create_dir_all(&changed_environment).unwrap();
    let steam_root = fixture.0.join("steamcmd");
    fs::create_dir_all(&steam_root).unwrap();
    fs::write(steam_root.join("steamcmd.exe"), b"fixture").unwrap();
    let mut child = ChildHolder(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "install_resources::tests::hold_resource_in_child_process",
                "--nocapture",
            ])
            .env(CHILD_ROOT, &fixture.0)
            .env(CHILD_LOCK_ROOT, &user_root)
            // Change only the child process's environment. A stale ambient
            // namespace lookup would no longer contend with the parent.
            .envs(
                [
                    "LOCALAPPDATA",
                    "XDG_DATA_HOME",
                    "HOME",
                    "USERPROFILE",
                    "TEMP",
                    "TMP",
                    "TMPDIR",
                ]
                .map(|name| (name, &changed_environment)),
            )
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    tokio::time::timeout(Duration::from_secs(5), async {
        while !fixture.0.join("ready").exists() {
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "child exited before acquiring resource"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let held_root = fixture.0.join("held");
    let _independent =
        ResourceLocks::acquire(&context, &[], &[fixture.0.join("independent")], deadline())
            .await
            .unwrap();
    for root in [&held_root, &fixture.0, &held_root.join("nested")] {
        assert!(matches!(
            ResourceLocks::acquire(
                &context,
                &[],
                std::slice::from_ref(root),
                blocked_deadline()
            )
            .await,
            Err(SteamCmdError::OperationTimedOut { .. })
        ));
    }
    let game = acquire_game_lifecycle(
        &context,
        &fixture.module("nested-runtime"),
        &[fixture.0.join("game")],
        deadline(),
    )
    .await
    .unwrap();
    // The derived SteamCMD lease must retain the game lease's captured domain.
    drop(context);
    let settings = app_core::AppSettings {
        steamcmd_root: steam_root.to_string_lossy().into_owned(),
        ..Default::default()
    };
    assert!(matches!(
        game.acquire_steamcmd_with_deadline(&settings, blocked_deadline())
            .await,
        Err(SteamCmdError::OperationTimedOut { .. })
    ));
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    drop(
        game.acquire_steamcmd_with_deadline(&settings, deadline())
            .await
            .unwrap(),
    );
    let recreated_context = crate::InstallLockContext::new(user_root);
    ResourceLocks::acquire(&recreated_context, &[], &[held_root], deadline())
        .await
        .unwrap();
    assert_eq!(fs::read_dir(&changed_environment).unwrap().count(), 0);
}

#[tokio::test]
async fn hold_resource_in_child_process() {
    let Some(root) = std::env::var_os(CHILD_ROOT).map(PathBuf::from) else {
        return;
    };
    let context = crate::InstallLockContext::new(
        std::env::var_os(CHILD_LOCK_ROOT)
            .expect("parent supplies captured lock namespace")
            .into(),
    );
    let _guard = ResourceLocks::acquire(
        &context,
        &[],
        &[root.join("held"), root.join("steamcmd")],
        deadline(),
    )
    .await
    .unwrap();
    fs::write(root.join("ready"), b"ready").unwrap();
    tokio::time::sleep(Duration::from_secs(15)).await;
}

#[tokio::test]
async fn independent_user_namespaces_do_not_share_process_gates() {
    let fixture = Fixture::new();
    let first_context = crate::InstallLockContext::new(fixture.0.join("first-user"));
    let second_context = crate::InstallLockContext::new(fixture.0.join("second-user"));
    let resource = fixture.0.join("game");
    let _first = ResourceLocks::acquire(
        &first_context,
        &["same-module"],
        std::slice::from_ref(&resource),
        deadline(),
    )
    .await
    .unwrap();
    let _second =
        ResourceLocks::acquire(&second_context, &["same-module"], &[resource], deadline())
            .await
            .unwrap();
}
