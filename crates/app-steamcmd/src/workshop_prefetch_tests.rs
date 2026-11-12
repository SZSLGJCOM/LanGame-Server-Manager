use super::*;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

static NEXT: AtomicU64 = AtomicU64::new(1);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lg-prefetch-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }

    fn settings(&self) -> AppSettings {
        AppSettings {
            steamcmd_root: self.0.join("main-runtime").to_string_lossy().into_owned(),
            ..AppSettings::default()
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn write_cache(install: &Path, app: u32, entries: &[(&str, &str, &[u8])]) {
    let workshop = install.join("steamapps/workshop");
    let mut installed = String::new();
    for (id, manifest, payload) in entries {
        let path = workshop.join("content").join(app.to_string()).join(id);
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("payload.bin"), payload).unwrap();
        installed.push_str(&format!(
            "\"{id}\" {{ \"manifest\" \"{manifest}\" \"size\" \"{}\" }}\n",
            payload.len(),
        ));
    }
    // Missing details is a supported native layout, not an incomplete download.
    fs::write(workshop.join(format!("appworkshop_{app}.acf")), format!(
        "\"AppWorkshop\" {{ \"appid\" \"{app}\" \"PreservedField\" \"operator\" \"WorkshopItemsInstalled\" {{ {installed} }} }}"
    )).unwrap();
}

fn native_result(app: u32, entries: &[(&str, &Path)]) -> SteamWorkshopDownloadResult {
    let install = entries[0].1;
    SteamWorkshopDownloadResult {
        consumer_app_id: app,
        install_root: install.to_string_lossy().into_owned(),
        workshop_root: install
            .join("steamapps/workshop/content")
            .join(app.to_string())
            .to_string_lossy()
            .into_owned(),
        items: entries
            .iter()
            .map(|(id, root)| crate::SteamWorkshopDownloadItemResult {
                item_id: (*id).into(),
                expected_path: root
                    .join("steamapps/workshop/content")
                    .join(app.to_string())
                    .join(id)
                    .to_string_lossy()
                    .into_owned(),
                expected_path_exists: true,
            })
            .collect(),
        output_excerpt: "native validation succeeded".into(),
    }
}

fn proof(app: u32, entries: &[(&str, &Path)]) -> VerifiedWorkshopCache {
    VerifiedWorkshopCache::capture_native_download(
        native_result(app, entries),
        &InstallCancellation::new(),
    )
    .unwrap()
}

fn payload(install: &Path, app: u32, id: &str) -> Vec<u8> {
    fs::read(
        install
            .join("steamapps/workshop/content")
            .join(app.to_string())
            .join(id)
            .join("payload.bin"),
    )
    .unwrap()
}

#[test]
fn private_roots_reject_shared_runtime_and_nested_worker_paths() {
    let fixture = Fixture::new();
    let settings = fixture.settings();
    let runtime = fixture.0.join("worker");
    let download = fixture.0.join("attempt");
    let mut request = WorkshopPrefetchRequest {
        runtime_root: &runtime,
        download_root: &download,
        consumer_app_id: 42,
        ids: &[],
    };
    assert!(private_roots(&settings, &request).is_ok());
    request.runtime_root = Path::new(&settings.steamcmd_root);
    assert!(private_roots(&settings, &request).is_err());
    request.runtime_root = &runtime;
    request.download_root = &runtime;
    assert!(private_roots(&settings, &request).is_err());
}

#[tokio::test]
async fn private_prepare_refuses_foreign_directory_without_discovering_main_runtime() {
    let fixture = Fixture::new();
    let runtime = fixture.0.join("foreign");
    fs::create_dir(&runtime).unwrap();
    fs::write(runtime.join("keep"), b"existing data").unwrap();
    let mut settings = fixture.settings();
    settings.steamcmd_root = runtime.to_string_lossy().into_owned();
    let result = crate::steamcmd_prepare::prepare_private_runtime_locked(
        &settings,
        InstallDeadline::new("test", Duration::from_secs(1)),
        |_| {},
    )
    .await;
    assert!(matches!(
        result,
        Err(SteamCmdError::UnmanagedSteamCmdRoot { .. })
    ));
    assert_eq!(fs::read(runtime.join("keep")).unwrap(), b"existing data");
    assert_eq!(fs::read_dir(runtime).unwrap().count(), 1);
}

#[test]
fn scripts_for_the_same_app_are_unique_and_owned_by_each_runtime() {
    let fixture = Fixture::new();
    let first = crate::workshop_script::WorkshopScript::create(&fixture.0, 42, "first").unwrap();
    let second = crate::workshop_script::WorkshopScript::create(&fixture.0, 42, "second").unwrap();
    assert_ne!(first.path(), second.path());
    assert!(first.path().starts_with(&fixture.0));
    assert_eq!(fs::read_to_string(first.path()).unwrap(), "first");
    let first_path = first.path().to_owned();
    drop(first);
    assert!(!first_path.exists());
    assert_eq!(fs::read_to_string(second.path()).unwrap(), "second");
}

#[tokio::test]
async fn prefetch_refuses_existing_download_data_before_bootstrap_or_native_writes() {
    let fixture = Fixture::new();
    let runtime = fixture.0.join("worker");
    let download = fixture.0.join("existing-game");
    fs::create_dir(&download).unwrap();
    fs::write(download.join("save"), b"must survive").unwrap();
    let result = prefetch_workshop_items(
        &fixture.settings(),
        WorkshopPrefetchRequest {
            runtime_root: &runtime,
            download_root: &download,
            consumer_app_id: 42,
            ids: &["571001".into()],
        },
        crate::test_install_lock_context(),
        &InstallCancellation::new(),
        |_| {},
    )
    .await;
    assert!(result.is_err());
    assert_eq!(fs::read(download.join("save")).unwrap(), b"must survive");
    assert!(
        !runtime.exists(),
        "bootstrap must not run for an invalid download target"
    );
    assert_eq!(fs::read_dir(download).unwrap().count(), 1);
}

#[tokio::test]
async fn publication_merges_split_sources_and_preserves_unrequested_items() {
    let fixture = Fixture::new();
    let a = fixture.0.join("a");
    let b = fixture.0.join("b");
    let target = fixture.0.join("target");
    write_cache(&a, 42, &[("571001", "10", b"latest")]);
    write_cache(&b, 42, &[("571002", "20", b"second")]);
    write_cache(
        &target,
        42,
        &[("571001", "10", b"broken"), ("571003", "30", b"keep")],
    );
    let prepared = Arc::new(PreparedWorkshopDownload {
        verified: proof(42, &[("571001", &a), ("571002", &b)]),
        source: Arc::new(
            ResourceLocks::acquire(
                crate::test_install_lock_context(),
                &[],
                &[a, b],
                InstallDeadline::new("test", Duration::from_secs(2)),
            )
            .await
            .unwrap(),
        ),
    });
    let guard = crate::acquire_game_install_lifecycle(
        crate::test_install_lock_context(),
        "prefetch-publish",
        std::slice::from_ref(&target),
    )
    .await
    .unwrap();
    let result = publish_prefetched_workshop_items(prepared, target.clone(), &guard)
        .await
        .unwrap();
    assert_eq!(
        payload(&target, 42, "571001"),
        b"latest",
        "same-size corrupt target must be replaced"
    );
    assert_eq!(payload(&target, 42, "571002"), b"second");
    assert_eq!(payload(&target, 42, "571003"), b"keep");
    assert!(
        result
            .items
            .iter()
            .all(|item| Path::new(&item.expected_path).starts_with(&target))
    );
    let manifest =
        fs::read_to_string(target.join("steamapps/workshop/appworkshop_42.acf")).unwrap();
    assert!(
        manifest.contains("PreservedField")
            && manifest.contains("operator")
            && manifest.contains("571003")
    );
    assert!(
        target
            .join(".langame-workshop-membership/42/571001.json")
            .exists()
    );
    let content = Path::new(&result.workshop_root);
    assert!(
        crate::workshop_cache_integrity::receipt_path(content, 42, "571001")
            .unwrap()
            .exists()
    );
}

#[test]
fn changed_prefetch_source_is_rejected_before_target_mutation() {
    let fixture = Fixture::new();
    let source = fixture.0.join("source");
    let target = fixture.0.join("target");
    write_cache(&source, 42, &[("571001", "10", b"latest")]);
    write_cache(&target, 42, &[("571001", "9", b"before")]);
    let verified = proof(42, &[("571001", &source)]);
    fs::write(
        source.join("steamapps/workshop/content/42/571001/payload.bin"),
        b"broken",
    )
    .unwrap();
    let before = fs::read(target.join("steamapps/workshop/appworkshop_42.acf")).unwrap();
    assert!(publish(&verified, &target).is_err());
    assert_eq!(payload(&target, 42, "571001"), b"before");
    assert_eq!(
        fs::read(target.join("steamapps/workshop/appworkshop_42.acf")).unwrap(),
        before
    );
}

#[test]
fn publication_rolls_back_prior_item_when_later_swap_fails() {
    let fixture = Fixture::new();
    let source = fixture.0.join("source");
    let target = fixture.0.join("target");
    write_cache(
        &source,
        42,
        &[("579001", "10", b"latest"), ("579002", "10", b"latest")],
    );
    write_cache(
        &target,
        42,
        &[("579001", "9", b"before"), ("579002", "9", b"before")],
    );
    let verified = proof(42, &[("579001", &source), ("579002", &source)]);
    let before = fs::read(target.join("steamapps/workshop/appworkshop_42.acf")).unwrap();
    crate::dst_workshop_cache_publish::fail_item_backup_for_test("579002");
    assert!(publish(&verified, &target).is_err());
    assert_eq!(payload(&target, 42, "579001"), b"before");
    assert_eq!(payload(&target, 42, "579002"), b"before");
    assert_eq!(
        fs::read(target.join("steamapps/workshop/appworkshop_42.acf")).unwrap(),
        before
    );
}

#[cfg(windows)]
#[path = "workshop_prefetch_native_tests.rs"]
mod native;

#[tokio::test]
async fn failed_cleanup_keeps_original_failure_and_refuses_unowned_or_non_directory_cache() {
    let fixture = Fixture::new();
    let runtime = fixture.0.join("worker");
    let lease = ResourceLocks::acquire(
        crate::test_install_lock_context(),
        &[],
        std::slice::from_ref(&runtime),
        InstallDeadline::new("test", Duration::from_secs(2)),
    )
    .await
    .unwrap();
    fs::create_dir(&runtime).unwrap();
    fs::write(runtime.join("keep"), b"unowned data").unwrap();
    assert!(cleanup_worker_cache(&runtime, &lease).is_err());
    assert_eq!(fs::read(runtime.join("keep")).unwrap(), b"unowned data");
    fs::remove_file(runtime.join("keep")).unwrap();
    crate::prepare_configured_steamcmd_root(&runtime).unwrap();
    fs::create_dir(runtime.join("steamapps")).unwrap();
    fs::write(runtime.join("steamapps/workshop"), b"not a cache directory").unwrap();
    let original = SteamCmdError::SteamCmdCommandFailed {
        output_excerpt: "original native failure".into(),
    };
    let result: Result<(), _> = finish_worker_attempt(&runtime, &lease, Err(original));
    let error = result.unwrap_err().to_string();
    assert!(error.contains("original native failure"));
    assert!(error.contains("cleanup also failed"));
    assert_eq!(
        fs::read(runtime.join("steamapps/workshop")).unwrap(),
        b"not a cache directory"
    );
}

#[tokio::test]
async fn unconfirmed_native_exit_preserves_cache_and_original_cleanup_error() {
    let fixture = Fixture::new();
    let runtime = fixture.0.join("worker");
    crate::prepare_configured_steamcmd_root(&runtime).unwrap();
    write_cache(&runtime, 42, &[("571001", "10", b"latest")]);
    let lease = ResourceLocks::acquire(
        crate::test_install_lock_context(),
        &[],
        std::slice::from_ref(&runtime),
        InstallDeadline::new("test", Duration::from_secs(2)),
    )
    .await
    .unwrap();
    let result: Result<(), _> = finish_worker_attempt(
        &runtime,
        &lease,
        Err(SteamCmdError::InstallProcessCleanupFailed {
            operation: "fixture".into(),
            detail: "exit not confirmed".into(),
        }),
    );
    assert!(matches!(
        result,
        Err(SteamCmdError::InstallProcessCleanupFailed { .. })
    ));
    assert_eq!(payload(&runtime, 42, "571001"), b"latest");
}
