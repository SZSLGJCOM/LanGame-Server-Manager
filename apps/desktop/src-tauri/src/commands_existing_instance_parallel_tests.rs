use super::*;
use app_core::PortBinding;
use std::sync::atomic::AtomicUsize;
use tokio::sync::{Barrier, watch};

fn candidates(count: usize) -> Vec<Candidate> {
    (0..count)
        .map(|index| {
            Candidate::from_bindings(
                &index.to_string(),
                "terraria",
                &[PortBinding {
                    name: "game".into(),
                    protocol: "tcp".into(),
                    port: 7000 + index as u16,
                }],
            )
            .unwrap()
        })
        .collect()
}

fn completed(candidate: &Candidate, passed: bool) -> Receipt {
    Receipt {
        instance_id: candidate.instance_id.clone(),
        module_id: candidate.module_id.clone(),
        passed,
        ..Default::default()
    }
}

// The two surviving workers cannot finish until the failed worker is reaped.
// Their counters therefore distinguish joining/draining from dropping JoinSet.
async fn interrupted_batch(mode: &'static str) {
    let barrier = Arc::new(Barrier::new(3));
    let stopped = Arc::new(AtomicUsize::new(0));
    let recovered = Arc::new(AtomicUsize::new(0));
    let (release, wait) = watch::channel(false);
    let worker_stopped = stopped.clone();
    let recovery_count = recovered.clone();
    let result = drive(
        candidates(4),
        Ownership::default(),
        move |candidate, ownership| {
            let barrier = barrier.clone();
            let stopped = worker_stopped.clone();
            let mut wait = wait.clone();
            async move {
                assert!(ownership.mark_start_requested(&candidate.instance_id).await);
                barrier.wait().await;
                if candidate.instance_id == "0" {
                    assert_ne!(mode, "panic", "intentional worker panic");
                    return (completed(&candidate, false), mode != "unsafe");
                }
                wait.changed().await.unwrap();
                stopped.fetch_add(1, Ordering::SeqCst);
                (completed(&candidate, true), true)
            }
        },
        move |candidate, ownership| {
            let recovered = recovery_count.clone();
            async move {
                assert!(
                    ownership
                        .state
                        .read()
                        .await
                        .start_requested
                        .contains(&candidate.instance_id)
                );
                recovered.fetch_add(1, Ordering::SeqCst);
                (completed(&candidate, false), false)
            }
        },
        || Ok(MIN_AVAILABLE_MEMORY),
        |candidate, _, _| {
            if candidate.instance_id == "0" {
                release.send(true).unwrap();
                if mode == "receipt" {
                    return Err("receipt write failed".into());
                }
            }
            Ok(())
        },
    )
    .await
    .unwrap();
    assert_eq!(result.admitted, 3);
    assert_eq!(result.completed, 3);
    assert_eq!(result.pending, 1);
    assert!(result.halted);
    assert_eq!(stopped.load(Ordering::SeqCst), 2);
    assert_eq!(
        recovered.load(Ordering::SeqCst),
        usize::from(mode == "panic")
    );
}

#[tokio::test]
async fn existing_parallel_unsafe_completion_drains_admitted_workers() {
    interrupted_batch("unsafe").await;
}

#[tokio::test]
async fn existing_parallel_receipt_failure_drains_admitted_workers() {
    interrupted_batch("receipt").await;
}

#[tokio::test]
async fn existing_parallel_worker_panic_recovers_then_drains_admitted_workers() {
    interrupted_batch("panic").await;
}

#[tokio::test]
async fn existing_parallel_safe_game_failure_continues_pending_instances() {
    let result = drive(
        candidates(5),
        Ownership::default(),
        |candidate, _| async move { (completed(&candidate, candidate.instance_id != "0"), true) },
        |_, _| async { panic!("recovery must not run") },
        || Ok(MIN_AVAILABLE_MEMORY),
        |_, _, _| Ok(()),
    )
    .await
    .unwrap();
    assert_eq!(
        (
            result.admitted,
            result.completed,
            result.passed,
            result.pending
        ),
        (5, 5, 4, 0)
    );
    assert!(!result.halted);
}

#[tokio::test]
async fn existing_parallel_completed_unsafe_sibling_closes_admission_before_reaping() {
    let ownership = Ownership::default();
    let observed = ownership.clone();
    let (completed_tx, mut completions) = tokio::sync::mpsc::unbounded_channel();
    let driver = drive(
        candidates(4),
        ownership,
        move |candidate, _| {
            let completed_tx = completed_tx.clone();
            async move {
                completed_tx.send(candidate.instance_id.clone()).unwrap();
                (completed(&candidate, true), candidate.instance_id != "1")
            }
        },
        |_, _| async { panic!("recovery must not run") },
        || Ok(MIN_AVAILABLE_MEMORY),
        |_, _, _| Ok(()),
    );
    let check = async {
        for _ in 0..3 {
            completions.recv().await.unwrap();
        }
        assert!(observed.halted());
    };
    let (result, ()) = tokio::join!(driver, check);
    let result = result.unwrap();
    assert_eq!(
        (result.admitted, result.completed, result.pending),
        (3, 3, 1)
    );
    assert!(result.halted);
}

#[tokio::test]
async fn existing_parallel_low_memory_stops_admission_and_drains_existing_worker() {
    let mut reads = 0;
    let result = drive(
        candidates(3),
        Ownership::default(),
        |candidate, _| async move { (completed(&candidate, true), true) },
        |_, _| async { panic!("recovery must not run") },
        || {
            reads += 1;
            Ok(if reads == 1 { MIN_AVAILABLE_MEMORY } else { 0 })
        },
        |_, _, _| Ok(()),
    )
    .await
    .unwrap();
    assert_eq!(
        (result.admitted, result.completed, result.pending),
        (1, 1, 2)
    );
    assert!(result.halted);
    assert_eq!(result.failures, ["available_physical_memory_below_8_gib"]);
}

#[test]
fn existing_parallel_large_heap_and_multiple_shards_tighten_admission() {
    for (module, settings, expected) in [
        ("minecraft", json!({"memory_max_mb": 2048}), false),
        ("minecraft", json!({"memory_max_mb": 8192}), true),
        ("minecraft", json!({}), true),
        ("dontstarve", json!({"enable_caves": false}), false),
        ("dontstarve", json!({"enable_caves": true}), true),
        (
            "dontstarve",
            json!({"shard_layout": "island_adventures"}),
            true,
        ),
    ] {
        let mut candidate = candidates(1).remove(0);
        candidate.module_id = module.into();
        tighten_resources(&mut candidate, &settings);
        assert_eq!(candidate.exclusive, expected, "{module}: {settings}");
    }
}
