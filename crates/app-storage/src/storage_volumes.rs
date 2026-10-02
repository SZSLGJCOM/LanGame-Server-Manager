use std::io;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StorageVolume {
    pub root: PathBuf,
    pub available_bytes: u64,
    pub is_system: bool,
}

#[cfg(windows)]
pub(crate) fn available_volumes() -> io::Result<Vec<StorageVolume>> {
    use std::ptr::null_mut;
    use windows_sys::Win32::Storage::FileSystem::{
        GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
    };

    // These Win32 constants live in separate windows-sys feature modules.
    const DRIVE_FIXED: u32 = 3;
    const FILE_READ_ONLY_VOLUME: u32 = 0x0008_0000;

    let system_root = std::env::var("SystemRoot").ok();
    let system_drive = std::env::var("SystemDrive").ok();
    let system_letter = system_drive_letter(system_root.as_deref(), system_drive.as_deref())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "SystemRoot and SystemDrive do not identify the Windows system drive",
            )
        })?;
    // GetLogicalDrives has no pointer arguments and reports failure as zero.
    let drives = unsafe { GetLogicalDrives() };
    if drives == 0 {
        return Err(io::Error::last_os_error());
    }

    let mut volumes = Vec::new();
    for index in 0..26 {
        if drives & (1_u32 << index) == 0 {
            continue;
        }
        let letter = b'A' + index;
        let root = [u16::from(letter), u16::from(b':'), u16::from(b'\\'), 0];
        // The terminated drive-root buffer remains valid for each Win32 call.
        if unsafe { GetDriveTypeW(root.as_ptr()) } != DRIVE_FIXED {
            continue;
        }
        let mut flags = 0;
        // Optional outputs are null; flags points to writable storage for this call.
        let readable = unsafe {
            GetVolumeInformationW(
                root.as_ptr(),
                null_mut(),
                0,
                null_mut(),
                null_mut(),
                &mut flags,
                null_mut(),
                0,
            )
        };
        if readable == 0 || flags & FILE_READ_ONLY_VOLUME != 0 {
            continue;
        }
        let mut available_bytes = 0;
        // Use caller-available space, which respects quotas, rather than total free space.
        let queried = unsafe {
            GetDiskFreeSpaceExW(root.as_ptr(), &mut available_bytes, null_mut(), null_mut())
        };
        if queried == 0 {
            continue;
        }
        volumes.push(StorageVolume {
            root: PathBuf::from(format!("{}:\\", char::from(letter))),
            available_bytes,
            is_system: letter == system_letter,
        });
    }
    Ok(rank_volumes(volumes))
}

#[cfg(not(windows))]
pub(crate) fn available_volumes() -> io::Result<Vec<StorageVolume>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "automatic fixed-drive selection is only supported on Windows",
    ))
}

#[cfg(any(windows, test))]
fn rank_volumes(mut volumes: Vec<StorageVolume>) -> Vec<StorageVolume> {
    volumes.retain(|volume| volume.available_bytes > 0);
    volumes.sort_by(|left, right| {
        left.is_system
            .cmp(&right.is_system)
            .then_with(|| right.available_bytes.cmp(&left.available_bytes))
            .then_with(|| left.root.cmp(&right.root))
    });
    volumes
}

#[cfg(any(windows, test))]
fn system_drive_letter(system_root: Option<&str>, system_drive: Option<&str>) -> Option<u8> {
    fn drive_letter(path: &str, allow_bare_drive: bool) -> Option<u8> {
        let bytes = path.as_bytes();
        let letter = *bytes.first()?;
        if !letter.is_ascii_alphabetic() || bytes.get(1) != Some(&b':') {
            return None;
        }
        match bytes.get(2) {
            Some(b'\\' | b'/') => Some(letter.to_ascii_uppercase()),
            None if allow_bare_drive => Some(letter.to_ascii_uppercase()),
            _ => None,
        }
    }

    system_root
        .and_then(|path| drive_letter(path, false))
        .or_else(|| system_drive.and_then(|path| drive_letter(path, true)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn volume(letter: char, available_bytes: u64, is_system: bool) -> StorageVolume {
        StorageVolume {
            root: PathBuf::from(format!("{letter}:\\")),
            available_bytes,
            is_system,
        }
    }

    #[test]
    fn non_system_drives_precede_system_and_use_largest_available_space() {
        let expected = vec![
            volume('E', 300, false),
            volume('D', 100, false),
            volume('C', 900, true),
        ];
        let actual = rank_volumes(vec![
            expected[2].clone(),
            expected[1].clone(),
            expected[0].clone(),
        ]);
        assert_eq!(actual, expected);
    }

    #[test]
    fn a_system_drive_is_usable_when_it_is_the_only_candidate() {
        let only = volume('F', 50, true);
        assert_eq!(rank_volumes(vec![only.clone()]), vec![only]);
    }

    #[test]
    fn zero_available_space_is_excluded_including_on_the_system_drive() {
        assert_eq!(
            rank_volumes(vec![
                volume('C', 0, true),
                volume('D', 0, false),
                volume('E', 1, false),
            ]),
            vec![volume('E', 1, false)]
        );
        assert!(rank_volumes(vec![volume('F', 0, true)]).is_empty());
    }

    #[test]
    fn equal_capacity_has_stable_drive_order_regardless_of_enumeration_order() {
        let expected = vec![volume('D', 100, false), volume('E', 100, false)];
        assert_eq!(rank_volumes(expected.clone()), expected);
        assert_eq!(
            rank_volumes(vec![expected[1].clone(), expected[0].clone()]),
            expected
        );
    }

    #[test]
    fn ranking_uses_each_capacity_snapshot_without_remembering_a_previous_winner() {
        let before = rank_volumes(vec![volume('D', 200, false), volume('E', 100, false)]);
        let after = rank_volumes(vec![volume('D', 50, false), volume('E', 100, false)]);
        assert_eq!(before[0].root, PathBuf::from("D:\\"));
        assert_eq!(after[0].root, PathBuf::from("E:\\"));
    }

    #[test]
    fn system_drive_comes_from_windows_location_instead_of_assuming_c() {
        let system_letter = system_drive_letter(Some("f:\\Windows"), Some("C:"));
        assert_eq!(system_letter, Some(b'F'));
        let actual = rank_volumes(
            [('F', 900), ('C', 100)]
                .into_iter()
                .map(|(letter, bytes)| volume(letter, bytes, Some(letter as u8) == system_letter))
                .collect(),
        );
        assert_eq!(
            actual,
            vec![volume('C', 100, false), volume('F', 900, true)]
        );
    }

    #[test]
    fn system_drive_fallback_rejects_relative_and_network_locations() {
        assert_eq!(system_drive_letter(None, Some("e:")), Some(b'E'));
        assert_eq!(
            system_drive_letter(Some("relative\\Windows"), Some("G:\\")),
            Some(b'G')
        );
        assert_eq!(
            system_drive_letter(Some("\\\\server\\Windows"), Some("C:relative")),
            None
        );
        assert_eq!(system_drive_letter(Some("C:"), None), None);
        assert_eq!(system_drive_letter(None, None), None);
    }

    #[cfg(not(windows))]
    #[test]
    fn non_windows_enumeration_reports_unsupported() {
        assert_eq!(
            available_volumes().unwrap_err().kind(),
            io::ErrorKind::Unsupported
        );
    }
}
