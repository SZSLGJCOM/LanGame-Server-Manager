//! Real process admission complements database status and program mutation locks.
use super::{InstanceDetails, Profile, io, payload};
use std::fs::File;
use std::path::{Path, PathBuf};

struct Entry {
    pid: u32,
    image: PathBuf,
}

fn admit(entries: &[Entry], expected_image: &Path, allowed_pid: Option<u32>) -> Result<(), String> {
    let mut matching = entries.iter().filter(|entry| entry.image == expected_image);
    match (allowed_pid, matching.next(), matching.next()) {
        (None, None, None) => Ok(()),
        (Some(allowed), Some(entry), None) if allowed != 0 && entry.pid == allowed => Ok(()),
        (None, _, _) => Err("This program directory still has a live server process. No save extension files may be changed until it exits.".into()),
        _ => Err("The program directory must contain exactly the current managed server process before a save request can be published.".into()),
    }
}

/// None requires no live process for preparation/refresh. Some requires exactly
/// the current managed PID, so two servers cannot consume the same request file.
pub(super) fn ensure_scope(
    root: &Path,
    game: Profile,
    allowed_pid: Option<u32>,
) -> Result<(), String> {
    ensure_image_scope(root, game.image, allowed_pid)
}

fn config_directory(details: &InstanceDetails) -> Result<&Path, String> {
    let config = Path::new(&details.config_file_path)
        .parent()
        .ok_or("The managed instance config directory is unavailable.")?;
    if config.file_name().and_then(|name| name.to_str()) != Some("config") {
        return Err("The managed instance config directory is invalid.".into());
    }
    Ok(config)
}

/// Barotrauma's entry assembly lives beside its instance XML in config. The
/// runtime remains a source package and must not run a second competing server.
pub(super) fn ensure_instance_scope(
    details: &InstanceDetails,
    root: &Path,
    game: Profile,
    allowed_pid: Option<u32>,
) -> Result<(), String> {
    if game.module != "barotrauma" {
        return ensure_scope(root, game, allowed_pid);
    }
    ensure_scope(root, game, None)?;
    let config = config_directory(details)?;
    let _pins = io::pin_directories(config)?;
    if std::fs::symlink_metadata(config.join(game.image))
        .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
    {
        return if allowed_pid.is_none() {
            Ok(())
        } else {
            Err("The managed Barotrauma entry executable is unavailable.".into())
        };
    }
    ensure_scope(config, game, allowed_pid)
}

fn expected_process_image(
    details: &InstanceDetails,
    root: &Path,
    game: Profile,
) -> Result<PathBuf, String> {
    Ok(if game.module == "barotrauma" {
        config_directory(details)?.join(game.image)
    } else {
        root.join(game.image)
    })
}

fn pin_matching_package(root: &Path, target: &Path, game: Profile) -> Result<Vec<File>, String> {
    let mut pins = io::pin_directories(root)?;
    pins.extend(io::pin_directories(target)?);
    let mut paths = payload::native_files(game)?
        .into_iter()
        .map(|(path, _)| path)
        .collect::<Vec<_>>();
    if !paths.contains(&game.image) {
        paths.push(game.image);
    }
    for relative in paths {
        let source = root.join(relative);
        let destination = target.join(relative);
        pins.extend(io::pin_directories(
            source.parent().ok_or("Invalid native package path")?,
        )?);
        pins.extend(io::pin_directories(
            destination.parent().ok_or("Invalid native entry path")?,
        )?);
        let (source_pin, source_bytes) = io::pin_regular(&source, 512 * 1024 * 1024)?;
        let (target_pin, target_bytes) = io::pin_regular(&destination, 512 * 1024 * 1024)?;
        if io::digest(&source_bytes) != io::digest(&target_bytes) {
            return Err("The Barotrauma instance entry files differ from the reviewed runtime package. No save request was sent.".into());
        }
        pins.extend([source_pin, target_pin]);
    }
    Ok(pins)
}

pub(super) fn execution_pins(
    details: &InstanceDetails,
    root: &Path,
    game: Profile,
    actual_image: &Path,
) -> Result<Vec<File>, String> {
    let expected = expected_process_image(details, root, game)?;
    let mut pins = io::pin_directories(expected.parent().ok_or("Invalid managed image path")?)?;
    if dunce::canonicalize(actual_image).map_err(|_| "The current managed image is unavailable.")?
        != dunce::canonicalize(&expected)
            .map_err(|_| "The expected managed image is unavailable.")?
    {
        return Err("The managed process belongs to another program directory.".into());
    }
    if game.module == "barotrauma" {
        pins.extend(payload::pin_native(root, game)?);
        pins.extend(pin_matching_package(
            root,
            config_directory(details)?,
            game,
        )?);
    } else {
        let (image_pin, _) = io::pin_regular(&expected, 256 * 1024 * 1024)?;
        pins.push(image_pin);
    }
    Ok(pins)
}

pub(in crate::commands) fn ensure_image_scope(
    root: &Path,
    image: &str,
    allowed_pid: Option<u32>,
) -> Result<(), String> {
    let _pins = super::io::pin_directories(root)?;
    let expected = dunce::canonicalize(root.join(image))
        .map_err(|_| "The dedicated server executable is unavailable for process admission.")?;
    let name = Path::new(image)
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("Invalid server image name.")?;
    admit(&entries(name)?, &expected, allowed_pid)
}

#[cfg(windows)]
fn entries(image: &str) -> Result<Vec<Entry>, String> {
    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_NO_MORE_FILES, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    struct Snapshot(windows_sys::Win32::Foundation::HANDLE);
    impl Drop for Snapshot {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    let handle = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if handle == INVALID_HANDLE_VALUE {
        return Err("Cannot inspect the actual dedicated server processes; no save extension changes were made.".into());
    }
    let snapshot = Snapshot(handle);
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
    let mut available = unsafe { Process32FirstW(snapshot.0, &mut entry) };
    let mut found = Vec::new();
    while available != 0 {
        let end = entry
            .szExeFile
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(entry.szExeFile.len());
        if String::from_utf16_lossy(&entry.szExeFile[..end]).eq_ignore_ascii_case(image)
            && let Some(identity) = app_runtime::inspect_process_identity(entry.th32ProcessID)
                .map_err(|_| "Cannot verify a matching dedicated server process; process admission was rejected.")?
        {
            let image = dunce::canonicalize(&identity.image_path)
                .map_err(|_| "Cannot verify a matching dedicated server program path; process admission was rejected.")?;
            found.push(Entry { pid: entry.th32ProcessID, image });
        }
        available = unsafe { Process32NextW(snapshot.0, &mut entry) };
    }
    if std::io::Error::last_os_error().raw_os_error() != Some(ERROR_NO_MORE_FILES as i32) {
        return Err("Actual dedicated server process enumeration was incomplete; process admission was rejected.".into());
    }
    Ok(found)
}

#[cfg(not(windows))]
fn entries(_image: &str) -> Result<Vec<Entry>, String> {
    Err("Native save process admission requires Windows.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn details(module: &str, root: &Path) -> InstanceDetails {
        serde_json::from_value(serde_json::json!({
            "summary": { "id": "entry-path", "name": "Entry path", "module_id": module,
                "status": "Stopped", "bind_ip": "127.0.0.1", "port_count": 0, "autostart": false },
            "config_file_path": root.join("config/instance.json"),
            "saves_path": root.join("data/worlds"), "auto_backup_on_stop": false,
            "backup_retention_count": 0, "settings_json": "{}", "ports": [], "active_run": null
        }))
        .unwrap()
    }

    #[test]
    fn barotrauma_managed_process_uses_the_instance_config_entry_and_rejects_runtime_or_foreign_images()
     {
        let root = super::super::tests::Fixture::new();
        let game = super::super::profile("barotrauma").unwrap();
        let runtime = root.0.join("runtime");
        let instance = details(game.module, &root.0);
        let expected = expected_process_image(&instance, &runtime, game).unwrap();
        assert_eq!(expected, root.0.join("config/DedicatedServer.exe"));
        let entry = |image: PathBuf| Entry { pid: 42, image };
        assert!(admit(&[entry(expected.clone())], &expected, Some(42)).is_ok());
        assert!(admit(&[entry(runtime.join(game.image))], &expected, Some(42)).is_err());
        assert!(
            admit(
                &[entry(root.0.join("foreign/DedicatedServer.exe"))],
                &expected,
                Some(42)
            )
            .is_err()
        );
        fs::create_dir_all(&runtime).unwrap();
        fs::create_dir_all(root.0.join("config")).unwrap();
        fs::write(runtime.join(game.image), b"MZ fixture").unwrap();
        fs::write(&expected, b"MZ fixture").unwrap();
        assert!(execution_pins(&instance, &runtime, game, &runtime.join(game.image)).is_err());
    }

    #[test]
    fn barotrauma_config_entry_keeps_native_mirrors_pinned_and_rejects_changed_executable_or_dll() {
        let root = super::super::tests::Fixture::new();
        let game = super::super::profile("barotrauma").unwrap();
        let runtime = root.0.join("runtime");
        let config = root.0.join("config");
        fs::create_dir_all(&runtime).unwrap();
        fs::create_dir_all(&config).unwrap();
        let paths = [game.image, "DedicatedServer.dll", "BarotraumaCore.dll"];
        for path in paths {
            fs::write(runtime.join(path), path.as_bytes()).unwrap();
            fs::write(config.join(path), path.as_bytes()).unwrap();
        }
        assert!(pin_matching_package(&runtime, &config, game).is_ok());
        for path in paths {
            fs::write(config.join(path), b"operator changed entry file").unwrap();
            assert!(
                pin_matching_package(&runtime, &config, game).is_err(),
                "{path}"
            );
            assert_eq!(
                fs::read(config.join(path)).unwrap(),
                b"operator changed entry file"
            );
            assert_eq!(fs::read(runtime.join(path)).unwrap(), path.as_bytes());
            fs::write(config.join(path), path.as_bytes()).unwrap();
        }
        assert!(pin_matching_package(&runtime, &config, game).is_ok());
    }
    #[test]
    fn real_program_admission_rejects_unmanaged_or_duplicate_scope_without_blocking_foreign_paths()
    {
        for module in ["corekeeper", "vrising", "sonsoftheforest", "barotrauma"] {
            let game = super::super::profile(module).unwrap();
            let expected = PathBuf::from("D:/private-runtime").join(game.image);
            let foreign = PathBuf::from("D:/another-instance").join(game.image);
            let entry = |pid, image: &Path| Entry {
                pid,
                image: image.to_owned(),
            };
            assert!(admit(&[], &expected, None).is_ok());
            assert!(admit(&[], &expected, Some(10)).is_err());
            assert!(admit(&[entry(20, &foreign)], &expected, None).is_ok());
            assert!(admit(&[entry(20, &foreign)], &expected, Some(10)).is_err());
            assert!(admit(&[entry(10, &expected)], &expected, None).is_err());
            assert!(admit(&[entry(10, &expected)], &expected, Some(10)).is_ok());
            assert!(admit(&[entry(10, &expected)], &expected, Some(11)).is_err());
            assert!(
                admit(
                    &[entry(10, &expected), entry(11, &expected)],
                    &expected,
                    None
                )
                .is_err()
            );
            assert!(
                admit(
                    &[entry(10, &expected), entry(11, &expected)],
                    &expected,
                    Some(10)
                )
                .is_err()
            );
            assert!(
                admit(
                    &[entry(10, &expected), entry(20, &foreign)],
                    &expected,
                    Some(10)
                )
                .is_ok()
            );
        }
    }
}
