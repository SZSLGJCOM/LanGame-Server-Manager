use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::{Notify, mpsc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Requester {
    First,
    Second,
}

#[derive(Debug, PartialEq, Eq)]
enum Event {
    Acquired(Requester, String),
    WaitingForZ,
}

struct Schedule {
    state: DesktopState,
    ids: Mutex<Vec<String>>,
    first_holds_z: Notify,
    resume_first: Notify,
    events: mpsc::Sender<Event>,
}

struct ControlledAccess {
    schedule: Arc<Schedule>,
    requester: Requester,
    reads: AtomicUsize,
}

impl InstanceMutationAccess for ControlledAccess {
    async fn instance_ids(&self) -> Result<Vec<String>, String> {
        let read = self.reads.fetch_add(1, Ordering::SeqCst);
        if self.requester == Requester::First && read == 1 {
            // U1's first acquisition owns z. Pause its re-enumeration until U2
            // owns the newly created a and has actually polled the z lock.
            self.schedule.first_holds_z.notify_one();
            self.schedule.resume_first.notified().await;
        }
        Ok(self.schedule.ids.lock().unwrap().clone())
    }

    async fn acquire(&self, instance_id: &str) -> OwnedMutexGuard<()> {
        let acquisition = self.schedule.state.acquire_instance_mutation(instance_id);
        tokio::pin!(acquisition);
        if self.requester == Requester::Second && instance_id == "z" {
            let pending = std::future::poll_fn(|cx| {
                std::task::Poll::Ready(acquisition.as_mut().poll(cx).is_pending())
            })
            .await;
            assert!(
                pending,
                "U1 must still own z at the controlled interleaving"
            );
            self.schedule.events.send(Event::WaitingForZ).await.unwrap();
        }
        let guard = acquisition.await;
        self.schedule
            .events
            .send(Event::Acquired(self.requester, instance_id.into()))
            .await
            .unwrap();
        guard
    }
}

async fn next_event(events: &mut mpsc::Receiver<Event>) -> Event {
    tokio::time::timeout(Duration::from_secs(5), events.recv())
        .await
        .expect("controlled schedule made progress")
        .expect("event sender exists")
}

#[tokio::test]
async fn module_mutation_locks_reorder_after_creation_without_cross_uninstall_deadlock() {
    let (events, mut receiver) = mpsc::channel(16);
    let schedule = Arc::new(Schedule {
        state: DesktopState::default(),
        ids: Mutex::new(vec!["z".into()]),
        first_holds_z: Notify::new(),
        resume_first: Notify::new(),
        events,
    });
    let first_schedule = Arc::clone(&schedule);
    let first = tokio::spawn(async move {
        acquire_stable_instance_locks(
            &ControlledAccess {
                schedule: first_schedule,
                requester: Requester::First,
                reads: AtomicUsize::new(0),
            },
            Duration::from_secs(5),
        )
        .await
    });
    tokio::time::timeout(Duration::from_secs(5), schedule.first_holds_z.notified())
        .await
        .unwrap();
    assert_eq!(
        next_event(&mut receiver).await,
        Event::Acquired(Requester::First, "z".into())
    );
    schedule.ids.lock().unwrap().push("a".into());
    let second_schedule = Arc::clone(&schedule);
    let second = tokio::spawn(async move {
        let guards = acquire_stable_instance_locks(
            &ControlledAccess {
                schedule: second_schedule,
                requester: Requester::Second,
                reads: AtomicUsize::new(0),
            },
            Duration::from_secs(5),
        )
        .await?;
        let count = guards.len();
        drop(guards);
        Ok::<_, String>(count)
    });
    assert_eq!(
        next_event(&mut receiver).await,
        Event::Acquired(Requester::Second, "a".into())
    );
    assert_eq!(next_event(&mut receiver).await, Event::WaitingForZ);
    // Deadlock schedule: U1 owns z; U2 owns a and waits for z. U1 next sees a.
    schedule.resume_first.notify_one();
    assert_eq!(
        second
            .await
            .unwrap()
            .expect("U2 must acquire z before its deadline"),
        2
    );
    let guards = first
        .await
        .unwrap()
        .expect("U1 must reacquire in order before its deadline");
    assert_eq!(guards.len(), 2);
    assert!(
        schedule
            .state
            .try_acquire_instance_mutation("a")
            .await
            .is_none()
    );
    assert!(
        schedule
            .state
            .try_acquire_instance_mutation("z")
            .await
            .is_none()
    );
    drop(guards);
    assert!(
        schedule
            .state
            .try_acquire_instance_mutation("a")
            .await
            .is_some()
    );
    assert!(
        schedule
            .state
            .try_acquire_instance_mutation("z")
            .await
            .is_some()
    );
}

struct ChangingAccess {
    state: DesktopState,
    reads: AtomicUsize,
    fail_after_acquisition: bool,
}

impl InstanceMutationAccess for ChangingAccess {
    async fn instance_ids(&self) -> Result<Vec<String>, String> {
        let read = self.reads.fetch_add(1, Ordering::SeqCst);
        if self.fail_after_acquisition && read > 0 {
            return Err("fixture database read failed".into());
        }
        // Continually introduce a lower ID. No sleeps or external timing source
        // are required to exercise the bounded stable-set loop.
        Ok((0..=read)
            .map(|index| format!("{:020}", usize::MAX - index))
            .collect())
    }

    async fn acquire(&self, instance_id: &str) -> OwnedMutexGuard<()> {
        self.state.acquire_instance_mutation(instance_id).await
    }
}

#[tokio::test]
async fn module_mutation_locks_bound_continuous_creation_and_release_guards() {
    let access = ChangingAccess {
        state: DesktopState::default(),
        reads: AtomicUsize::new(0),
        fail_after_acquisition: false,
    };
    let error = acquire_stable_instance_locks(&access, Duration::from_millis(100))
        .await
        .unwrap_err();
    assert_eq!(error, TIMEOUT_MESSAGE);
    let reads = access.reads.load(Ordering::SeqCst);
    assert!(reads > 1);
    for read in [0, reads - 1] {
        let id = format!("{:020}", usize::MAX - read);
        assert!(
            access
                .state
                .try_acquire_instance_mutation(&id)
                .await
                .is_some()
        );
    }
}

#[tokio::test]
async fn module_mutation_locks_release_guards_after_database_failure() {
    let access = ChangingAccess {
        state: DesktopState::default(),
        reads: AtomicUsize::new(0),
        fail_after_acquisition: true,
    };
    let error = acquire_stable_instance_locks(&access, Duration::from_secs(5))
        .await
        .unwrap_err();
    assert_eq!(error, "fixture database read failed");
    assert!(
        access
            .state
            .try_acquire_instance_mutation(&format!("{:020}", usize::MAX))
            .await
            .is_some()
    );
}
