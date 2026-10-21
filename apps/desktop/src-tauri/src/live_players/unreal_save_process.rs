//! Bind the fixed Shipping responder to the current managed bootstrap identity.
//! A private path alone cannot authorize a second or unrelated native process.
use app_core::ProcessIdentity;
use std::path::Path;
#[derive(Clone, Copy)]
pub(crate) enum GameProfile {
    Astroneer,
    Windrose,
    Moria,
    Abiotic,
    Nightingale,
    Conan,
    Scum,
    Palworld,
    Dragonwilds,
    Humanitz,
}
impl GameProfile {
    fn project(self) -> &'static str {
        match self {
            Self::Astroneer => "Astro",
            Self::Windrose => "R5",
            Self::Moria => "Moria",
            Self::Abiotic => "AbioticFactor",
            Self::Nightingale => "NWX",
            Self::Conan => "ConanSandbox",
            Self::Scum => "SCUM",
            Self::Palworld => "Pal",
            Self::Dragonwilds => "RSDragonwilds",
            Self::Humanitz => "HumanitZServer",
        }
    }
    pub(super) fn bootstrap(self) -> &'static str {
        match self {
            Self::Astroneer => "AstroServer.exe",
            Self::Windrose => "WindroseServer.exe",
            Self::Moria => "MoriaServer.exe",
            Self::Abiotic => "AbioticFactorServer.exe",
            Self::Nightingale => "NWXServer.exe",
            Self::Conan => "ConanSandboxServer.exe",
            Self::Scum => "SCUMServer.exe",
            Self::Palworld => "PalServer.exe",
            Self::Dragonwilds => "RSDragonwildsServer.exe",
            Self::Humanitz => "HumanitZServer.exe",
        }
    }
    fn shipping(self) -> &'static str {
        match self {
            Self::Astroneer => "AstroServer-Win64-Shipping.exe",
            Self::Windrose => "WindroseServer-Win64-Shipping.exe",
            Self::Moria => "MoriaServer-Win64-Shipping.exe",
            Self::Abiotic => "AbioticFactorServer-Win64-Shipping.exe",
            Self::Nightingale => "NWXServer-Win64-Shipping.exe",
            Self::Conan => "ConanSandboxServer-Win64-Shipping.exe",
            Self::Scum => "SCUMServer.exe",
            Self::Palworld => "PalServer-Win64-Shipping-Cmd.exe",
            Self::Dragonwilds => "RSDragonwildsServer-Win64-Shipping.exe",
            Self::Humanitz => "HumanitZServer-Win64-Shipping.exe",
        }
    }
}
#[derive(Clone, Debug)]
pub(crate) struct NativeProcess {
    pub pid: u32,
    pub identity: ProcessIdentity,
}
#[derive(Clone)]
struct Entry {
    pid: u32,
    parent: u32,
    identity: ProcessIdentity,
}
#[cfg(any(windows, test))]
fn accepted_image(image: &Path, shipping: &Path, bootstrap: Option<&Path>) -> bool {
    image == shipping || bootstrap.is_some_and(|path| image == path)
}
#[cfg(windows)]
fn entries(profile: GameProfile, root: &Path) -> Result<Vec<Entry>, String> {
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
        return Err("Cannot inspect native game process ownership.".into());
    }
    let snapshot = Snapshot(handle);
    let shipping = std::fs::canonicalize(
        root.join(profile.project())
            .join("Binaries/Win64")
            .join(profile.shipping()),
    )
    .map_err(|error| error.to_string())?;
    let bootstrap = match std::fs::canonicalize(root.join(profile.bootstrap())) {
        Ok(path) => Some(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.to_string()),
    };
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
        let name = String::from_utf16_lossy(&entry.szExeFile[..end]);
        if name.eq_ignore_ascii_case(profile.bootstrap())
            || name.eq_ignore_ascii_case(profile.shipping())
        {
            if let Some(identity) = app_runtime::inspect_process_identity(entry.th32ProcessID)
                .map_err(|_| "Cannot verify a native game process identity.")?
            {
                let image = std::fs::canonicalize(&identity.image_path)
                    .map_err(|_| "Cannot verify a native game process image.")?;
                if accepted_image(&image, &shipping, bootstrap.as_deref()) {
                    found.push(Entry {
                        pid: entry.th32ProcessID,
                        parent: entry.th32ParentProcessID,
                        identity,
                    });
                }
            }
        }
        available = unsafe { Process32NextW(snapshot.0, &mut entry) };
    }
    if std::io::Error::last_os_error().raw_os_error() != Some(ERROR_NO_MORE_FILES as i32) {
        return Err("native game process enumeration was incomplete.".into());
    }
    Ok(found)
}
#[cfg(not(windows))]
fn entries(_profile: GameProfile, _root: &Path) -> Result<Vec<Entry>, String> {
    Err("Native saving requires Windows.".into())
}
fn select(
    profile: GameProfile,
    entries: &[Entry],
    root_pid: u32,
    root_identity: &ProcessIdentity,
) -> Result<NativeProcess, String> {
    let mut native = entries.iter().filter(|entry| {
        Path::new(&entry.identity.image_path)
            .file_name()
            .is_some_and(|name| {
                name.to_string_lossy()
                    .eq_ignore_ascii_case(profile.shipping())
            })
    });
    let process = native
        .next()
        .ok_or("The current Shipping process is unavailable.")?;
    if native.next().is_some() {
        return Err(
            "Multiple Shipping processes occupy this runtime; native saving is unavailable.".into(),
        );
    }
    if process.pid == root_pid {
        if !app_runtime::process_identities_match(root_identity, &process.identity) {
            return Err("The managed native generation changed.".into());
        }
    } else if process.parent != root_pid
        || process.identity.creation_time < root_identity.creation_time
    {
        return Err(
            "The Shipping process does not belong to the current managed bootstrap generation."
                .into(),
        );
    }
    Ok(NativeProcess {
        pid: process.pid,
        identity: process.identity.clone(),
    })
}
pub(crate) fn capture(
    profile: GameProfile,
    root: &Path,
    pid: u32,
    identity: &ProcessIdentity,
) -> Result<NativeProcess, String> {
    super::transport::verify_process(pid, identity)
        .map_err(|_| "The managed bootstrap identity changed.")?;
    let process = select(profile, &entries(profile, root)?, pid, identity)?;
    super::transport::verify_process(process.pid, &process.identity)
        .map_err(|_| "The Shipping process identity changed.")?;
    super::transport::verify_process(pid, identity)
        .map_err(|_| "The managed bootstrap identity changed.")?;
    Ok(process)
}
pub(crate) fn verify(
    profile: GameProfile,
    root: &Path,
    pid: u32,
    identity: &ProcessIdentity,
    expected: &NativeProcess,
) -> Result<(), String> {
    let actual = capture(profile, root, pid, identity)?;
    if !same_generation(&actual, expected) {
        return Err("The Shipping responder generation changed during native saving.".into());
    }
    Ok(())
}
fn same_generation(actual: &NativeProcess, expected: &NativeProcess) -> bool {
    actual.pid == expected.pid
        && app_runtime::process_identities_match(&expected.identity, &actual.identity)
}
pub(crate) fn stopped(profile: GameProfile, root: &Path) -> Result<(), String> {
    if !entries(profile, root)?.is_empty() {
        return Err("The actual native program directory still has a game process; no save extension files were changed.".into());
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dragonwilds_accepts_only_the_owned_shipping_generation() {
        let root = ProcessIdentity {
            creation_time: 10,
            image_path: "D:/game/RSDragonwildsServer.exe".into(),
        };
        let child = Entry {
            pid: 20,
            parent: 5,
            identity: ProcessIdentity {
                creation_time: 11,
                image_path:
                    "D:/game/RSDragonwilds/Binaries/Win64/RSDragonwildsServer-Win64-Shipping.exe"
                        .into(),
            },
        };
        assert_eq!(
            select(GameProfile::Dragonwilds, &[child.clone()], 5, &root)
                .unwrap()
                .pid,
            20
        );
        assert_eq!(
            select(
                GameProfile::Dragonwilds,
                &[child.clone()],
                20,
                &child.identity
            )
            .unwrap()
            .pid,
            20
        );
        let mut foreign = child.clone();
        foreign.parent = 6;
        let mut stale = child.clone();
        stale.identity.creation_time = 9;
        for invalid in [
            vec![foreign],
            vec![stale],
            vec![child.clone(), child.clone()],
        ] {
            assert!(select(GameProfile::Dragonwilds, &invalid, 5, &root).is_err());
        }
        let shipping = Path::new(&child.identity.image_path);
        assert!(!accepted_image(
            Path::new(
                "D:/other/RSDragonwilds/Binaries/Win64/RSDragonwildsServer-Win64-Shipping.exe"
            ),
            shipping,
            Some(Path::new(&root.image_path))
        ));
        let expected = NativeProcess {
            pid: child.pid,
            identity: child.identity,
        };
        let mut reused = expected.clone();
        reused.identity.creation_time += 1;
        assert!(!same_generation(&expected, &reused));
    }
    fn identity(time: u64, shipping: bool) -> ProcessIdentity {
        ProcessIdentity {
            creation_time: time,
            image_path: if shipping {
                "D:/game/R5/Binaries/Win64/WindroseServer-Win64-Shipping.exe"
            } else {
                "D:/game/WindroseServer.exe"
            }
            .into(),
        }
    }
    #[test]
    fn all_fixed_profiles_select_only_their_own_shipping_name() {
        for profile in [
            GameProfile::Astroneer,
            GameProfile::Windrose,
            GameProfile::Moria,
            GameProfile::Abiotic,
            GameProfile::Nightingale,
            GameProfile::Conan,
            GameProfile::Scum,
            GameProfile::Palworld,
            GameProfile::Dragonwilds,
            GameProfile::Humanitz,
        ] {
            let native = Entry {
                pid: 20,
                parent: 5,
                identity: ProcessIdentity {
                    creation_time: 11,
                    image_path: format!(
                        "D:/game/{}/Binaries/Win64/{}",
                        profile.project(),
                        profile.shipping()
                    ),
                },
            };
            let root = ProcessIdentity {
                creation_time: 10,
                image_path: format!("D:/game/{}", profile.bootstrap()),
            };
            assert_eq!(select(profile, &[native], 5, &root).unwrap().pid, 20);
        }
    }
    #[test]
    fn astroneer_rejects_foreign_ambiguous_stale_or_wrong_image_children() {
        let root = ProcessIdentity {
            creation_time: 10,
            image_path: "D:/game/AstroServer.exe".into(),
        };
        let child = Entry {
            pid: 20,
            parent: 5,
            identity: ProcessIdentity {
                creation_time: 11,
                image_path: "D:/game/Astro/Binaries/Win64/AstroServer-Win64-Shipping.exe".into(),
            },
        };
        assert_eq!(
            select(GameProfile::Astroneer, &[child.clone()], 5, &root)
                .unwrap()
                .pid,
            20
        );
        let mut foreign = child.clone();
        foreign.parent = 6;
        let mut stale = child.clone();
        stale.identity.creation_time = 9;
        let mut wrong_image = child.clone();
        wrong_image.identity.image_path =
            "D:/game/Astro/Binaries/Win64/OtherServer-Win64-Shipping.exe".into();
        for invalid in [
            vec![foreign],
            vec![stale],
            vec![wrong_image],
            vec![child.clone(), child],
        ] {
            assert!(select(GameProfile::Astroneer, &invalid, 5, &root).is_err());
        }
    }
    #[test]
    fn astroneer_images_are_fixed_paths_and_the_native_generation_cannot_be_reused() {
        let bootstrap = Path::new("D:/game/AstroServer.exe");
        let shipping = Path::new("D:/game/Astro/Binaries/Win64/AstroServer-Win64-Shipping.exe");
        assert!(accepted_image(bootstrap, shipping, Some(bootstrap)));
        assert!(accepted_image(shipping, shipping, Some(bootstrap)));
        for foreign in [
            Path::new("D:/other/AstroServer.exe"),
            Path::new("D:/other/Astro/Binaries/Win64/AstroServer-Win64-Shipping.exe"),
            Path::new("D:/game/Astro/Binaries/Win64/OtherServer-Win64-Shipping.exe"),
        ] {
            assert!(!accepted_image(foreign, shipping, Some(bootstrap)));
        }
        let expected = NativeProcess {
            pid: 20,
            identity: ProcessIdentity {
                creation_time: 11,
                image_path: shipping.to_string_lossy().into(),
            },
        };
        assert!(same_generation(&expected, &expected));
        let mut reused = expected.clone();
        reused.identity.creation_time += 1;
        assert!(!same_generation(&reused, &expected));
        let mut replaced = expected.clone();
        replaced.pid += 1;
        assert!(!same_generation(&replaced, &expected));
        let mut wrong_image = expected.clone();
        wrong_image.identity.image_path =
            "D:/other/Astro/Binaries/Win64/AstroServer-Win64-Shipping.exe".into();
        assert!(!same_generation(&wrong_image, &expected));
    }
    #[test]
    fn current_shipping_is_unique_direct_child_of_the_managed_generation() {
        let root = identity(10, false);
        let entry = Entry {
            pid: 20,
            parent: 5,
            identity: identity(11, true),
        };
        assert_eq!(
            select(GameProfile::Windrose, &[entry.clone()], 5, &root)
                .unwrap()
                .pid,
            20
        );
        let mut invalid = entry.clone();
        invalid.parent = 6;
        assert!(select(GameProfile::Windrose, &[invalid], 5, &root).is_err());
        let mut stale = entry.clone();
        stale.identity.creation_time = 9;
        assert!(select(GameProfile::Windrose, &[stale], 5, &root).is_err());
        assert!(select(GameProfile::Windrose, &[entry.clone(), entry], 5, &root).is_err());
        let direct = Entry {
            pid: 5,
            parent: 1,
            identity: identity(12, true),
        };
        assert!(
            select(
                GameProfile::Windrose,
                &[direct.clone()],
                5,
                &identity(11, true)
            )
            .is_err()
        );
        assert!(select(GameProfile::Windrose, &[direct], 5, &identity(12, true)).is_ok());
    }
}
