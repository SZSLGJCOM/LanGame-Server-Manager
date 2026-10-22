//! Prepare the optional roster loader while startup owns the program lifecycle.
use std::path::Path;

use app_core::InstanceDetails;
use app_storage::InstanceProgramMode;

use crate::state::RuntimeStartReservationLease;

use super::commands_runtime_supervision::append_startup_console_line;

#[path = "commands_dragonwilds_player_query_files.rs"]
mod files;

#[cfg(windows)]
pub(super) fn ensure_program_stopped(root: &Path) -> Result<(), String> {
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
        return Err(format!(
            "Cannot check Dragonwilds process occupancy: {}",
            std::io::Error::last_os_error()
        ));
    }
    let snapshot = Snapshot(handle);
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
    let mut available = unsafe { Process32FirstW(snapshot.0, &mut entry) };
    while available != 0 {
        let end = entry
            .szExeFile
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(entry.szExeFile.len());
        let name = String::from_utf16_lossy(&entry.szExeFile[..end]);
        if (name.eq_ignore_ascii_case("RSDragonwildsServer.exe")
            || name.eq_ignore_ascii_case("RSDragonwildsServer-Win64-Shipping.exe"))
            && let Some(identity) = app_runtime::inspect_process_identity(entry.th32ProcessID)
                .map_err(|error| format!("Cannot verify Dragonwilds process occupancy: {error}"))?
        {
            let image =
                dunce::canonicalize(&identity.image_path).map_err(|error| error.to_string())?;
            let root = dunce::canonicalize(root).map_err(|error| error.to_string())?;
            let prefix =
                format!("{}\\", root.to_string_lossy().trim_end_matches('\\')).to_ascii_lowercase();
            if image
                .to_string_lossy()
                .to_ascii_lowercase()
                .starts_with(&prefix)
            {
                return Err("Dragonwilds 的实际程序目录仍有游戏进程运行，未安装或修改玩家查询加载器。请先正常停止该服务器。".into());
            }
        }
        available = unsafe { Process32NextW(snapshot.0, &mut entry) };
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() != Some(ERROR_NO_MORE_FILES as i32) {
        return Err(format!(
            "Cannot finish Dragonwilds process inspection: {error}"
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
pub(super) fn ensure_program_stopped(_root: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(windows)]
fn bundled_files() -> (&'static [u8], &'static [u8], &'static [u8]) {
    (
        include_bytes!(concat!(
            env!("OUT_DIR"),
            "/dragonwilds-player-query/version.dll"
        )),
        include_bytes!(concat!(
            env!("OUT_DIR"),
            "/dragonwilds-player-query/LICENSE-UE4SS.txt"
        )),
        include_bytes!(concat!(
            env!("OUT_DIR"),
            "/dragonwilds-player-query/LICENSE-fmt.txt"
        )),
    )
}

#[cfg(not(windows))]
fn bundled_files() -> (&'static [u8], &'static [u8], &'static [u8]) {
    (&[], &[], &[])
}

pub(super) fn owns_compatible_loader(root: &Path) -> Result<bool, String> {
    Ok(files::inspect(root, bundled_files().0)? == files::Preparation::Ready)
}

fn progress(log_path: Option<&Path>, message: &str) -> Result<(), String> {
    if let Some(path) = log_path {
        append_startup_console_line(path, message, "")?;
    }
    Ok(())
}

/// Caller retains both instance and program guards until this future completes.
/// Publication finishes even when shutdown cancels the following native start.
pub(super) async fn prepare(
    instance: &InstanceDetails,
    reservation: &RuntimeStartReservationLease,
    log_path: Option<&Path>,
) -> Result<(), String> {
    if instance.summary.module_id != "runescapedragonwilds" {
        return Ok(());
    }
    let instance_root = super::commands_program_storage::instance_root(instance)?;
    if app_storage::instance_program_mode(instance_root).map_err(|error| error.to_string())?
        != InstanceProgramMode::Independent
    {
        return progress(
            log_path,
            "Dragonwilds 玩家查询：共享程序目录保留原状；自动准备需要独立运行副本。",
        );
    }
    let root = app_storage::resolve_instance_runtime_root(instance_root)
        .map_err(|error| error.to_string())?;
    let (proxy, ue4ss_license, fmt_license) = bundled_files();
    let inspecting_root = root.clone();
    let preparation = tokio::task::spawn_blocking(move || files::inspect(&inspecting_root, proxy))
        .await
        .map_err(|error| format!("Dragonwilds player-query inspection worker failed: {error}"))??;
    match preparation {
        files::Preparation::Ready => return Ok(()),
        files::Preparation::ExternalLoader => {
            return progress(
                log_path,
                "Dragonwilds 玩家查询：检测到已有加载器，保留其文件与配置；兼容配置会继续准备名单脚本。",
            );
        }
        files::Preparation::UnsupportedBuild => {
            return progress(
                log_path,
                "Dragonwilds 玩家查询：当前游戏版本尚未验证加载器兼容性，已跳过自动安装；游戏服务器继续启动。",
            );
        }
        files::Preparation::NeedsInstall => {}
    }
    progress(
        log_path,
        "Dragonwilds 玩家查询：正在校验并准备程序内置的加载器和名单脚本…",
    )?;
    let cancellation = reservation.storage_operation().cancellation_token();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        if cancellation.load(std::sync::atomic::Ordering::Acquire) {
            return Err("启动已取消，玩家查询扩展未发布。".into());
        }
        let archive = crate::unreal_loader::bundled_archive()?;
        files::install(&root, archive, proxy, ue4ss_license, fmt_license)
    })
    .await
    .map_err(|error| format!("Dragonwilds player-query installation worker failed: {error}"))??;
    progress(
        log_path,
        "Dragonwilds 玩家查询扩展已准备，正在启动游戏服务器。",
    )
}

#[cfg(all(test, windows))]
#[test]
fn bundled_dragonwilds_loader_is_pinned() {
    files::verify_archive(crate::unreal_loader::bundled_archive().unwrap()).unwrap();
}

#[cfg(test)]
pub(crate) fn prepare_dragonwilds_player_query_probe(
    root: &Path,
    archive: &Path,
) -> Result<(), String> {
    if !root.is_absolute() || !root.join(".lgsm-isolated-player-probe").is_file() {
        return Err("Native loader acceptance requires a marked isolated installation.".into());
    }
    let (proxy, ue4ss_license, fmt_license) = bundled_files();
    if files::inspect(root, proxy)? != files::Preparation::NeedsInstall {
        return Err(
            "Native loader acceptance must start from an unprepared isolated runtime.".into(),
        );
    }
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(archive)
        .map_err(|error| error.to_string())?
        .take(files::ARCHIVE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    files::install(root, &bytes, proxy, ue4ss_license, fmt_license)?;
    verify_dragonwilds_player_query_probe(root)
}

#[cfg(test)]
pub(crate) fn verify_dragonwilds_player_query_probe(root: &Path) -> Result<(), String> {
    if files::inspect(root, bundled_files().0)? != files::Preparation::Ready {
        return Err("Native loader acceptance did not retain a verified installation.".into());
    }
    Ok(())
}
