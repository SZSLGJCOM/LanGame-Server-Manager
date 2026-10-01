use super::*;
use std::time::{Duration, Instant};
use windows_sys::Win32::System::JobObjects::{
    JOB_OBJECT_CPU_RATE_CONTROL_ENABLE, JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP,
    JOB_OBJECT_LIMIT_JOB_MEMORY, JOBOBJECT_CPU_RATE_CONTROL_INFORMATION,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectCpuRateControlInformation,
    JobObjectExtendedLimitInformation, QueryInformationJobObject,
};
use windows_sys::Win32::System::Memory::{
    MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE, VirtualAlloc, VirtualFree,
};

const PROBE: &str = "windows_resource_limits_tests::allocation_probe";
const DIRECTORY: &str = "LGSM_RESOURCE_PROBE_DIRECTORY";
const ROLE: &str = "LGSM_RESOURCE_PROBE_ROLE";
const ALLOCATION: usize = 96 * 1024 * 1024;

#[test]
#[ignore = "bounded child allocation probe, exercised by shared_instance_job_enforces_aggregate_memory"]
fn allocation_probe() {
    let directory = PathBuf::from(std::env::var_os(DIRECTORY).expect("probe directory"));
    let role = std::env::var(ROLE).unwrap();
    // Commit without touching pages: exercise the Job's real commit cap without
    // consuming global physical memory or deliberately exhausting the host.
    let allocation = unsafe {
        VirtualAlloc(
            std::ptr::null(),
            ALLOCATION,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        )
    };
    let result = if allocation.is_null() {
        "denied"
    } else {
        "allocated"
    };
    fs::write(directory.join(format!("{role}-result")), result).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while !directory.join("release").exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    if !allocation.is_null() {
        assert_ne!(unsafe { VirtualFree(allocation, 0, MEM_RELEASE) }, 0);
    }
}

struct Fixture {
    root: PathBuf,
    processes: Vec<RuntimeChild>,
}
impl Fixture {
    fn new() -> Self {
        let root = test_support::unique_test_root();
        fs::create_dir(&root).unwrap();
        Self {
            root,
            processes: Vec::new(),
        }
    }
    fn launch(&mut self, role: &str, group: &RuntimeResourceGroup, terminal: bool) {
        let executable = std::env::current_exe()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let args = ["--ignored", "--exact", PROBE, "--nocapture"].map(String::from);
        let environment = BTreeMap::from([
            (DIRECTORY.into(), self.root.to_string_lossy().into_owned()),
            (ROLE.into(), role.into()),
        ]);
        let command = SpawnCommand {
            executable: &executable,
            args: &args,
            working_directory: &self.root,
            environment: &environment,
        };
        let log = File::create(self.root.join(format!("{role}.log"))).unwrap();
        let (child, _) = if terminal {
            pseudo_console::spawn(&command, log, false, Some(group))
        } else {
            spawn_standard_process(
                &command,
                log.try_clone().unwrap(),
                log,
                true,
                false,
                Some(group),
            )
        }
        .unwrap();
        self.processes.push(child);
    }
    fn result(&self, role: &str) -> String {
        let path = self.root.join(format!("{role}-result"));
        let deadline = Instant::now() + Duration::from_secs(10);
        while !path.exists() {
            assert!(
                Instant::now() < deadline,
                "allocation probe did not publish a result: {}",
                path.display()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        fs::read_to_string(path).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::write(self.root.join("release"), "");
        for child in &mut self.processes {
            let _ = child.finish_process_tree();
        }
        self.processes.clear();
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn shared_instance_job_enforces_aggregate_memory_and_preserves_other_world() {
    for terminal in [false, true] {
        let admission = RuntimeResourceAdmission::default();
        let limits = app_core::RuntimeResourceLimits {
            cpu_percent: Some(25),
            memory_limit_mib: Some(160),
            host_memory_reserve_mib: 0,
        };
        let group = admission.reserve("two-world-instance", &limits).unwrap();
        let handle = group.job_handle().unwrap();
        let mut cpu = JOBOBJECT_CPU_RATE_CONTROL_INFORMATION::default();
        assert_ne!(
            unsafe {
                QueryInformationJobObject(
                    handle,
                    JobObjectCpuRateControlInformation,
                    (&mut cpu as *mut JOBOBJECT_CPU_RATE_CONTROL_INFORMATION).cast(),
                    std::mem::size_of_val(&cpu) as u32,
                    std::ptr::null_mut(),
                )
            },
            0
        );
        assert_eq!(
            cpu.ControlFlags,
            JOB_OBJECT_CPU_RATE_CONTROL_ENABLE | JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP
        );
        assert_eq!(unsafe { cpu.Anonymous.CpuRate }, 2500);
        let mut memory = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        assert_ne!(
            unsafe {
                QueryInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    (&mut memory as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                    std::mem::size_of_val(&memory) as u32,
                    std::ptr::null_mut(),
                )
            },
            0
        );
        assert_ne!(
            memory.BasicLimitInformation.LimitFlags & JOB_OBJECT_LIMIT_JOB_MEMORY,
            0
        );
        assert_eq!(memory.JobMemoryLimit, 160 * 1024 * 1024);

        let mut fixture = Fixture::new();
        fixture.launch("surface", &group, terminal);
        assert_eq!(fixture.result("surface"), "allocated");
        fixture.launch("caves", &group, terminal);
        assert_eq!(
            fixture.result("caves"),
            "denied",
            "the two worlds must share one 160 MiB budget"
        );
        group.mark_started();
        assert_eq!(admission.snapshot().unwrap().pending_memory_bytes, 0);
        drop(group);
        assert_eq!(admission.snapshot().unwrap().active_groups, 1);
        fixture.processes[0].finish_process_tree().unwrap();
        assert!(
            fixture.processes[1].try_wait_code().unwrap().is_none(),
            "stopping one world's child Job must preserve its sibling"
        );
        drop(fixture);
        assert_eq!(
            admission.snapshot().unwrap(),
            RuntimeResourceAdmissionSnapshot::default()
        );
    }
}
