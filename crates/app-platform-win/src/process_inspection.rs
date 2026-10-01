use std::collections::{HashMap, VecDeque};
use std::ffi::c_void;

use app_core::ProcessIdentity;

use super::{
    CloseHandle, FileTime, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, ProcessSnapshotEntry,
    collect_process_snapshot_entries, file_time_to_u64,
};

#[derive(Debug, Clone)]
pub struct WindowInspectionTarget {
    pub pid: u32,
    pub process_key: String,
    pub display_name: String,
    pub process_identity: ProcessIdentity,
}

#[derive(Debug)]
pub(super) struct TargetProcessRecord {
    pub pid: u32,
    pub process_key: String,
    pub display_name: String,
    pub relation: &'static str,
    pub process_name: String,
    observation: ProcessObservation,
}

impl TargetProcessRecord {
    pub(super) fn is_running(&self) -> bool {
        self.observation.is_running()
    }
}

#[derive(Debug)]
struct ProcessObservation {
    identity: ProcessIdentity,
    // Retaining the process object also prevents its PID being reused while
    // window enumeration, asynchronous suppression or network inspection runs.
    handle: *mut c_void,
}

impl Drop for ProcessObservation {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe { CloseHandle(self.handle) };
        }
    }
}

impl ProcessObservation {
    fn open(pid: u32) -> Option<Self> {
        const SYNCHRONIZE: u32 = 0x00100000;
        let handle =
            unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE, 0, pid) };
        if handle.is_null() {
            return None;
        }
        let mut process = Self {
            identity: ProcessIdentity {
                creation_time: 0,
                image_path: String::new(),
            },
            handle,
        };
        let mut creation = FileTime::default();
        let mut exit = FileTime::default();
        let mut kernel = FileTime::default();
        let mut user = FileTime::default();
        if unsafe { GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) } == 0
        {
            return None;
        }
        let mut image = vec![0_u16; 32_768];
        let mut length = image.len() as u32;
        if unsafe { QueryFullProcessImageNameW(handle, 0, image.as_mut_ptr(), &mut length) } == 0 {
            return None;
        }
        process.identity = ProcessIdentity {
            creation_time: file_time_to_u64(creation),
            image_path: normalize_image_path(&String::from_utf16_lossy(&image[..length as usize])),
        };
        process.is_running().then_some(process)
    }

    fn is_running(&self) -> bool {
        const WAIT_TIMEOUT: u32 = 258;
        unsafe { WaitForSingleObject(self.handle, 0) == WAIT_TIMEOUT }
    }
}

pub(super) fn collect_verified_target_process_records(
    targets: &[WindowInspectionTarget],
) -> Result<HashMap<u32, TargetProcessRecord>, String> {
    let mut captured_at = FileTime::default();
    unsafe { GetSystemTimeAsFileTime(&mut captured_at) };
    let snapshot = collect_process_snapshot_entries()?;
    Ok(build_target_process_records(
        targets,
        &snapshot,
        file_time_to_u64(captured_at),
        ProcessObservation::open,
    ))
}

fn build_target_process_records(
    targets: &[WindowInspectionTarget],
    snapshot: &[ProcessSnapshotEntry],
    captured_at: u64,
    mut observe: impl FnMut(u32) -> Option<ProcessObservation>,
) -> HashMap<u32, TargetProcessRecord> {
    let by_pid = snapshot
        .iter()
        .map(|entry| (entry.process_id, entry))
        .collect::<HashMap<_, _>>();
    let mut children = HashMap::<u32, Vec<u32>>::new();
    for entry in snapshot {
        children
            .entry(entry.parent_process_id)
            .or_default()
            .push(entry.process_id);
    }
    let mut records = HashMap::<u32, TargetProcessRecord>::new();
    let mut ordered_targets = targets.iter().collect::<Vec<_>>();
    ordered_targets.sort_by(|left, right| {
        left.process_key
            .cmp(&right.process_key)
            .then(left.pid.cmp(&right.pid))
    });
    for target in ordered_targets {
        let mut queue = VecDeque::from([(target.pid, None)]);
        while let Some((pid, parent_created_at)) = queue.pop_front() {
            if records.contains_key(&pid) {
                continue;
            }
            let Some(entry) = by_pid.get(&pid) else {
                continue;
            };
            let Some(observation) = observe(pid) else {
                continue;
            };
            let identity = &observation.identity;
            // A snapshot's parent PID can refer to an earlier process lifetime.
            // Also reject a PID reopened after the snapshot began: it may now
            // represent an unrelated process absent from that snapshot.
            if identity.creation_time > captured_at
                || parent_created_at.is_some_and(|parent| identity.creation_time < parent)
                || (pid == target.pid && !identity_matches(&target.process_identity, identity))
            {
                continue;
            }
            if let Some(children) = children.get(&pid) {
                queue.extend(
                    children
                        .iter()
                        .map(|child| (*child, Some(identity.creation_time))),
                );
            }
            records.insert(
                pid,
                TargetProcessRecord {
                    pid,
                    process_key: target.process_key.clone(),
                    display_name: target.display_name.clone(),
                    relation: if pid == target.pid {
                        "tracked_process"
                    } else {
                        "descendant_process"
                    },
                    process_name: entry.process_name.clone(),
                    observation,
                },
            );
        }
    }
    records
}

fn identity_matches(expected: &ProcessIdentity, actual: &ProcessIdentity) -> bool {
    expected.creation_time == actual.creation_time
        && normalize_image_path(&expected.image_path) == actual.image_path
}

fn normalize_image_path(path: &str) -> String {
    let path = path.replace('/', "\\");
    path.strip_prefix(r"\\?\UNC\")
        .map(|path| format!(r"\\{path}"))
        .or_else(|| path.strip_prefix(r"\\?\").map(String::from))
        .unwrap_or(path)
        .to_lowercase()
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetProcessTimes(
        process: *mut c_void,
        creation: *mut FileTime,
        exit: *mut FileTime,
        kernel: *mut FileTime,
        user: *mut FileTime,
    ) -> i32;
    fn QueryFullProcessImageNameW(
        process: *mut c_void,
        flags: u32,
        image: *mut u16,
        size: *mut u32,
    ) -> i32;
    fn WaitForSingleObject(handle: *mut c_void, timeout: u32) -> u32;
    fn GetSystemTimeAsFileTime(time: *mut FileTime);
}

#[cfg(test)]
#[path = "process_inspection_tests.rs"]
mod tests;
