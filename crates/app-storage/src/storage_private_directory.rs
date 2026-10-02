use std::io;
use std::path::Path;

/// Claim a new leaf directory without inheriting another user's access.
/// The caller owns and validates the parent; an existing leaf is never adopted.
pub(crate) fn create_private_directory(path: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        windows::create(path)
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new().mode(0o700).create(path)
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = path;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Private storage directories are unsupported on this platform",
        ))
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, LocalFree};
    use windows_sys::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
    };
    use windows_sys::Win32::Security::{
        GetTokenInformation, PSID, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateDirectoryW, GetVolumeInformationW, GetVolumePathNameW,
    };
    use windows_sys::Win32::System::SystemServices::FILE_PERSISTENT_ACLS;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    struct LocalAllocation(*mut c_void);

    impl Drop for LocalAllocation {
        fn drop(&mut self) {
            unsafe {
                LocalFree(self.0);
            }
        }
    }

    fn wide_path(path: &Path) -> io::Result<Vec<u16>> {
        let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
        if wide.contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Storage directory path contains a null character",
            ));
        }
        wide.push(0);
        Ok(wide)
    }

    fn require_persistent_acls(path: &Path) -> io::Result<()> {
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let parent = wide_path(&std::path::absolute(parent)?)?;
        let mut volume = vec![0u16; 32768];
        if unsafe { GetVolumePathNameW(parent.as_ptr(), volume.as_mut_ptr(), volume.len() as u32) }
            == 0
        {
            return Err(io::Error::last_os_error());
        }
        let mut flags = 0;
        if unsafe {
            GetVolumeInformationW(
                volume.as_ptr(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut flags,
                std::ptr::null_mut(),
                0,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        if flags & FILE_PERSISTENT_ACLS == 0 {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Storage volume cannot persist private directory permissions",
            ));
        }
        Ok(())
    }

    // Only called with a SID borrowed from a live Win32 token or descriptor.
    unsafe fn sid_text(sid: PSID) -> io::Result<String> {
        let mut text = std::ptr::null_mut();
        if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let allocation = LocalAllocation(text.cast());
        for length in 0..256 {
            if unsafe { *text.add(length) } == 0 {
                return String::from_utf16(unsafe { std::slice::from_raw_parts(text, length) })
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error));
            }
        }
        drop(allocation);
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Current Windows SID exceeds its supported length",
        ))
    }

    fn current_user_sid() -> io::Result<String> {
        let mut token = std::ptr::null_mut();
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let token = unsafe { OwnedHandle::from_raw_handle(token) };
        let mut needed = 0;
        let queried = unsafe {
            GetTokenInformation(
                token.as_raw_handle(),
                TokenUser,
                std::ptr::null_mut(),
                0,
                &mut needed,
            )
        };
        let error = io::Error::last_os_error();
        if queried != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Windows token size query unexpectedly succeeded without a buffer",
            ));
        }
        if error.raw_os_error() != Some(ERROR_INSUFFICIENT_BUFFER as i32) {
            return Err(error);
        }
        if needed < std::mem::size_of::<TOKEN_USER>() as u32 || needed > 65536 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Unexpected current Windows token size",
            ));
        }
        let mut buffer = vec![0usize; (needed as usize).div_ceil(std::mem::size_of::<usize>())];
        if unsafe {
            GetTokenInformation(
                token.as_raw_handle(),
                TokenUser,
                buffer.as_mut_ptr().cast(),
                needed,
                &mut needed,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
        unsafe { sid_text(user.User.Sid) }
    }

    pub(super) fn create(path: &Path) -> io::Result<()> {
        let wide = wide_path(path)?;
        match std::fs::symlink_metadata(path) {
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "Private storage directory already exists",
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => (),
            Err(error) => return Err(error),
        }
        require_persistent_acls(path)?;
        let sid = current_user_sid()?;
        let sddl: Vec<u16> = format!("D:P(A;OICI;FA;;;{sid})(A;OICI;FA;;;SY)\0")
            .encode_utf16()
            .collect();
        let mut descriptor = std::ptr::null_mut();
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut descriptor,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let descriptor = LocalAllocation(descriptor);
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: 0,
        };
        if unsafe { CreateDirectoryW(wide.as_ptr(), &attributes) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use windows_sys::Win32::Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT};
        use windows_sys::Win32::Security::{
            ACCESS_ALLOWED_ACE, CONTAINER_INHERIT_ACE, DACL_SECURITY_INFORMATION, GetAce,
            GetSecurityDescriptorControl, OBJECT_INHERIT_ACE, SE_DACL_PROTECTED,
        };
        use windows_sys::Win32::Storage::FileSystem::FILE_ALL_ACCESS;

        #[test]
        fn null_character_cannot_truncate_the_created_path() {
            let root = super::super::tests::TestRoot::new();
            let intended = root.0.join("private");
            let mut invalid = intended.as_os_str().to_os_string();
            invalid.push("\0ignored");
            assert_eq!(
                create_private_directory(Path::new(&invalid))
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::InvalidInput
            );
            assert!(!intended.exists());
        }

        #[test]
        fn new_directory_protects_and_inherits_only_user_and_system_access() {
            let root = super::super::tests::TestRoot::new();
            let path = root.0.join("private");
            create_private_directory(&path).unwrap();
            let mut acl = std::ptr::null_mut();
            let mut descriptor = std::ptr::null_mut();
            let code = unsafe {
                GetNamedSecurityInfoW(
                    wide_path(&path).unwrap().as_ptr(),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    &mut acl,
                    std::ptr::null_mut(),
                    &mut descriptor,
                )
            };
            assert_eq!(code, 0);
            let descriptor = LocalAllocation(descriptor);
            let mut control = 0;
            let mut revision = 0;
            assert_ne!(
                unsafe { GetSecurityDescriptorControl(descriptor.0, &mut control, &mut revision) },
                0
            );
            assert_ne!(control & SE_DACL_PROTECTED, 0);
            assert!(!acl.is_null());
            assert_eq!(unsafe { (*acl).AceCount }, 2);
            let mut actual = Vec::new();
            for index in 0..2 {
                let mut ace = std::ptr::null_mut();
                assert_ne!(unsafe { GetAce(acl, index, &mut ace) }, 0);
                let ace = unsafe { &*ace.cast::<ACCESS_ALLOWED_ACE>() };
                assert_eq!(ace.Header.AceType, 0);
                assert_eq!(
                    u32::from(ace.Header.AceFlags),
                    OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE
                );
                assert_eq!(ace.Mask, FILE_ALL_ACCESS);
                actual.push(
                    unsafe { sid_text(std::ptr::addr_of!(ace.SidStart).cast_mut().cast()) }
                        .unwrap(),
                );
            }
            actual.sort();
            let mut expected = vec![current_user_sid().unwrap(), String::from("S-1-5-18")];
            expected.sort();
            assert_eq!(actual, expected);
            let child = path.join("child");
            std::fs::create_dir(&child).unwrap();
            let file = child.join("settings.json");
            std::fs::write(&file, b"private content").unwrap();
            assert_eq!(std::fs::read(file).unwrap(), b"private content");
        }
    }
}

#[cfg(all(test, any(windows, unix)))]
mod tests {
    use super::*;

    pub(super) struct TestRoot(pub(super) std::path::PathBuf);

    impl TestRoot {
        pub(super) fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("lgsm-private-dir-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn refuses_existing_directory_and_preserves_its_contents() {
        let root = TestRoot::new();
        let path = root.0.join("existing");
        std::fs::create_dir(&path).unwrap();
        let file = path.join("sentinel");
        std::fs::write(&file, b"unchanged").unwrap();
        assert_eq!(
            create_private_directory(&path).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(std::fs::read(file).unwrap(), b"unchanged");
    }

    #[test]
    fn refuses_existing_file_and_missing_parent() {
        let root = TestRoot::new();
        let file = root.0.join("existing");
        std::fs::write(&file, b"unchanged").unwrap();
        assert_eq!(
            create_private_directory(&file).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(std::fs::read(file).unwrap(), b"unchanged");
        let missing = root.0.join("missing");
        assert_eq!(
            create_private_directory(&missing.join("leaf"))
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotFound
        );
        assert!(!missing.exists());
    }

    #[cfg(unix)]
    #[test]
    fn new_directory_excludes_group_and_other_access() {
        use std::os::unix::fs::PermissionsExt;
        let root = TestRoot::new();
        let path = root.0.join("private");
        create_private_directory(&path).unwrap();
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o077,
            0
        );
    }
}
