use std::collections::HashSet;
#[cfg(windows)]
use std::env;
use std::path::{Path, PathBuf};

use app_core::AppSettings;

use super::ownership::configured_steamcmd_ownership;
use super::{SteamCmdOwnership, SteamCmdSource, SteamCmdStatus};

pub(super) fn require_steamcmd_ready(
    settings: &AppSettings,
) -> Result<SteamCmdStatus, super::SteamCmdError> {
    let status = steamcmd_status(settings);
    if !status.ready || !status.executable_exists {
        return Err(super::SteamCmdError::SteamCmdNotReady {
            path: status.executable_path,
        });
    }
    Ok(status)
}

#[derive(Debug, Clone)]
pub(super) struct SteamCmdCandidate {
    pub(super) root: PathBuf,
    pub(super) executable_path: PathBuf,
    pub(super) source: SteamCmdSource,
}

pub(super) fn configured_steamcmd_root(settings: &AppSettings) -> PathBuf {
    PathBuf::from(&settings.steamcmd_root)
}

fn configured_steamcmd_executable_path(settings: &AppSettings) -> PathBuf {
    PathBuf::from(&settings.steamcmd_root).join("steamcmd.exe")
}

pub fn steamcmd_executable_path(settings: &AppSettings) -> PathBuf {
    resolve_existing_steamcmd_candidate(settings)
        .map(|candidate| candidate.executable_path)
        .unwrap_or_else(|| configured_steamcmd_executable_path(settings))
}

pub fn steamcmd_status(settings: &AppSettings) -> SteamCmdStatus {
    let configured_root = configured_steamcmd_root(settings);
    let configured_executable_path = configured_steamcmd_executable_path(settings);
    let candidate =
        resolve_existing_steamcmd_candidate(settings).unwrap_or_else(|| SteamCmdCandidate {
            root: configured_root.clone(),
            executable_path: configured_executable_path.clone(),
            source: SteamCmdSource::Configured,
        });
    let ownership = if matches!(candidate.source, SteamCmdSource::Discovered) {
        SteamCmdOwnership::External
    } else {
        configured_steamcmd_ownership(&configured_root)
    };

    SteamCmdStatus {
        root: candidate.root.to_string_lossy().into_owned(),
        executable_path: candidate.executable_path.to_string_lossy().into_owned(),
        executable_exists: candidate.executable_path.exists(),
        ready: super::steamcmd_readiness::ready(&candidate.root, ownership),
        configured_root: configured_root.to_string_lossy().into_owned(),
        configured_executable_path: configured_executable_path.to_string_lossy().into_owned(),
        source: candidate.source,
        ownership,
        can_uninstall: matches!(ownership, SteamCmdOwnership::Managed),
    }
}

pub fn managed_steamcmd_status(settings: &AppSettings) -> SteamCmdStatus {
    let configured_root = configured_steamcmd_root(settings);
    let configured_executable_path = configured_steamcmd_executable_path(settings);
    let ownership = configured_steamcmd_ownership(&configured_root);

    SteamCmdStatus {
        root: configured_root.to_string_lossy().into_owned(),
        executable_path: configured_executable_path.to_string_lossy().into_owned(),
        executable_exists: configured_executable_path.exists(),
        ready: super::steamcmd_readiness::ready(&configured_root, ownership),
        configured_root: configured_root.to_string_lossy().into_owned(),
        configured_executable_path: configured_executable_path.to_string_lossy().into_owned(),
        source: SteamCmdSource::Configured,
        ownership,
        can_uninstall: matches!(ownership, SteamCmdOwnership::Managed),
    }
}

fn resolve_existing_steamcmd_candidate(settings: &AppSettings) -> Option<SteamCmdCandidate> {
    let configured_root = configured_steamcmd_root(settings);
    candidate_from_root(&configured_root, SteamCmdSource::Configured)
        .or_else(|| discover_steamcmd_candidate(settings))
}

fn candidate_from_root(root: &Path, source: SteamCmdSource) -> Option<SteamCmdCandidate> {
    let executable_path = root.join("steamcmd.exe");
    if !executable_path.exists() {
        return None;
    }

    Some(SteamCmdCandidate {
        root: root.to_path_buf(),
        executable_path,
        source,
    })
}

#[cfg(windows)]
fn discover_steamcmd_candidate(settings: &AppSettings) -> Option<SteamCmdCandidate> {
    let configured_root = configured_steamcmd_root(settings);
    let candidate_roots = windows_steamcmd_candidate_roots();
    pick_existing_steamcmd_candidate(&configured_root, &candidate_roots)
}

#[cfg(not(windows))]
fn discover_steamcmd_candidate(_settings: &AppSettings) -> Option<SteamCmdCandidate> {
    None
}

pub(super) fn pick_existing_steamcmd_candidate(
    configured_root: &Path,
    candidate_roots: &[PathBuf],
) -> Option<SteamCmdCandidate> {
    let configured_key = normalized_path_key(configured_root);
    let mut seen = HashSet::new();

    for root in candidate_roots {
        let root_key = normalized_path_key(root);
        if root_key == configured_key || !seen.insert(root_key) {
            continue;
        }

        if let Some(candidate) = candidate_from_root(root, SteamCmdSource::Discovered) {
            return Some(candidate);
        }
    }

    None
}

fn normalized_path_key(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase()
}

#[cfg(windows)]
fn windows_steamcmd_candidate_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    if let Some(path_entries) = env::var_os("PATH") {
        for directory in env::split_paths(&path_entries) {
            roots.push(directory);
        }
    }

    if let Some(program_data) = env_path("PROGRAMDATA") {
        roots.push(program_data.join("LanGame").join("steamcmd"));
        roots.push(program_data.join("SteamCMD"));
        roots.push(program_data.join("steamcmd"));
    }

    if let Some(program_files_x86) = env_path("ProgramFiles(x86)") {
        roots.push(program_files_x86.join("SteamCMD"));
        roots.push(program_files_x86.join("steamcmd"));
    }

    if let Some(program_files) = env_path("ProgramFiles") {
        roots.push(program_files.join("SteamCMD"));
        roots.push(program_files.join("steamcmd"));
    }

    if let Some(user_profile) = env_path("USERPROFILE") {
        roots.push(user_profile.join("steamcmd"));
        roots.push(user_profile.join("SteamCMD"));
    }

    roots.push(PathBuf::from(r"C:\steamcmd"));
    roots.push(PathBuf::from(r"C:\SteamCMD"));
    roots
}

#[cfg(windows)]
fn env_path(name: &str) -> Option<PathBuf> {
    env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}
