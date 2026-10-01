use super::*;
use std::io::Read;
use std::os::windows::process::CommandExt;
use std::sync::mpsc;
use std::time::Instant;

fn test_pipe() -> (File, File) {
    let mut read = std::ptr::null_mut();
    let mut write = std::ptr::null_mut();
    assert_ne!(
        unsafe { CreatePipe(&mut read, &mut write, std::ptr::null_mut(), 4096) },
        0
    );
    unsafe { (File::from_raw_handle(read), File::from_raw_handle(write)) }
}

#[test]
fn ordinary_stdin_unread_pipe_finishes_with_a_write_timeout() {
    let (read, write) = test_pipe();
    let (completed, completion) = mpsc::channel();
    let writer = std::thread::spawn(move || {
        let mut stdin = RuntimeStdin::Windows(WindowsPipeWriter::new(write));
        let result = stdin.write_stdin_line(&"x".repeat(64 * 1024));
        completed
            .send(result.map_err(|error| error.kind()))
            .unwrap();
    });
    let started = Instant::now();
    let result = completion.recv_timeout(Duration::from_secs(3));
    // Closing the read end also unblocks the old unbounded implementation, so
    // this regression never leaves a blocked test writer behind.
    drop(read);
    writer.join().unwrap();

    assert_eq!(result, Ok(Err(std::io::ErrorKind::TimedOut)));
    assert!(started.elapsed() < Duration::from_secs(4));
}

fn pipe_bytes_available(read: &File) -> usize {
    let mut available = 0;
    assert_ne!(
        unsafe {
            PeekNamedPipe(
                read.as_raw_handle(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                &mut available,
                std::ptr::null_mut(),
            )
        },
        0,
        "inspect controlled pipe: {}",
        std::io::Error::last_os_error()
    );
    available as usize
}

fn read_available(read: &mut File) -> Vec<u8> {
    let mut bytes = vec![0; pipe_bytes_available(read)];
    read.read_exact(&mut bytes).unwrap();
    bytes
}

struct PendingWrite {
    read: Option<File>,
    result: mpsc::Receiver<(RuntimeStdin, Result<(), std::io::ErrorKind>)>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl PendingWrite {
    fn start(
        read: File,
        mut stdin: RuntimeStdin,
        command: String,
        cancellation: RuntimeStdinCancellation,
    ) -> Self {
        let (sender, result) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            let result = stdin
                .write_stdin_line_with_cancellation(&command, &cancellation)
                .map_err(|error| error.kind());
            let _ = sender.send((stdin, result));
        });
        Self {
            read: Some(read),
            result,
            thread: Some(thread),
        }
    }

    fn wait_for_partial_write(&self) {
        let deadline = Instant::now() + Duration::from_secs(1);
        while pipe_bytes_available(self.read.as_ref().unwrap()) == 0 {
            assert!(Instant::now() < deadline, "writer made no progress");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn finish(&mut self) -> (RuntimeStdin, Result<(), std::io::ErrorKind>) {
        let result = self.result.recv_timeout(Duration::from_secs(3)).unwrap();
        self.thread.take().unwrap().join().unwrap();
        result
    }
}

impl Drop for PendingWrite {
    fn drop(&mut self) {
        // Even a regression to blocking WriteFile is released before joining.
        self.read.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[test]
fn ordinary_stdin_newline_is_included_in_the_write_deadline() {
    let (read, write) = test_pipe();
    let mut writer = PendingWrite::start(
        read,
        RuntimeStdin::Windows(WindowsPipeWriter::new(write)),
        "x".repeat(4096),
        RuntimeStdinCancellation::default(),
    );
    let (mut stdin, result) = writer.finish();
    assert_eq!(result, Err(std::io::ErrorKind::TimedOut));
    assert_eq!(
        stdin.write_stdin_line("later").unwrap_err().kind(),
        std::io::ErrorKind::BrokenPipe
    );
    assert_eq!(
        read_available(writer.read.as_mut().unwrap()),
        vec![b'x'; 4096]
    );
}

#[test]
fn ordinary_stdin_cancellation_after_partial_consumption_poison_closes_all_writers() {
    let (read, write) = test_pipe();
    let pipe = WindowsPipeWriter::new(write);
    let mut startup_clone = pipe.try_clone().unwrap();
    let cancellation = RuntimeStdinCancellation::default();
    let command = "状态😀".repeat(16 * 1024);
    let mut writer = PendingWrite::start(
        read,
        RuntimeStdin::Windows(pipe),
        command.clone(),
        cancellation.clone(),
    );
    writer.wait_for_partial_write();
    let mut consumed = [0; 127];
    writer
        .read
        .as_mut()
        .unwrap()
        .read_exact(&mut consumed)
        .unwrap();
    let started = Instant::now();
    cancellation.cancel();
    let (mut stdin, result) = writer.finish();
    assert_eq!(result, Err(std::io::ErrorKind::Interrupted));
    assert!(started.elapsed() < Duration::from_secs(1));
    let mut accepted = consumed.to_vec();
    accepted.extend(read_available(writer.read.as_mut().unwrap()));
    let mut available = 0;
    let peeked = unsafe {
        PeekNamedPipe(
            writer.read.as_ref().unwrap().as_raw_handle(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            &mut available,
            std::ptr::null_mut(),
        )
    };
    assert_eq!(peeked, 0, "an idle clone retained the failed writer handle");
    assert_eq!(std::io::Error::last_os_error().raw_os_error(), Some(109));
    assert_eq!(
        stdin.write_stdin_line("later").unwrap_err().kind(),
        std::io::ErrorKind::BrokenPipe
    );
    assert_eq!(
        startup_clone
            .write_line("later", &RuntimeStdinCancellation::default())
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::BrokenPipe
    );
    assert!(!accepted.is_empty() && accepted.len() < command.len());
    assert_eq!(accepted, command.as_bytes()[..accepted.len()]);
}

#[test]
fn ordinary_stdin_cancelled_before_writing_preserves_the_stream() {
    let (mut read, write) = test_pipe();
    let mut stdin = RuntimeStdin::Windows(WindowsPipeWriter::new(write));
    let cancellation = RuntimeStdinCancellation::default();
    cancellation.cancel();
    assert_eq!(
        stdin
            .write_stdin_line_with_cancellation("cancelled", &cancellation)
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::Interrupted
    );
    assert_eq!(pipe_bytes_available(&read), 0);
    stdin.write_stdin_line("状态😀").unwrap();
    stdin.write_stdin_line("Save").unwrap();
    assert_eq!(read_available(&mut read), "状态😀\nSave\n".as_bytes());
}

#[test]
fn ordinary_stdin_startup_clone_cannot_interleave_an_active_command() {
    let (read, write) = test_pipe();
    let pipe = WindowsPipeWriter::new(write);
    let mut clone = pipe.try_clone().unwrap();
    let cancellation = RuntimeStdinCancellation::default();
    let mut writer = PendingWrite::start(
        read,
        RuntimeStdin::Windows(pipe),
        "x".repeat(64 * 1024),
        cancellation.clone(),
    );
    writer.wait_for_partial_write();
    let result = clone.write_line("interleaved", &RuntimeStdinCancellation::default());
    cancellation.cancel();
    let (_, stopped) = writer.finish();
    assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::WouldBlock);
    assert_eq!(stopped, Err(std::io::ErrorKind::Interrupted));
}

#[test]
fn ordinary_stdin_peer_close_is_an_error_and_prevents_reuse() {
    let (read, write) = test_pipe();
    let mut stdin = RuntimeStdin::Windows(WindowsPipeWriter::new(write));
    drop(read);
    for _ in 0..2 {
        assert_eq!(
            stdin.write_stdin_line("Save").unwrap_err().kind(),
            std::io::ErrorKind::BrokenPipe
        );
    }
}

const CHILD_FIXTURE_NAME: &str = "stdin_write_tests::ordinary_stdin_child_fixture";

#[test]
fn ordinary_stdin_child_fixture() {
    match std::env::var("LGSM_STDIN_PIPE_CHILD").as_deref() {
        Ok("echo") => {
            let mut input = Vec::new();
            std::io::stdin().read_to_end(&mut input).unwrap();
            assert_eq!(input, "状态😀\nSave\n".as_bytes());
            println!("LGSM_STDIN_COMMANDS_VERIFIED");
        }
        Ok("unread") => std::thread::sleep(Duration::from_secs(30)),
        _ => {}
    }
}

struct OwnedFixture {
    child: RuntimeChild,
    output: std::process::ChildStdout,
}

impl OwnedFixture {
    fn spawn(mode: &str) -> Self {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", CHILD_FIXTURE_NAME, "--nocapture"])
            .env("LGSM_STDIN_PIPE_CHILD", mode)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .unwrap();
        let output = child.stdout.take().unwrap();
        Self {
            child: RuntimeChild::Standard(child),
            output,
        }
    }

    fn wait_for_exit(&mut self) -> Option<i32> {
        let RuntimeChild::Standard(child) = &mut self.child else {
            unreachable!();
        };
        assert_eq!(
            unsafe { WaitForSingleObject(child.as_raw_handle(), 3000) },
            WAIT_OBJECT_0,
            "controlled stdin child did not exit"
        );
        child.try_wait().unwrap().unwrap().code()
    }
}

impl Drop for OwnedFixture {
    fn drop(&mut self) {
        if !matches!(self.child.try_wait(), Ok(Some(_))) {
            let _ = self.child.terminate_owned();
            if let RuntimeChild::Standard(child) = &mut self.child {
                unsafe { WaitForSingleObject(child.as_raw_handle(), 3000) };
                let _ = child.try_wait();
            }
        }
    }
}

#[test]
fn ordinary_standard_stdin_preserves_unicode_and_order_across_leases() {
    let mut fixture = OwnedFixture::spawn("echo");
    let mut first = fixture.child.take_stdin().unwrap();
    first.write_stdin_line("状态😀").unwrap();
    fixture.child.restore_stdin(first);
    let mut second = fixture.child.take_stdin().unwrap();
    second.write_stdin_line("Save").unwrap();
    drop(second);
    assert_eq!(fixture.wait_for_exit(), Some(0));
    let mut output = String::new();
    fixture.output.read_to_string(&mut output).unwrap();
    assert!(output.contains("LGSM_STDIN_COMMANDS_VERIFIED"));
}

#[test]
fn ordinary_standard_stdin_unread_child_cannot_retain_a_failed_writer() {
    let mut fixture = OwnedFixture::spawn("unread");
    let mut stdin = fixture.child.take_stdin().unwrap();
    let started = Instant::now();
    let error = stdin
        .write_stdin_line(&"x".repeat(1024 * 1024))
        .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    assert!(started.elapsed() < Duration::from_secs(3));
    fixture.child.restore_stdin(stdin);
    assert!(fixture.child.take_stdin().is_none());
    assert!(fixture.child.try_wait().unwrap().is_none());
    fixture.child.terminate_owned().unwrap();
    assert!(fixture.wait_for_exit().is_some());
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn PeekNamedPipe(
        pipe: *mut std::ffi::c_void,
        buffer: *mut std::ffi::c_void,
        size: u32,
        read: *mut u32,
        available: *mut u32,
        left: *mut u32,
    ) -> i32;
}
