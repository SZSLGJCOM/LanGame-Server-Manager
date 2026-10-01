use super::*;
use std::ffi::c_void;

pub(super) const RESOURCE_PROBE: &str = "runtime_reliability_tests::resources::resource_probe";
const WARMUP_CYCLES: usize = 8;
const LAUNCHES_PER_CYCLE: usize = 4;

pub(super) fn measured_cycles() -> usize {
    let cycles = std::env::var("LANGAME_RELIABILITY_CYCLES")
        .map(|value| {
            value
                .parse::<usize>()
                .expect("LANGAME_RELIABILITY_CYCLES must be an integer")
        })
        .unwrap_or(24);
    assert!(
        (4..=256).contains(&cycles),
        "LANGAME_RELIABILITY_CYCLES must be in 4..=256"
    );
    cycles
}

pub(super) fn probe_budget() -> Duration {
    Duration::from_secs(((measured_cycles() + WARMUP_CYCLES) * 12 + 30) as u64)
}

#[derive(Clone, Copy, Debug)]
struct ResourceSample {
    handles: u32,
    threads: u32,
    private_bytes: usize,
    owned_output_sinks_remaining: usize,
}

impl ResourceSample {
    fn capture() -> Self {
        // Close the inspection snapshot before counting handles so the sample
        // describes the steady-state owner, not the observation machinery.
        let threads = current_thread_count();
        let process = unsafe { GetCurrentProcess() };
        let mut handles = 0;
        assert_ne!(unsafe { GetProcessHandleCount(process, &mut handles) }, 0);
        let mut memory: ProcessMemoryCountersEx = unsafe { std::mem::zeroed() };
        memory.cb = std::mem::size_of_val(&memory) as u32;
        let size = memory.cb;
        assert_ne!(
            unsafe { GetProcessMemoryInfo(process, &mut memory, size) },
            0
        );
        Self {
            handles,
            threads,
            private_bytes: memory.private_usage,
            owned_output_sinks_remaining: probe::active_output_sinks(),
        }
    }

    fn json(self) -> serde_json::Value {
        serde_json::json!({ "handles": self.handles, "threads": self.threads,
            "private_bytes": self.private_bytes,
            "owned_output_sinks_remaining": self.owned_output_sinks_remaining })
    }
}

#[test]
#[ignore = "isolated resource sampler; exercised by runtime_reliability parent test"]
fn resource_probe() {
    let root = PathBuf::from(std::env::var_os(probe::PROBE_ROOT).unwrap());
    let cycles = measured_cycles();
    // Preallocate observation buffers before the baseline. The sampler itself
    // must not introduce apparent linear growth during the measured interval.
    let mut warmup = Vec::with_capacity(WARMUP_CYCLES);
    let mut measured = Vec::with_capacity(cycles);
    let mut reader_thread_exits_verified = 0;
    for cycle in 0..WARMUP_CYCLES {
        reader_thread_exits_verified += run_cycle(&root, cycle);
        warmup.push(ResourceSample::capture());
    }
    let settled = &warmup[WARMUP_CYCLES - 4..];
    let max_handles = settled.iter().map(|sample| sample.handles).max().unwrap();
    for cycle in 0..cycles {
        reader_thread_exits_verified += run_cycle(&root, WARMUP_CYCLES + cycle);
        measured.push(ResourceSample::capture());
    }
    let report = serde_json::json!({
        "schema_version": 2,
        "measured_cycles": cycles,
        "warmup_cycles": WARMUP_CYCLES,
        "launches_per_cycle": LAUNCHES_PER_CYCLE,
        "warmup": warmup.iter().copied().map(ResourceSample::json).collect::<Vec<_>>(),
        "measured": measured.iter().copied().map(ResourceSample::json).collect::<Vec<_>>(),
        "reader_thread_exits_verified": reader_thread_exits_verified,
        "limits": { "handles": max_handles, "owned_output_sinks_remaining": 0 },
        "diagnostic_trends": {
            "threads": diagnostic_trend(&measured, |sample| u64::from(sample.threads)),
            "private_bytes": diagnostic_trend(&measured, |sample| sample.private_bytes as u64),
        },
    });
    // Preserve the complete evidence even when a resource assertion fails.
    fs::write(root.join("resource-samples.json"), report.to_string()).unwrap();
    assert!(
        warmup
            .iter()
            .all(|sample| sample.owned_output_sinks_remaining == 0),
        "a warmup launch retained its output sink: {warmup:?}"
    );
    for (cycle, sample) in measured.iter().enumerate() {
        assert!(
            sample.handles <= max_handles,
            "handle growth at cycle {cycle}: {sample:?}; warmup={settled:?}"
        );
        assert_eq!(
            sample.owned_output_sinks_remaining, 0,
            "an owned reader retained its output sink at cycle {cycle}: {sample:?}"
        );
    }
    // Windows may lazily create workers and retain committed allocator pages.
    // Their process-wide totals remain diagnostic evidence, not proof that our
    // readers leaked. Exact reader handles are checked at every owner release.
    fs::write(root.join("resources-verified.json"), report.to_string()).unwrap();
}

fn run_cycle(root: &Path, cycle: usize) -> usize {
    let mut reader_thread_exits_verified = 0;
    for (surface_index, surface) in SURFACES.into_iter().enumerate() {
        // A stopped server followed by a fresh launch models the ownership
        // boundary used by restart. The next launch must not inherit old I/O.
        let mut stopped = Fixture::at(
            root.join(format!("stop-{cycle}-{surface_index}")),
            surface.clone(),
            "unread",
            probe::SERVER_PROBE,
        );
        stopped.begin();
        stop_spawned_process(stopped.spawned.as_mut().unwrap()).unwrap();
        stopped.close_owner();
        reader_thread_exits_verified += 1;
        stopped.cleanup();

        let mut restarted = Fixture::at(
            root.join(format!("restart-{cycle}-{surface_index}")),
            surface,
            "crash",
            probe::SERVER_PROBE,
        );
        restarted.begin();
        assert_eq!(restarted.wait_exit(Duration::from_secs(10)), Some(37));
        restarted.close_owner();
        reader_thread_exits_verified += 1;
        assert!(restarted.output_contains("LGSM_CRASH_FINAL_BYTES"));
        restarted.cleanup();
    }
    reader_thread_exits_verified
}

fn diagnostic_trend(
    samples: &[ResourceSample],
    value: impl Fn(&ResourceSample) -> u64,
) -> serde_json::Value {
    let mean = |window: &[ResourceSample]| {
        window.iter().map(&value).sum::<u64>() as f64 / window.len() as f64
    };
    let first = mean(&samples[..4]);
    let last = mean(&samples[samples.len() - 4..]);
    serde_json::json!({
        "interpretation": "diagnostic_only",
        "min": samples.iter().map(&value).min().unwrap(),
        "max": samples.iter().map(&value).max().unwrap(),
        "first_window_mean": first,
        "last_window_mean": last,
        "delta": last - first,
    })
}

fn current_thread_count() -> u32 {
    let raw = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    assert_ne!(raw as isize, -1, "process snapshot failed");
    let snapshot = OwnedWindowsHandle::new(raw);
    let mut entry: ProcessEntry32W = unsafe { std::mem::zeroed() };
    entry.dw_size = std::mem::size_of_val(&entry) as u32;
    let mut available = unsafe { Process32FirstW(snapshot.as_raw(), &mut entry) } != 0;
    while available {
        if entry.th32_process_id == std::process::id() {
            return entry.cnt_threads;
        }
        available = unsafe { Process32NextW(snapshot.as_raw(), &mut entry) } != 0;
    }
    panic!("isolated resource sampler was absent from its process snapshot");
}

#[repr(C)]
struct ProcessMemoryCountersEx {
    cb: u32,
    page_fault_count: u32,
    peak_working_set_size: usize,
    working_set_size: usize,
    quota_peak_paged_pool_usage: usize,
    quota_paged_pool_usage: usize,
    quota_peak_non_paged_pool_usage: usize,
    quota_non_paged_pool_usage: usize,
    pagefile_usage: usize,
    peak_pagefile_usage: usize,
    private_usage: usize,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetProcessHandleCount(process: *mut c_void, count: *mut u32) -> i32;
}

#[link(name = "psapi")]
unsafe extern "system" {
    fn GetProcessMemoryInfo(
        process: *mut c_void,
        counters: *mut ProcessMemoryCountersEx,
        size: u32,
    ) -> i32;
}
