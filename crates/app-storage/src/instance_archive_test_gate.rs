use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, mpsc};
use std::time::Duration;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum Point {
    Archiving,
    ArchiveMoved,
    Restoring,
    Dependencies,
    Packages,
    PackagesReady,
    NativeSettingsReady,
    BackupPreparing,
    BackupPublishing,
    BackupCleanup,
    BackupRollback,
}

struct Gate {
    started: tokio::sync::oneshot::Sender<()>,
    release: mpsc::Receiver<()>,
}

type Key = (PathBuf, Point);

fn gates() -> &'static Mutex<HashMap<Key, Gate>> {
    static GATES: OnceLock<Mutex<HashMap<Key, Gate>>> = OnceLock::new();
    GATES.get_or_init(Mutex::default)
}

pub(crate) struct Pause {
    key: Key,
    started: Option<tokio::sync::oneshot::Receiver<()>>,
    release: Option<mpsc::Sender<()>>,
}

impl Pause {
    pub(crate) async fn reached(&mut self) {
        tokio::time::timeout(Duration::from_secs(15), self.started.take().unwrap())
            .await
            .expect("archive worker did not reach its file phase")
            .expect("archive worker dropped its test gate");
    }

    pub(crate) fn resume(&mut self) {
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
    }
}

impl Drop for Pause {
    fn drop(&mut self) {
        self.resume();
        gates().lock().unwrap().remove(&self.key);
    }
}

pub(crate) fn register(database: &Path, point: Point) -> Pause {
    let key = (database.to_owned(), point);
    let (started, receiver) = tokio::sync::oneshot::channel();
    let (release, released) = mpsc::channel();
    assert!(
        gates()
            .lock()
            .unwrap()
            .insert(
                key.clone(),
                Gate {
                    started,
                    release: released,
                }
            )
            .is_none()
    );
    Pause {
        key,
        started: Some(receiver),
        release: Some(release),
    }
}

pub(crate) fn pause(database: &Path, point: Point) {
    let gate = gates()
        .lock()
        .unwrap()
        .remove(&(database.to_owned(), point));
    if let Some(gate) = gate {
        let _ = gate.started.send(());
        gate.release
            .recv_timeout(Duration::from_secs(15))
            .expect("archive test did not release its file phase");
    }
}
