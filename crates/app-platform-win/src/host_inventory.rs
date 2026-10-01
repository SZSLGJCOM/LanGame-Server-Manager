//! Static hardware identity, kept separate from rate/capacity sampling.
use super::{deserialize_powershell_json_array, normalize_whitespace, powershell_json, wide_null};
use serde::Deserialize;
use std::collections::{BTreeSet, HashMap};
use std::ffi::{OsStr, c_void};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Component, Path, Prefix};
use std::time::{Duration, Instant};

const INVENTORY_TTL: Duration = Duration::from_secs(10 * 60);
const MAX_DISK_EXTENTS: usize = 128;

#[derive(Default)]
pub(super) struct DiskInventoryCache {
    volume_id: String,
    sampled_at: Option<Instant>,
    model: String,
}

impl DiskInventoryCache {
    pub(super) fn model_for_volume(&mut self, volume_id: Option<&str>) -> String {
        self.sample_with(volume_id, Instant::now(), read_volume_model)
    }

    fn sample_with(
        &mut self,
        volume_id: Option<&str>,
        now: Instant,
        read: impl FnOnce(&str) -> Option<String>,
    ) -> String {
        let Some(volume_id) = volume_id else {
            *self = Self::default();
            return String::new();
        };
        if self.volume_id != volume_id
            || self
                .sampled_at
                .is_none_or(|sampled| now.duration_since(sampled) >= INVENTORY_TTL)
        {
            self.volume_id = volume_id.to_owned();
            // Failed inventory is unknown too; cache that result to avoid a
            // fresh CIM process on every rate poll. A changed volume retries.
            self.model = read(volume_id).unwrap_or_default();
            self.sampled_at = Some(now);
        }
        self.model.clone()
    }
}

#[derive(Deserialize)]
struct DiskRecord {
    #[serde(rename = "Index")]
    index: u32,
    #[serde(rename = "Model")]
    model: Option<String>,
}

fn read_volume_model(volume_id: &str) -> Option<String> {
    let disks = volume_disk_numbers(volume_id)?;
    let output = powershell_json(
        r#"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$ErrorActionPreference = 'Stop'
$rows = @(Get-CimInstance Win32_DiskDrive | Select-Object -First 129 Index, Model)
$rows | ConvertTo-Json -Compress
"#,
    )?;
    let records = deserialize_powershell_json_array::<DiskRecord>(&output)?;
    if records.len() > MAX_DISK_EXTENTS {
        return None;
    }
    model_for_disk_numbers(&disks, records)
}

fn model_for_disk_numbers(disks: &[u32], records: Vec<DiskRecord>) -> Option<String> {
    if disks.is_empty() {
        return None;
    }
    let records = records
        .into_iter()
        .map(|record| (record.index, record.model))
        .collect::<HashMap<_, _>>();
    let mut models = BTreeSet::new();
    for disk in disks {
        let model = normalize_whitespace(records.get(disk)?.as_deref()?);
        if model.is_empty() {
            return None;
        }
        models.insert(model);
    }
    Some(models.into_iter().collect::<Vec<_>>().join(" / "))
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct DiskExtent {
    disk_number: u32,
    starting_offset: i64,
    extent_length: i64,
}

#[repr(C)]
struct VolumeDiskExtents {
    count: u32,
    extents: [DiskExtent; MAX_DISK_EXTENTS],
}

fn volume_disk_numbers(volume_id: &str) -> Option<Vec<u32>> {
    // UNC shares have no local physical disk identity. Never substitute a
    // local drive when the target volume cannot be opened or inspected.
    if !volume_id.to_ascii_lowercase().starts_with(r"\\?\volume{") {
        return None;
    }
    let path = wide_null(OsStr::new(volume_id.trim_end_matches('\\')));
    let raw = unsafe {
        CreateFileW(
            path.as_ptr(),
            0,
            3,
            std::ptr::null(),
            3,
            0,
            std::ptr::null_mut(),
        )
    };
    if raw.is_null() || raw as isize == -1 {
        return None;
    }
    // Zero desired access only queries volume metadata; the handle cannot
    // read/write disk contents and is closed on every return path.
    let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
    let mut output = VolumeDiskExtents {
        count: 0,
        extents: [DiskExtent::default(); MAX_DISK_EXTENTS],
    };
    let mut returned = 0_u32;
    let success = unsafe {
        DeviceIoControl(
            handle.as_raw_handle(),
            0x0056_0000,
            std::ptr::null(),
            0,
            (&mut output as *mut VolumeDiskExtents).cast(),
            std::mem::size_of_val(&output) as u32,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    if success == 0 {
        return None;
    }
    disk_numbers_from_extents(&output, returned as usize)
}

fn disk_numbers_from_extents(output: &VolumeDiskExtents, returned: usize) -> Option<Vec<u32>> {
    let count = usize::try_from(output.count).ok()?;
    if count == 0
        || count > MAX_DISK_EXTENTS
        || returned
            < std::mem::offset_of!(VolumeDiskExtents, extents)
                + count * std::mem::size_of::<DiskExtent>()
        || returned > std::mem::size_of::<VolumeDiskExtents>()
    {
        return None;
    }
    Some(
        output.extents[..count]
            .iter()
            .map(|extent| extent.disk_number)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
    )
}

pub(super) fn readable_path(value: &str) -> Option<String> {
    let path = Path::new(value);
    let mut components = path.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        return (!value.is_empty()).then(|| value.to_owned());
    };
    // Components normalizes away a directory mount's trailing separator.
    // Preserve the original suffix after validating the Windows namespace.
    let suffix = value.get(prefix.as_os_str().to_str()?.len()..)?;
    match prefix.kind() {
        Prefix::VerbatimDisk(drive) => Some(format!("{}:{suffix}", drive as char)),
        Prefix::VerbatimUNC(server, share) => Some(format!(
            r"\\{}\{}{}",
            server.to_string_lossy(),
            share.to_string_lossy(),
            suffix
        )),
        Prefix::Verbatim(_) | Prefix::DeviceNS(_) => None,
        _ => Some(value.to_owned()),
    }
}

pub(super) fn volume_display_label(volume_id: &str, mount: &str) -> String {
    if let Some(label) = readable_path(mount) {
        return label;
    }
    let name = wide_null(OsStr::new(volume_id));
    let mut paths = vec![0_u16; 32_768];
    let mut returned = 0;
    if unsafe {
        GetVolumePathNamesForVolumeNameW(
            name.as_ptr(),
            paths.as_mut_ptr(),
            paths.len() as u32,
            &mut returned,
        )
    } == 0
    {
        return String::new();
    }
    if returned as usize > paths.len() {
        return String::new();
    }
    preferred_mount_label(
        paths[..returned as usize]
            .split(|value| *value == 0)
            .filter(|path| !path.is_empty())
            .map(String::from_utf16_lossy),
    )
}

fn preferred_mount_label(paths: impl Iterator<Item = String>) -> String {
    let mut labels = paths
        .filter_map(|path| readable_path(&path))
        .collect::<Vec<_>>();
    labels.sort_by_key(|label| (label.len(), label.to_ascii_lowercase()));
    labels.into_iter().next().unwrap_or_default()
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateFileW(
        path: *const u16,
        access: u32,
        sharing: u32,
        security: *const c_void,
        disposition: u32,
        flags: u32,
        template: *mut c_void,
    ) -> *mut c_void;
    fn DeviceIoControl(
        device: *mut c_void,
        code: u32,
        input: *const c_void,
        input_size: u32,
        output: *mut c_void,
        output_size: u32,
        returned: *mut u32,
        overlapped: *mut c_void,
    ) -> i32;
    fn GetVolumePathNamesForVolumeNameW(
        name: *const u16,
        paths: *mut u16,
        length: u32,
        returned: *mut u32,
    ) -> i32;
}

#[cfg(test)]
#[path = "host_inventory_tests.rs"]
mod tests;
