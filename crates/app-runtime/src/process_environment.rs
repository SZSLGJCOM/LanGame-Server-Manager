use std::collections::BTreeMap;
use std::io;
use std::path::Path;

pub(super) struct SpawnCommand<'a> {
    pub(super) executable: &'a str,
    pub(super) args: &'a [String],
    pub(super) working_directory: &'a Path,
    pub(super) environment: &'a BTreeMap<String, String>,
}

#[cfg(windows)]
pub(super) fn validate_windows_command(command: &SpawnCommand<'_>) -> io::Result<()> {
    if command.executable.contains('\0')
        || command.args.iter().any(|argument| argument.contains('\0'))
        || command.working_directory.to_string_lossy().contains('\0')
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "The executable, arguments and working directory must not contain NUL characters.",
        ));
    }
    Ok(())
}

pub(super) fn validate_environment(
    environment: &BTreeMap<String, String>,
    requires_admin: bool,
) -> io::Result<()> {
    let invalid = |message| io::Error::new(io::ErrorKind::InvalidInput, message);
    if requires_admin && !environment.is_empty() {
        return Err(invalid(
            "Elevated launches cannot apply a process environment.",
        ));
    }
    if environment.len() > 64
        || environment
            .iter()
            .map(|(key, value)| key.len() + value.len())
            .sum::<usize>()
            > 32768
    {
        return Err(invalid("The process environment exceeds its size limit."));
    }
    let mut names = std::collections::HashSet::new();
    for (key, value) in environment {
        if key.is_empty()
            || !key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            || !names.insert(key.to_ascii_uppercase())
            || value.contains('\0')
            || value.contains("{{")
            || value.contains("}}")
        {
            return Err(invalid(
                "The process environment contains an invalid or unresolved entry.",
            ));
        }
    }
    Ok(())
}

#[cfg(windows)]
impl SpawnCommand<'_> {
    pub(super) fn windows_environment(&self) -> Option<Vec<u16>> {
        if self.environment.is_empty() {
            return None;
        }
        Some(windows_environment_block(
            std::env::vars_os(),
            self.environment,
        ))
    }
}

#[cfg(windows)]
fn windows_environment_block(
    inherited: impl IntoIterator<Item = (std::ffi::OsString, std::ffi::OsString)>,
    overrides: &BTreeMap<String, String>,
) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;

    let mut entries = inherited
        .into_iter()
        .filter(|(name, _)| {
            !overrides
                .keys()
                .any(|key| name.to_string_lossy().eq_ignore_ascii_case(key))
        })
        .chain(
            overrides
                .iter()
                .map(|(key, value)| (key.into(), value.into())),
        )
        .collect::<Vec<_>>();
    entries.sort_by_cached_key(|(key, _)| key.to_string_lossy().to_uppercase());
    let mut block = Vec::new();
    for (key, value) in entries {
        block.extend(key.encode_wide());
        block.push(u16::from(b'='));
        block.extend(value.encode_wide());
        block.push(0);
    }
    if block.is_empty() {
        block.push(0);
    }
    block.push(0);
    block
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn native_launch_rejects_nul_instead_of_truncating_the_command() {
        let root = crate::test_support::unique_test_root();
        std::fs::create_dir(&root).unwrap();
        let arguments = [String::from("--unreachable\0tail")];
        let cases = [
            ("missing.exe\0ignored", &[][..], root.as_path()),
            ("missing.exe", &arguments[..], root.as_path()),
            ("missing.exe", &[][..], Path::new("invalid\0directory")),
        ];
        for (executable, args, directory) in cases {
            let command = SpawnCommand {
                executable,
                args,
                working_directory: directory,
                environment: &BTreeMap::new(),
            };
            let file = std::fs::File::create(root.join("console.log")).unwrap();
            let stderr = file.try_clone().unwrap();
            let error = crate::spawn_standard_process(&command, file, stderr, true, false, None)
                .unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
            let file = std::fs::File::create(root.join("console.log")).unwrap();
            let error = crate::pseudo_console::spawn(&command, file, false, None).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn environment_rejects_injection_unresolved_names_and_elevation() {
        for entries in [
            vec![("INVALID=NAME", "value")],
            vec![("PATH", "bad\0tail")],
            vec![("PROFILE", "{{paths.missing}}")],
            vec![("Path", "one"), ("PATH", "two")],
        ] {
            let environment = entries
                .into_iter()
                .map(|(key, value)| (key.into(), value.into()))
                .collect();
            assert!(validate_environment(&environment, false).is_err());
        }
        let environment = BTreeMap::from([("USERPROFILE".into(), "D:/实例/profile".into())]);
        assert!(validate_environment(&environment, false).is_ok());
        assert!(validate_environment(&environment, true).is_err());
        assert!(validate_environment(&BTreeMap::new(), true).is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn windows_environment_preserves_inheritance_and_replaces_names() {
        let inherited = [
            ("Path", "original"),
            ("TEMP", "D:/scratch"),
            ("=D:", "D:/working"),
        ]
        .into_iter()
        .map(|(key, value)| (key.into(), value.into()));
        let overrides = BTreeMap::from([("PATH".into(), "D:/工具/bin".into())]);
        let block = windows_environment_block(inherited, &overrides);
        assert!(block.ends_with(&[0, 0]));
        let text = String::from_utf16(&block).expect("UTF-16 environment");
        let entries = text
            .split('\0')
            .filter(|entry| !entry.is_empty())
            .collect::<Vec<_>>();
        assert_eq!(
            entries,
            ["=D:=D:/working", "PATH=D:/工具/bin", "TEMP=D:/scratch"]
        );
    }

    #[cfg(windows)]
    #[test]
    fn native_and_standard_processes_receive_only_their_own_environment() {
        let root = crate::test_support::unique_test_root();
        std::fs::create_dir_all(&root).unwrap();
        let system = std::env::var_os("SystemRoot").unwrap();
        let executable = Path::new(&system).join("System32/WindowsPowerShell/v1.0/powershell.exe");
        let script = root.join("environment-probe.ps1");
        std::fs::write(&script, "$ErrorActionPreference = 'Stop'\nif ($env:LGSM_PROCESS_ENV_TEST -cne 'child-only' -or -not $env:SystemRoot) { exit 9 }\n[IO.File]::WriteAllText((Join-Path $PSScriptRoot 'environment-value.txt'), $env:LGSM_PROCESS_ENV_TEST)\nexit 0\n").unwrap();
        let parent_value = std::env::var_os("LGSM_PROCESS_ENV_TEST");
        for hidden in [false, true] {
            let log_path = root.join(format!("environment-{hidden}.log"));
            let log = std::fs::File::create(&log_path).unwrap();
            let stderr = log.try_clone().unwrap();
            let args = [
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                script.to_str().unwrap(),
            ]
            .map(str::to_owned);
            let environment =
                BTreeMap::from([("LGSM_PROCESS_ENV_TEST".into(), "child-only".into())]);
            let command = SpawnCommand {
                executable: executable.to_str().unwrap(),
                args: &args,
                working_directory: &root,
                environment: &environment,
            };
            let (mut child, desktop) = if hidden {
                crate::spawn_hidden_desktop_process(
                    &command,
                    log,
                    stderr,
                    crate::CREATE_NEW_CONSOLE,
                    None,
                )
            } else {
                crate::spawn_standard_process(&command, log, stderr, true, true, None)
            }
            .expect("spawn environment probe");
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            let exit_code = loop {
                match child.try_wait_code().unwrap() {
                    Some(code) => break Some(code),
                    None if std::time::Instant::now() >= deadline => {
                        child.terminate_owned().unwrap();
                        child.wait_code().unwrap();
                        break None;
                    }
                    None => std::thread::sleep(std::time::Duration::from_millis(20)),
                }
            };
            drop(child);
            drop(desktop);
            assert_eq!(
                exit_code,
                Some(Some(0)),
                "hidden={hidden}; log={}",
                std::fs::read_to_string(&log_path).unwrap()
            );
            let result = root.join("environment-value.txt");
            assert_eq!(
                std::fs::read_to_string(&result).unwrap_or_else(|error| panic!(
                    "hidden={hidden}; {}: {error}",
                    result.display()
                )),
                "child-only"
            );
            std::fs::remove_file(result).unwrap();
            assert_eq!(std::env::var_os("LGSM_PROCESS_ENV_TEST"), parent_value);
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
