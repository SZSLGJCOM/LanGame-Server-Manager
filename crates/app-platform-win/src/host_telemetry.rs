use super::*;
use app_core::{DiskVolumeSnapshot, TelemetryStatus};
use std::io;
use std::os::windows::fs::MetadataExt;
use std::time::{SystemTime, UNIX_EPOCH};

#[repr(C)]
#[derive(Default)]
struct PerformanceInformation {
    cb: u32,
    commit_total: usize,
    commit_limit: usize,
    commit_peak: usize,
    physical_total: usize,
    physical_available: usize,
    system_cache: usize,
    kernel_total: usize,
    kernel_paged: usize,
    kernel_nonpaged: usize,
    page_size: usize,
    handle_count: u32,
    process_count: u32,
    thread_count: u32,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn K32GetPerformanceInfo(info: *mut PerformanceInformation, size: u32) -> i32;
    fn GetVolumePathNameW(path: *const u16, mount: *mut u16, length: u32) -> i32;
    fn GetVolumeNameForVolumeMountPointW(mount: *const u16, name: *mut u16, length: u32) -> i32;
    fn MultiByteToWideChar(
        code_page: u32,
        flags: u32,
        input: *const u8,
        input_length: i32,
        output: *mut u16,
        output_length: i32,
    ) -> i32;
}

pub(super) fn decode_network_counter_output(output: &[u8]) -> Option<String> {
    if let Some(text) = decode_utf16_with_bom(output) {
        return Some(text);
    }
    if let Ok(text) = std::str::from_utf8(output) {
        return Some(text.to_owned());
    }
    // Native console utilities use the Windows OEM code page when redirected.
    // PowerShell's parent console encoding is not the encoding of this child.
    decode_code_page(output, 1)
}

fn decode_code_page(input: &[u8], code_page: u32) -> Option<String> {
    let input_length = i32::try_from(input.len()).ok()?;
    let flags = 0x8; // MB_ERR_INVALID_CHARS: never replace invalid bytes silently.
    let length = unsafe {
        MultiByteToWideChar(
            code_page,
            flags,
            input.as_ptr(),
            input_length,
            std::ptr::null_mut(),
            0,
        )
    };
    if length <= 0 {
        return None;
    }
    let mut wide = vec![0_u16; length as usize];
    let written = unsafe {
        MultiByteToWideChar(
            code_page,
            flags,
            input.as_ptr(),
            input_length,
            wide.as_mut_ptr(),
            length,
        )
    };
    if written != length {
        return None;
    }
    String::from_utf16(&wide).ok()
}

pub(super) fn observation_time() -> Option<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| u64::try_from(elapsed.as_millis()).ok())
}

pub(super) fn read_commit_memory() -> (Option<u64>, Option<u64>) {
    let mut info = PerformanceInformation::default();
    let size = std::mem::size_of::<PerformanceInformation>() as u32;
    info.cb = size;
    if unsafe { K32GetPerformanceInfo(&mut info, size) } == 0 {
        return (None, None);
    }
    commit_bytes(info.commit_total, info.commit_limit, info.page_size)
}

fn commit_bytes(
    used_pages: usize,
    limit_pages: usize,
    page_size: usize,
) -> (Option<u64>, Option<u64>) {
    if page_size == 0 || limit_pages == 0 {
        return (None, None);
    }
    match (
        (used_pages as u64).checked_mul(page_size as u64),
        (limit_pages as u64).checked_mul(page_size as u64),
    ) {
        (Some(used), Some(limit)) => (Some(used), Some(limit)),
        _ => (None, None),
    }
}

pub(super) fn cpu_usage(
    previous: Option<CpuTimes>,
    current: Option<CpuTimes>,
) -> (f32, TelemetryStatus) {
    let Some(current) = current else {
        return (0.0, TelemetryStatus::Unavailable);
    };
    let Some(previous) = previous else {
        return (0.0, TelemetryStatus::WarmingUp);
    };
    let Some(total) = current
        .total
        .checked_sub(previous.total)
        .filter(|delta| *delta > 0)
    else {
        return (0.0, TelemetryStatus::WarmingUp);
    };
    let Some(idle) = current
        .idle
        .checked_sub(previous.idle)
        .filter(|idle| *idle <= total)
    else {
        return (0.0, TelemetryStatus::WarmingUp);
    };
    (
        ((total - idle) as f64 / total as f64 * 100.0) as f32,
        TelemetryStatus::Valid,
    )
}

pub(super) fn counter_rates(
    previous: Option<(u64, u64, Instant)>,
    current: (u64, u64),
    now: Instant,
) -> (u64, u64, TelemetryStatus) {
    let Some((previous_received, previous_sent, sampled_at)) = previous else {
        return (0, 0, TelemetryStatus::WarmingUp);
    };
    let elapsed = now.saturating_duration_since(sampled_at).as_secs_f64();
    let (Some(received), Some(sent)) = (
        current.0.checked_sub(previous_received),
        current.1.checked_sub(previous_sent),
    ) else {
        return (0, 0, TelemetryStatus::WarmingUp);
    };
    if elapsed < 0.5 {
        return (0, 0, TelemetryStatus::WarmingUp);
    }
    (
        (received as f64 / elapsed) as u64,
        (sent as f64 / elapsed) as u64,
        TelemetryStatus::Valid,
    )
}

pub(super) fn combined_network_status(
    left: TelemetryStatus,
    right: TelemetryStatus,
) -> TelemetryStatus {
    if left == TelemetryStatus::Valid || right == TelemetryStatus::Valid {
        TelemetryStatus::Valid
    } else if left == TelemetryStatus::WarmingUp || right == TelemetryStatus::WarmingUp {
        TelemetryStatus::WarmingUp
    } else {
        TelemetryStatus::Unavailable
    }
}

pub(super) fn disk_latency_ms(value: Option<f64>, status: Option<u32>) -> Option<f32> {
    // PDH_CSTATUS_VALID_DATA and PDH_CSTATUS_NEW_DATA are the successful states.
    if !matches!(status, Some(0 | 1)) {
        return None;
    }
    let milliseconds = value? * 1_000.0;
    (milliseconds.is_finite() && milliseconds >= 0.0 && milliseconds <= f32::MAX as f64)
        .then_some(milliseconds as f32)
}

/// Resolve actual mount points (including directory mounts and junction targets),
/// then query each volume once. This never walks a directory tree.
pub(super) fn read_disk_volumes(paths: &[String]) -> Vec<DiskVolumeSnapshot> {
    let mut volumes = Vec::<DiskVolumeSnapshot>::new();
    for path in paths.iter().filter(|path| !path.trim().is_empty()) {
        let identity = resolve_volume_identity(Path::new(path));
        let Some((id, mount)) = identity else {
            merge_volume(
                &mut volumes,
                DiskVolumeSnapshot {
                    id: format!("unavailable:{}", path.to_ascii_lowercase()),
                    label: host_inventory::readable_path(path).unwrap_or_default(),
                    paths: vec![path.clone()],
                    ..Default::default()
                },
            );
            continue;
        };
        if let Some(volume) = volumes.iter_mut().find(|volume| volume.id == id) {
            if !volume.paths.contains(path) {
                volume.paths.push(path.clone());
            }
            continue;
        }
        let mut volume = DiskVolumeSnapshot {
            label: host_inventory::volume_display_label(&id, &mount),
            id,
            paths: vec![path.clone()],
            ..Default::default()
        };
        let wide = wide_null(OsStr::new(&mount));
        let success = unsafe {
            GetDiskFreeSpaceExW(
                wide.as_ptr(),
                &mut volume.available_bytes,
                &mut volume.total_bytes,
                &mut volume.free_bytes,
            )
        };
        if success != 0 && volume.total_bytes > 0 {
            volume.status = TelemetryStatus::Valid;
        }
        merge_volume(&mut volumes, volume);
    }
    volumes
}

pub(super) fn resolve_volume_identity(path: &Path) -> Option<(String, String)> {
    resolve_volume_path(path).and_then(|resolved| volume_identity(&resolved))
}

fn resolve_volume_path(path: &Path) -> Option<PathBuf> {
    let absolute = std::path::absolute(path).ok()?;
    resolve_existing_volume_path(
        &absolute,
        |candidate| {
            std::fs::symlink_metadata(candidate)
                .map(|metadata| metadata.file_attributes() & 0x400 != 0)
        },
        |candidate| std::fs::canonicalize(candidate),
    )
}

/// Check ancestors from the root so a missing child cannot hide a broken
/// junction. Only ordinary absence permits using an already verified parent;
/// access errors and unresolved reparse points mean unknown volume ownership.
fn resolve_existing_volume_path(
    absolute: &Path,
    mut is_reparse_point: impl FnMut(&Path) -> io::Result<bool>,
    mut canonicalize: impl FnMut(&Path) -> io::Result<PathBuf>,
) -> Option<PathBuf> {
    let mut last_existing = None;
    for candidate in absolute.ancestors().collect::<Vec<_>>().into_iter().rev() {
        match is_reparse_point(candidate) {
            Ok(reparse) => {
                if reparse {
                    // Resolve before inspecting any children. A dangling mount
                    // must not become capacity on the containing local volume.
                    canonicalize(candidate).ok()?;
                }
                last_existing = Some(candidate);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return last_existing.and_then(|parent| canonicalize(parent).ok());
            }
            Err(_) => return None,
        }
    }
    last_existing.and_then(|candidate| canonicalize(candidate).ok())
}

fn merge_volume(volumes: &mut Vec<DiskVolumeSnapshot>, volume: DiskVolumeSnapshot) {
    if let Some(existing) = volumes.iter_mut().find(|existing| existing.id == volume.id) {
        for path in volume.paths {
            if !existing.paths.contains(&path) {
                existing.paths.push(path);
            }
        }
    } else {
        volumes.push(volume);
    }
}

fn volume_identity(path: &Path) -> Option<(String, String)> {
    let wide = wide_null(path.as_os_str());
    let mut mount = vec![0_u16; 32_768];
    if unsafe { GetVolumePathNameW(wide.as_ptr(), mount.as_mut_ptr(), mount.len() as u32) } == 0 {
        return None;
    }
    let length = mount.iter().position(|value| *value == 0)?;
    let label = String::from_utf16_lossy(&mount[..length]);
    let mut name = [0_u16; 64];
    let id = if unsafe {
        GetVolumeNameForVolumeMountPointW(mount.as_ptr(), name.as_mut_ptr(), name.len() as u32)
    } != 0
    {
        let length = name.iter().position(|value| *value == 0)?;
        String::from_utf16_lossy(&name[..length])
    } else {
        // Network shares do not have a local volume GUID; their mount path is the identity.
        label.clone()
    };
    Some((id.to_ascii_lowercase(), label))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chinese_oem_network_output_is_decoded_before_byte_row_parsing() {
        // CP936 output observed from native redirected netstat on Chinese Windows.
        let bytes =
            b"\xbd\xd3\xbf\xda\xcd\xb3\xbc\xc6\r\n\r\n\xd7\xd6\xbd\xda 2281900541 3901391899\r\n";
        let text = decode_code_page(bytes, 936).unwrap();
        assert_eq!(parse_network_totals(&text), Some((2281900541, 3901391899)));
        assert!(decode_code_page(&[0x81], 936).is_none());
    }

    #[test]
    fn cpu_failure_warmup_and_idle_are_distinct() {
        let first = CpuTimes {
            idle: 50,
            total: 100,
        };
        assert_eq!(cpu_usage(None, None).1, TelemetryStatus::Unavailable);
        assert_eq!(cpu_usage(None, Some(first)).1, TelemetryStatus::WarmingUp);
        assert_eq!(
            cpu_usage(
                Some(first),
                Some(CpuTimes {
                    idle: 150,
                    total: 200
                })
            ),
            (0.0, TelemetryStatus::Valid)
        );
        assert_eq!(
            cpu_usage(
                Some(first),
                Some(CpuTimes {
                    idle: 100,
                    total: 300
                })
            ),
            (75.0, TelemetryStatus::Valid)
        );
    }

    #[test]
    fn network_baseline_reset_and_real_zero_are_distinct() {
        let now = Instant::now();
        let before = now - Duration::from_secs(2);
        assert_eq!(
            counter_rates(None, (0, 0), now).2,
            TelemetryStatus::WarmingUp
        );
        assert_eq!(
            counter_rates(Some((20, 30, before)), (20, 30), now),
            (0, 0, TelemetryStatus::Valid)
        );
        assert_eq!(
            counter_rates(Some((20, 30, before)), (10, 30), now).2,
            TelemetryStatus::WarmingUp
        );
        assert_eq!(
            counter_rates(Some((20, 30, before)), (220, 430), now),
            (100, 200, TelemetryStatus::Valid)
        );
    }

    #[test]
    fn commit_uses_system_pages_and_checks_invalid_units() {
        assert_eq!(commit_bytes(3, 20, 4096), (Some(12_288), Some(81_920)));
        assert_eq!(commit_bytes(3, 20, 0), (None, None));
        #[cfg(target_pointer_width = "64")]
        assert_eq!(commit_bytes(usize::MAX, usize::MAX, 4096), (None, None));
    }

    #[test]
    fn volume_deduplication_keeps_all_paths_and_unavailable_volumes() {
        let mut volumes = Vec::new();
        for (id, path) in [
            ("volume-a", "D:/games"),
            ("volume-a", "D:/servers"),
            ("missing", "Z:/backups"),
        ] {
            merge_volume(
                &mut volumes,
                DiskVolumeSnapshot {
                    id: id.into(),
                    paths: vec![path.into()],
                    ..Default::default()
                },
            );
        }
        assert_eq!(volumes.len(), 2);
        assert_eq!(volumes[0].paths, ["D:/games", "D:/servers"]);
        assert_eq!(volumes[1].status, TelemetryStatus::Unavailable);
    }

    #[test]
    fn ordinary_missing_directory_uses_verified_parent_volume() {
        let parent = Path::new("C:/data");
        let result = resolve_existing_volume_path(
            Path::new("C:/data/not-created/instance"),
            |path| {
                if path.starts_with("C:/data/not-created") {
                    Err(io::ErrorKind::NotFound.into())
                } else {
                    Ok(false)
                }
            },
            |path| Ok(path.to_path_buf()),
        );
        assert_eq!(result.as_deref(), Some(parent));
    }

    #[test]
    fn dangling_junction_cannot_fall_back_to_its_containing_volume() {
        let junction = Path::new("C:/data/offline-volume");
        let mut inspected = Vec::new();
        let result = resolve_existing_volume_path(
            Path::new("C:/data/offline-volume/instance/backups"),
            |path| {
                inspected.push(path.to_path_buf());
                Ok(path == junction)
            },
            |path| {
                if path == junction {
                    Err(io::ErrorKind::NotFound.into())
                } else {
                    Ok(path.to_path_buf())
                }
            },
        );
        assert!(result.is_none());
        assert_eq!(inspected.last().map(PathBuf::as_path), Some(junction));
    }

    #[test]
    fn permission_or_io_failure_cannot_fall_back_to_parent_volume() {
        for kind in [io::ErrorKind::PermissionDenied, io::ErrorKind::Other] {
            let result = resolve_existing_volume_path(
                Path::new("C:/data/protected/instance"),
                |path| {
                    if path == Path::new("C:/data/protected") {
                        Err(kind.into())
                    } else {
                        Ok(false)
                    }
                },
                |path| Ok(path.to_path_buf()),
            );
            assert!(
                result.is_none(),
                "access failure {kind:?} became parent volume"
            );
        }
    }

    #[test]
    fn missing_child_of_accessible_junction_uses_the_target_volume() {
        let junction = Path::new("C:/data/save-volume");
        let target = PathBuf::from("E:/saves");
        let result = resolve_existing_volume_path(
            Path::new("C:/data/save-volume/new-backup"),
            |path| {
                if path == Path::new("C:/data/save-volume/new-backup") {
                    Err(io::ErrorKind::NotFound.into())
                } else {
                    Ok(path == junction)
                }
            },
            |path| {
                if path == junction {
                    Ok(target.clone())
                } else {
                    Ok(path.to_path_buf())
                }
            },
        );
        assert_eq!(result, Some(target));
    }

    #[test]
    fn windows_commit_and_existing_temp_volume_ffi_smoke() {
        let (used, limit) = read_commit_memory();
        assert!(used.is_some_and(|bytes| bytes > 0));
        assert!(limit.is_some_and(|bytes| bytes > 0));

        // Read only an existing OS temp directory; no test files or volumes are created.
        let temp = std::fs::canonicalize(std::env::temp_dir())
            .expect("the configured OS temp directory must exist for this read-only smoke test");
        let volumes = read_disk_volumes(&[temp.to_string_lossy().into_owned()]);
        assert_eq!(volumes.len(), 1);
        let volume = &volumes[0];
        assert_eq!(volume.status, TelemetryStatus::Valid);
        assert!(volume.total_bytes > 0);
        assert!(volume.available_bytes <= volume.total_bytes);
        assert!(!volume.id.is_empty());
        assert!(!volume.label.is_empty());
    }

    #[test]
    fn disk_latency_requires_a_valid_counter_status_even_for_zero() {
        assert_eq!(disk_latency_ms(Some(0.0), Some(0)), Some(0.0));
        assert_eq!(disk_latency_ms(Some(0.0025), Some(1)), Some(2.5));
        assert_eq!(disk_latency_ms(Some(0.0), Some(0x8000_07d5)), None);
        assert_eq!(disk_latency_ms(Some(0.0), None), None);
        assert_eq!(disk_latency_ms(Some(f64::NAN), Some(0)), None);
    }
}
