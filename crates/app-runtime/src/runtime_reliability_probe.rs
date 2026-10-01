use super::*;
use std::io::{BufRead, Write};
use std::os::windows::process::CommandExt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

pub(super) const SERVER_PROBE: &str = "runtime_reliability_tests::probe::server_probe";
pub(super) const PROBE_ROOT: &str = "LGSM_RELIABILITY_ROOT";
const PROBE_MODE: &str = "LGSM_RELIABILITY_MODE";
pub(super) const FLOOD_BYTES: usize = 2 * 1024 * 1024;
static ACTIVE_OUTPUT_SINKS: AtomicUsize = AtomicUsize::new(0);

pub(super) fn active_output_sinks() -> usize {
    ACTIVE_OUTPUT_SINKS.load(Ordering::Acquire)
}

#[test]
#[ignore = "internal simulated server; exercised by runtime_reliability parent tests"]
fn server_probe() {
    let root = PathBuf::from(std::env::var_os(PROBE_ROOT).unwrap());
    let mode = std::env::var(PROBE_MODE).unwrap();
    fs::write(root.join("ready"), std::process::id().to_string()).unwrap();
    assert!(wait_until(
        || root.join("begin").exists(),
        Duration::from_secs(15)
    ));
    match mode.as_str() {
        "flood" => {
            let chunk = [b'~'; 8192];
            let mut stdout = std::io::stdout().lock();
            for _ in 0..FLOOD_BYTES / chunk.len() {
                stdout.write_all(&chunk).unwrap();
            }
            stdout.write_all(b"\nLGSM_FLOOD_COMPLETE\n").unwrap();
            stdout.flush().unwrap();
            fs::write(root.join("written"), b"done").unwrap();
            assert!(wait_until(
                || root.join("release").exists(),
                Duration::from_secs(20)
            ));
            std::io::stderr()
                .write_all("LGSM_STDERR_服务器\n".as_bytes())
                .unwrap();
            stdout.write_all(b"LGSM_FINAL_WITHOUT_NEWLINE").unwrap();
            stdout.flush().unwrap();
            // Bypass libtest's trailing status line to exercise a true missing newline.
            std::process::exit(23);
        }
        "unread" => {
            assert!(wait_until(
                || root.join("release").exists(),
                Duration::from_secs(20)
            ));
        }
        "echo" => {
            let mut commands = std::io::stdin().lock().lines();
            assert_eq!(commands.next().unwrap().unwrap(), "状态😀");
            assert_eq!(commands.next().unwrap().unwrap(), "Save");
            println!("LGSM_UNICODE_ORDER_VERIFIED");
        }
        "crash" => {
            std::io::stderr()
                .write_all(b"LGSM_CRASH_FINAL_BYTES")
                .unwrap();
            std::process::exit(37);
        }
        "launcher" => {
            let child = Command::new(std::env::current_exe().unwrap())
                .args(["--ignored", "--exact", SERVER_PROBE, "--nocapture"])
                .env(PROBE_MODE, "leaf")
                .creation_flags(CREATE_NO_WINDOW)
                .spawn()
                .unwrap();
            fs::write(root.join("descendant"), child.id().to_string()).unwrap();
            assert!(wait_until(
                || root.join("handoff").exists(),
                Duration::from_secs(20)
            ));
            // The parent fixture has retained the descendant's kernel handle
            // before publishing handoff. Exiting this launcher closes its local
            // handles; the outer launch Job still owns and reaps the live leaf.
            std::process::exit(0);
        }
        "leaf" => {
            fs::write(root.join("leaf-ready"), b"running").unwrap();
            assert!(wait_until(
                || root.join("release").exists(),
                Duration::from_secs(25)
            ));
        }
        mode => panic!("unknown simulated server mode: {mode}"),
    }
}

#[derive(Default, Debug)]
pub(super) struct OutputSummary {
    pub payload: usize,
    pub tail: Vec<u8>,
    reader_thread: Option<ReaderThreadHandle>,
    reader_observation_error: Option<i32>,
}

struct SummarySink(Arc<Mutex<OutputSummary>>);

impl SummarySink {
    fn new(summary: Arc<Mutex<OutputSummary>>) -> Self {
        ACTIVE_OUTPUT_SINKS.fetch_add(1, Ordering::AcqRel);
        Self(summary)
    }
}

#[derive(Debug)]
struct ReaderThreadHandle(usize);

impl Drop for ReaderThreadHandle {
    fn drop(&mut self) {
        close_handle(self.0 as *mut std::ffi::c_void);
    }
}

impl Drop for SummarySink {
    fn drop(&mut self) {
        // This sink is moved into the production reader. Retain that exact
        // thread's kernel handle so cleanup can prove termination independently
        // of the process's unrelated Windows worker-thread count.
        let process = unsafe { GetCurrentProcess() };
        let mut thread = std::ptr::null_mut();
        let observed = unsafe {
            DuplicateHandle(
                process,
                windows_sys::Win32::System::Threading::GetCurrentThread(),
                process,
                &mut thread,
                SYNCHRONIZE,
                0,
                0,
            )
        };
        let mut summary = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if observed != 0 {
            summary.reader_thread = Some(ReaderThreadHandle(thread as usize));
        } else {
            summary.reader_observation_error = std::io::Error::last_os_error().raw_os_error();
        }
        ACTIVE_OUTPUT_SINKS.fetch_sub(1, Ordering::AcqRel);
    }
}

impl Write for SummarySink {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let mut summary = self.0.lock().unwrap();
        summary.payload += bytes.iter().filter(|&&byte| byte == b'~').count();
        // Test observation is bounded too: no full transcript accumulates here.
        if bytes.len() >= 1024 {
            summary.tail.clear();
            summary.tail.extend_from_slice(&bytes[bytes.len() - 1024..]);
        } else {
            let excess = (summary.tail.len() + bytes.len()).saturating_sub(1024);
            summary.tail.drain(..excess);
            summary.tail.extend_from_slice(bytes);
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(super) struct Fixture {
    pub root: PathBuf,
    pub spawned: Option<SpawnedProcess>,
    pub handles: Vec<WindowsProcessHandle>,
    pub output: Arc<Mutex<OutputSummary>>,
}

impl Fixture {
    pub fn new(surface: ProcessHostSurface, mode: &str) -> Self {
        Self::at(
            test_support::unique_test_root(),
            surface,
            mode,
            SERVER_PROBE,
        )
    }

    pub fn at(root: PathBuf, surface: ProcessHostSurface, mode: &str, test: &str) -> Self {
        fs::create_dir(&root).unwrap();
        let output = Arc::new(Mutex::new(OutputSummary::default()));
        let mut fixture = Self {
            root,
            spawned: None,
            handles: Vec::new(),
            output,
        };
        let plan = LaunchPlan {
            environment: BTreeMap::from([
                (
                    PROBE_ROOT.into(),
                    fixture.root.to_string_lossy().into_owned(),
                ),
                (PROBE_MODE.into(), mode.into()),
            ]),
            instance_id: "runtime-reliability".into(),
            instance_name: "Runtime reliability fixture".into(),
            module_id: "demo".into(),
            install_root: fixture.root.to_string_lossy().into_owned(),
            install_state: app_core::InstallState::Installed,
            uses_private_runtime: false,
            working_directory: fixture.root.to_string_lossy().into_owned(),
            executable_path: std::env::current_exe()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            executable_exists: true,
            ready_to_launch: true,
            validation_issues: Vec::new(),
            args: [
                "--ignored",
                "--exact",
                test,
                "--nocapture",
                "--test-threads=1",
            ]
            .map(str::to_owned)
            .to_vec(),
            command_line: String::new(),
            window_policy: ProcessWindowPolicy::Background,
            uses_script_entrypoint: mode == "launcher",
            requires_admin: false,
            host_surface: surface,
            host_notes: None,
            performance_policy: Default::default(),
            performance_preview: Default::default(),
        };
        fixture.spawned = Some(
            spawn_launch_plan_with_log_writer(
                &plan,
                fixture.root.join("console.log"),
                Box::new(SummarySink::new(Arc::clone(&fixture.output))),
            )
            .unwrap(),
        );
        let pid = fixture.spawned.as_ref().unwrap().pid;
        fixture.handles.push(
            WindowsProcessHandle::open(pid, PROCESS_TERMINATE)
                .unwrap()
                .unwrap(),
        );
        fixture
    }

    pub fn begin(&self) {
        self.wait_file("ready");
        self.signal("begin");
    }

    pub fn signal(&self, name: &str) {
        fs::write(self.root.join(name), b"continue").unwrap();
    }

    pub fn wait_file(&self, name: &str) {
        assert!(
            wait_until(|| self.root.join(name).exists(), Duration::from_secs(10)),
            "simulated server did not produce {name}; output={:?}",
            self.output.lock().unwrap()
        );
    }

    pub fn wait_exit(&mut self, budget: Duration) -> Option<i32> {
        assert!(
            self.handles[0]
                .wait_for_exit(budget.as_millis() as u32)
                .unwrap(),
            "simulated server exceeded {budget:?}; output={:?}",
            self.output.lock().unwrap()
        );
        self.spawned
            .as_mut()
            .unwrap()
            .child
            .as_mut()
            .unwrap()
            .try_wait()
            .unwrap()
            .unwrap()
            .code()
    }

    pub fn output_contains(&self, marker: &str) -> bool {
        String::from_utf8_lossy(&self.output.lock().unwrap().tail).contains(marker)
    }

    pub fn close_owner(&mut self) {
        self.spawned.take();
        {
            let output = self.output.lock().unwrap();
            let reader = output.reader_thread.as_ref().unwrap_or_else(|| {
                panic!(
                    "reader did not release its sink; observation error={:?}; output={output:?}",
                    output.reader_observation_error
                )
            });
            assert_eq!(
                unsafe { WaitForSingleObject(reader.0 as *mut std::ffi::c_void, 0) },
                WAIT_OBJECT_0,
                "the exact managed output reader survived owner release",
            );
        }
        for handle in &self.handles {
            assert!(
                handle.wait_for_exit(3000).unwrap(),
                "owned process survived owner release"
            );
        }
    }

    pub fn cleanup(self) {
        let root = self.root.clone();
        drop(self);
        assert!(
            !root.try_exists().expect("inspect fixture cleanup"),
            "fixture scratch directory survived cleanup: {}",
            root.display()
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Owner is dropped before directories/observation handles. The launch Job
        // contains all descendants, including probes killed on a parent timeout.
        self.spawned.take();
        for handle in &self.handles {
            if handle.is_running().unwrap_or(true) {
                let _ = handle.terminate();
            }
            let _ = handle.wait_for_exit(3000);
        }
        self.handles.clear();
        let _ = fs::remove_dir_all(&self.root);
    }
}

pub(super) fn wait_until(mut ready: impl FnMut() -> bool, budget: Duration) -> bool {
    let deadline = Instant::now() + budget;
    loop {
        if ready() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
