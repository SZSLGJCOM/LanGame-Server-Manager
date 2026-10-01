use super::*;
use std::ffi::OsString;

/// Redirect environment-based user data only. Native KnownFolder APIs may use
/// the Windows profile independently; game-specific save paths remain required.
pub(super) struct NativeEnvironment {
    _app_data: ProgramDataEnvGuard,
    previous: Vec<(&'static str, Option<OsString>)>,
}

impl NativeEnvironment {
    pub(super) fn set(root: &Path) -> Result<Self, std::io::Error> {
        let profile = root.join("profile");
        let assignments = [
            ("APPDATA", profile.join("AppData/Roaming")),
            ("USERPROFILE", profile.clone()),
            ("HOME", profile),
        ];
        for (_, path) in &assignments {
            fs::create_dir_all(path)?;
        }
        let previous = assignments
            .iter()
            .map(|(key, _)| (*key, std::env::var_os(key)))
            .collect();
        // The native fixture holds command_smoke_lock and runs alone with
        // --test-threads=1. Restore before its disposable package is removed.
        for (key, path) in assignments {
            unsafe { std::env::set_var(key, path) };
        }
        Ok(Self {
            _app_data: ProgramDataEnvGuard::set(&root.join("programdata")),
            previous,
        })
    }
}

impl Drop for NativeEnvironment {
    fn drop(&mut self) {
        for (key, previous) in self.previous.drain(..) {
            match previous {
                Some(value) => unsafe { std::env::set_var(key, value) },
                None => unsafe { std::env::remove_var(key) },
            }
        }
    }
}
