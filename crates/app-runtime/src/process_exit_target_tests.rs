use super::*;
use std::io::{Read, Write};
use std::os::windows::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, mpsc};
use std::time::Duration;

const PROBE_TEST: &str = "process_exit_target::tests::exit_target_probe";

#[test]
#[ignore = "internal synthetic child; exercised by process exit target tests"]
fn exit_target_probe() {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut byte = [0];
        let _ = sender.send(std::io::stdin().read_exact(&mut byte));
    });
    receiver
        .recv_timeout(Duration::from_secs(20))
        .expect("probe exit request deadline")
        .expect("probe exit request");
}

pub(crate) struct Fixture {
    child: Child,
    handle: WindowsProcessHandle,
    identity: ProcessIdentity,
}

impl Fixture {
    pub(crate) fn new() -> Self {
        let child = Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", PROBE_TEST, "--nocapture"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(crate::CREATE_NO_WINDOW)
            .spawn()
            .expect("spawn isolated exit target");
        let handle = WindowsProcessHandle::open(child.id(), PROCESS_TERMINATE)
            .unwrap()
            .expect("probe remains alive");
        let identity = handle.identity().unwrap();
        Self {
            child,
            handle,
            identity,
        }
    }

    pub(crate) fn capture(&self) -> ProcessExitTarget {
        ProcessExitTarget::capture(self.child.id(), &self.identity)
            .unwrap()
            .expect("probe remains alive")
    }

    pub(crate) fn exit_normally(&mut self) {
        self.child
            .stdin
            .take()
            .unwrap()
            .write_all(b"x")
            .expect("release probe");
        assert!(self.handle.wait_for_exit(5000).unwrap());
        assert!(self.child.try_wait().unwrap().unwrap().success());
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if self.handle.is_running().unwrap_or(true) {
            let _ = self.child.kill();
        }
        let _ = self.handle.wait_for_exit(1500);
        let _ = self.child.try_wait();
    }
}

#[test]
fn capture_rejects_mismatched_identity_without_terminating_process() {
    let fixture = Fixture::new();
    let mut wrong_creation = fixture.identity.clone();
    wrong_creation.creation_time += 1;
    let mut wrong_image = fixture.identity.clone();
    wrong_image.image_path.push_str(".unrelated");
    for identity in [&wrong_creation, &wrong_image] {
        assert!(matches!(
            ProcessExitTarget::capture(fixture.child.id(), identity),
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied
        ));
        assert!(fixture.handle.is_running().unwrap());
    }
}

#[test]
fn capture_and_retained_target_recognize_normal_exit() {
    let mut fixture = Fixture::new();
    let target = fixture.capture();
    fixture.exit_normally();
    assert!(!target.is_running().unwrap());
    target.terminate().unwrap();
    assert!(
        ProcessExitTarget::capture(fixture.child.id(), &fixture.identity)
            .unwrap()
            .is_none()
    );
}

#[test]
fn termination_uses_retained_handle_across_threads_and_preserves_other_processes() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ProcessExitTarget>();

    let fixture = Fixture::new();
    let other = Fixture::new();
    let target = Arc::new(fixture.capture());
    assert!(target.is_running().unwrap());
    let worker_target = Arc::clone(&target);
    std::thread::spawn(move || worker_target.terminate())
        .join()
        .unwrap()
        .unwrap();
    assert!(fixture.handle.wait_for_exit(5000).unwrap());
    assert!(!target.is_running().unwrap());
    target.terminate().unwrap();
    assert!(other.handle.is_running().unwrap());
}
