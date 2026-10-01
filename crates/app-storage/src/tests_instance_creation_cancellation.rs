use super::*;
use crate::instance_creation_io::test_gate::{PausePoint, pause_at};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[tokio::test]
async fn explicit_cancellation_rejects_creation_before_admission() {
    let (root, paths, descriptor) = prepare_test().await;
    let error = create_instance_with_options(
        &paths,
        &descriptor,
        creation_input(),
        InstanceCreationOptions {
            cancellation: Some(Arc::new(AtomicBool::new(true))),
            ..Default::default()
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(error, StorageError::InstanceCreationCancelled));
    assert_creation_absent(&paths).await;
    create_test_instance(&paths, &descriptor).await;
    cleanup_root(&root);
}

#[tokio::test]
async fn explicit_cancellation_releases_a_contended_module_lock_waiter() {
    let (root, paths, descriptor) = prepare_test().await;
    let held_lock = acquire_module_instance_creation_lock_blocking(&paths, "dontstarve").unwrap();
    let cancellation = Arc::new(AtomicBool::new(false));
    let mut pause = pause_at(&cancellation, PausePoint::Lock);
    let caller = spawn_creation(&paths, &descriptor, &cancellation, None);
    pause.reached().await;
    cancellation.store(true, Ordering::Release);
    drop(pause);
    assert_cancelled(caller).await;
    assert_creation_absent(&paths).await;
    drop(held_lock);
    create_test_instance(&paths, &descriptor).await;
    cleanup_root(&root);
}

#[tokio::test]
async fn explicit_cancellation_removes_partial_runtime_copy_and_releases_ownership() {
    assert_preparation_cancellation(PausePoint::Copy, false).await;
}

#[tokio::test]
async fn explicit_cancellation_removes_partial_copy_during_stream_hashing() {
    assert_preparation_cancellation(PausePoint::Hash, true).await;
}

#[tokio::test]
async fn explicit_cancellation_during_local_metadata_scan_preserves_source_ownership() {
    assert_preparation_cancellation(PausePoint::Inspect, false).await;
}

async fn assert_preparation_cancellation(point: PausePoint, projection: bool) {
    let (root, paths, descriptor) = prepare_test().await;
    let shared_root = paths.games_root.join("dontstarve");
    fs::write(shared_root.join("payload.fixture"), vec![42_u8; 512 * 1024]).unwrap();
    let cancellation = Arc::new(AtomicBool::new(false));
    let mut pause = pause_at(&cancellation, point);
    let private_runtime = projection.then(|| PrivateRuntimeProjection {
        private_directories: vec![PathBuf::from("private-data")],
    });
    let caller = spawn_creation(&paths, &descriptor, &cancellation, private_runtime);
    pause.reached().await;
    let pending = managed_instance_directories(&paths);
    assert_eq!(pending.len(), 1);
    if projection || point == PausePoint::Copy {
        assert!(pending[0].join("runtime.staging").is_dir());
        assert!(shared_root.join("payload.fixture").is_file());
    } else {
        assert!(shared_root.join("payload.fixture").is_file());
        assert!(!pending[0].join("runtime").exists());
    }
    assert!(
        !pending[0]
            .join("runtime.staging/.langame-package-baseline.json")
            .exists()
    );
    cancellation.store(true, Ordering::Release);
    drop(pause);
    assert_cancelled(caller).await;
    assert_creation_absent(&paths).await;
    // A completed follow-up crosses the same module lock, SQLite connection,
    // filesystem publication that cancellation released.
    let created = create_test_instance(&paths, &descriptor).await;
    assert_eq!(
        fs::read(instance_private_runtime_root(&created).join("payload.fixture")).unwrap(),
        vec![42_u8; 512 * 1024]
    );
    assert_eq!(list_instances(&paths).await.unwrap().len(), 1);
    cleanup_root(&root);
}

fn creation_input() -> CreateInstanceInput {
    CreateInstanceInput {
        name: String::from("Explicit cancellation"),
        module_id: String::from("dontstarve"),
    }
}

fn spawn_creation(
    paths: &StoragePaths,
    descriptor: &ModuleDescriptor,
    cancellation: &Arc<AtomicBool>,
    private_runtime: Option<PrivateRuntimeProjection>,
) -> tokio::task::JoinHandle<Result<CreateInstanceResult, StorageError>> {
    let paths = paths.clone();
    let descriptor = descriptor.clone();
    let cancellation = Arc::clone(cancellation);
    tokio::spawn(async move {
        create_instance_with_options(
            &paths,
            &descriptor,
            creation_input(),
            InstanceCreationOptions {
                private_runtime,
                cancellation: Some(cancellation),
                ..Default::default()
            },
        )
        .await
    })
}

async fn assert_cancelled(
    caller: tokio::task::JoinHandle<Result<CreateInstanceResult, StorageError>>,
) {
    let error = tokio::time::timeout(Duration::from_secs(10), caller)
        .await
        .expect("explicitly cancelled creation did not settle")
        .unwrap()
        .unwrap_err();
    assert!(
        matches!(error, StorageError::InstanceCreationCancelled),
        "{error}"
    );
}

async fn assert_creation_absent(paths: &StoragePaths) {
    assert!(managed_instance_directories(paths).is_empty());
    assert!(list_instances(paths).await.unwrap().is_empty());
}
