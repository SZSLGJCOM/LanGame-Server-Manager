use super::*;

pub(super) fn inspect_windows_process_identity(
    pid: u32,
) -> Result<Option<ProcessIdentity>, RuntimeProcessError> {
    let Some(handle) = WindowsProcessHandle::open(pid, 0)? else {
        return Ok(None);
    };
    if !handle.is_running()? {
        return Ok(None);
    }
    resolve_identity_after_exit_race(handle.identity(), || handle.wait_for_exit(0))
}

pub(super) fn resolve_identity_after_exit_race(
    identity: Result<ProcessIdentity, RuntimeProcessError>,
    has_exited: impl FnOnce() -> Result<bool, RuntimeProcessError>,
) -> Result<Option<ProcessIdentity>, RuntimeProcessError> {
    match identity {
        Ok(identity) => Ok(Some(identity)),
        Err(error) => {
            // Image metadata can become unavailable during exit. Only a signaled
            // handle for that same process proves it is safe to report no identity.
            if has_exited()? { Ok(None) } else { Err(error) }
        }
    }
}

pub(super) fn query_windows_process_identity_from_handle(
    pid: u32,
    handle: *mut std::ffi::c_void,
) -> Result<ProcessIdentity, RuntimeProcessError> {
    let mut creation = FileTime::default();
    let mut exit = FileTime::default();
    let mut kernel = FileTime::default();
    let mut user = FileTime::default();
    if unsafe { GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) } == 0 {
        let source = std::io::Error::last_os_error();
        return Err(RuntimeProcessError::InspectProcess { pid, source });
    }

    let mut capacity = MAX_PATH_WIDE;
    let image_path = loop {
        let mut buffer = vec![0_u16; capacity];
        let mut length = buffer.len() as u32;
        if unsafe { QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut length) } != 0 {
            buffer.truncate(length as usize);
            break String::from_utf16_lossy(&buffer);
        }
        let source = std::io::Error::last_os_error();
        if source.raw_os_error() == Some(ERROR_INSUFFICIENT_BUFFER)
            && capacity < MAX_PROCESS_IMAGE_PATH_WIDE
        {
            capacity = (capacity * 2).min(MAX_PROCESS_IMAGE_PATH_WIDE);
            continue;
        }
        return Err(RuntimeProcessError::InspectProcess { pid, source });
    };

    Ok(ProcessIdentity {
        creation_time: (u64::from(creation.high) << 32) | u64::from(creation.low),
        image_path: normalized_process_image_path(&image_path),
    })
}
