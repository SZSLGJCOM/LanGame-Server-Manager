use super::*;
use crate::state::DesktopState;
use app_core::{InstanceStatus, InstanceSummary};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

fn exited(run_id: i64) -> ExitedManagedProcess {
    ExitedManagedProcess {
        summary: InstanceSummary {
            id: "cancelled-reconciliation".into(),
            name: "Reconciliation fixture".into(),
            module_id: "demo".into(),
            status: InstanceStatus::Running,
            active_process_count: 1,
            bind_ip: "127.0.0.1".into(),
            port_count: 0,
            autostart: false,
        },
        session_id: Some("source-session".into()),
        run_id,
        process_key: "main".into(),
        display_name: "Fixture".into(),
        pid: 42,
        log_path: "fixture.log".into(),
        is_primary: true,
        exit_code: Some(17),
    }
}

async fn mutation_guards(state: &DesktopState) -> HashMap<String, Arc<OwnedMutexGuard<()>>> {
    HashMap::from([(
        String::from("cancelled-reconciliation"),
        Arc::new(
            state
                .try_acquire_instance_mutation("cancelled-reconciliation")
                .await
                .unwrap(),
        ),
    )])
}

#[tokio::test]
async fn cancelled_waiter_keeps_exits_collected_by_its_blocking_worker() {
    let state = DesktopState::default();
    let owner = Arc::clone(&state.runtime_reconciliation);
    let lease = state
        .begin_storage_context_operation("reconciliation test")
        .unwrap();
    let admission = owner.acquire().await;
    let mutations = mutation_guards(&state).await;
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let worker_owner = Arc::clone(&owner);
    let waiter = tokio::spawn(async move {
        worker_owner
            .collect(admission, lease, mutations, move || {
                let _ = started_tx.send(());
                release_rx.recv_timeout(Duration::from_secs(2)).unwrap();
                Ok((vec![exited(7)], Vec::new()))
            })
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), started_rx)
        .await
        .unwrap()
        .unwrap();
    waiter.abort();
    release_tx.send(()).unwrap();
    let admission = tokio::time::timeout(Duration::from_secs(2), owner.acquire())
        .await
        .unwrap();
    let retained = owner.next().unwrap().unwrap();
    assert_eq!(retained.exited.run_id, 7);
    assert_eq!(retained.exited.exit_code, Some(17));
    assert!(state.begin_storage_context_transition().is_err());
    assert!(
        state
            .try_acquire_instance_mutation("cancelled-reconciliation")
            .await
            .is_none()
    );
    owner.acknowledge(&retained.exited).unwrap();
    drop(retained);
    drop(admission);
    assert!(state.begin_storage_context_transition().is_ok());
    assert!(
        state
            .try_acquire_instance_mutation("cancelled-reconciliation")
            .await
            .is_some()
    );
}

#[tokio::test]
async fn pending_exits_bound_the_queue_to_one_collected_batch() {
    let state = DesktopState::default();
    let owner = Arc::clone(&state.runtime_reconciliation);
    let lease = state
        .begin_storage_context_operation("reconciliation test")
        .unwrap();
    let admission = owner.acquire().await;
    owner
        .collect(
            Arc::clone(&admission),
            lease.clone(),
            mutation_guards(&state).await,
            || Ok((vec![exited(7)], Vec::new())),
        )
        .await
        .unwrap();
    owner
        .collect(Arc::clone(&admission), lease, HashMap::new(), || {
            panic!("pending exits must be acknowledged before another reap")
        })
        .await
        .unwrap();
    let retained = owner.next().unwrap().unwrap();
    assert_eq!(retained.exited.run_id, 7);
    owner.acknowledge(&retained.exited).unwrap();
    assert!(owner.next().unwrap().is_none());
}

#[tokio::test]
async fn persistence_finishes_after_caller_cancellation_and_is_not_replayed() {
    let state = DesktopState::default();
    let owner = Arc::clone(&state.runtime_reconciliation);
    let lease = state
        .begin_storage_context_operation("reconciliation test")
        .unwrap();
    let admission = owner.acquire().await;
    owner
        .collect(
            Arc::clone(&admission),
            lease.clone(),
            mutation_guards(&state).await,
            || Ok((vec![exited(7)], Vec::new())),
        )
        .await
        .unwrap();
    let mutation = owner.next().unwrap().unwrap().mutation;
    let calls = Arc::new(AtomicUsize::new(0));
    let writes = Arc::clone(&calls);
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let persistence = owner.persist(
        Arc::clone(&admission),
        lease.clone(),
        Arc::clone(&mutation),
        exited(7),
        async move {
            let _ = started_tx.send(());
            release_rx.await.unwrap();
            writes.fetch_add(1, Ordering::SeqCst);
            Ok(true)
        },
    );
    let waiter = tokio::spawn(persistence);
    started_rx.await.unwrap();
    waiter.abort();
    drop(admission);
    drop(mutation);
    release_tx.send(()).unwrap();
    let admission = tokio::time::timeout(Duration::from_secs(2), owner.acquire())
        .await
        .unwrap();
    assert!(owner.next().unwrap().unwrap().persisted);
    assert!(
        state
            .try_acquire_instance_mutation("cancelled-reconciliation")
            .await
            .is_none()
    );
    let mutation = owner.next().unwrap().unwrap().mutation;
    assert!(
        owner
            .persist(admission, lease, mutation, exited(7), async {
                panic!("a committed exit must not be marked again")
            })
            .await
            .unwrap()
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    owner.acknowledge(&exited(7)).unwrap();
    assert!(
        state
            .try_acquire_instance_mutation("cancelled-reconciliation")
            .await
            .is_some()
    );
}

#[tokio::test]
async fn failed_persistence_keeps_the_exit_for_retry() {
    let state = DesktopState::default();
    let owner = Arc::clone(&state.runtime_reconciliation);
    let lease = state
        .begin_storage_context_operation("reconciliation test")
        .unwrap();
    let admission = owner.acquire().await;
    owner
        .collect(
            Arc::clone(&admission),
            lease.clone(),
            mutation_guards(&state).await,
            || Ok((vec![exited(7)], Vec::new())),
        )
        .await
        .unwrap();
    let mutation = owner.next().unwrap().unwrap().mutation;
    assert!(
        owner
            .persist(admission, lease, mutation, exited(7), async {
                Err("controlled persistence failure".into())
            })
            .await
            .is_err()
    );
    assert!(!owner.next().unwrap().unwrap().persisted);
    owner.acknowledge(&exited(7)).unwrap();
}

#[tokio::test]
async fn collection_releases_nonexited_instances_and_retains_shared_shard_locks_until_ack() {
    let state = DesktopState::default();
    let owner = Arc::clone(&state.runtime_reconciliation);
    let lease = state
        .begin_storage_context_operation("reconciliation lock test")
        .unwrap();
    let admission = owner.acquire().await;
    let mut mutations = mutation_guards(&state).await;
    mutations.insert(
        String::from("still-running"),
        Arc::new(
            state
                .try_acquire_instance_mutation("still-running")
                .await
                .unwrap(),
        ),
    );
    owner
        .collect(admission, lease, mutations, || {
            Ok((vec![exited(7), exited(8)], Vec::new()))
        })
        .await
        .unwrap();
    assert!(
        state
            .try_acquire_instance_mutation("still-running")
            .await
            .is_some()
    );
    assert!(
        state
            .try_acquire_instance_mutation("cancelled-reconciliation")
            .await
            .is_none()
    );
    owner.acknowledge(&exited(7)).unwrap();
    assert!(
        state
            .try_acquire_instance_mutation("cancelled-reconciliation")
            .await
            .is_none()
    );
    owner.acknowledge(&exited(8)).unwrap();
    assert!(
        state
            .try_acquire_instance_mutation("cancelled-reconciliation")
            .await
            .is_some()
    );
}
