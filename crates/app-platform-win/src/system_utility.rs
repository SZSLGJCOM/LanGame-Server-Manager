use std::io;
use std::process::Output;
use std::time::Duration;

pub(super) const QUERY_TIMEOUT: Duration = Duration::from_secs(15);
pub(super) const FIREWALL_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Copy)]
pub(super) enum SystemUtility {
    PowerShell,
    Registry,
    Network,
}

impl SystemUtility {
    fn relative_path(self) -> &'static str {
        match self {
            Self::PowerShell => "WindowsPowerShell/v1.0/powershell.exe",
            Self::Registry => "reg.exe",
            Self::Network => "netstat.exe",
        }
    }
}

pub(super) fn capture(
    utility: SystemUtility,
    arguments: &[&str],
    timeout: Duration,
) -> io::Result<Output> {
    let executable = app_runtime::windows_system_directory()?.join(utility.relative_path());
    app_runtime::capture_windows_utility(&executable, arguments, timeout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_utilities_use_existing_absolute_system_paths() {
        let directory = app_runtime::windows_system_directory().unwrap();
        assert!(directory.is_absolute());
        for utility in [
            SystemUtility::PowerShell,
            SystemUtility::Registry,
            SystemUtility::Network,
        ] {
            assert!(directory.join(utility.relative_path()).is_file());
        }
    }
}
