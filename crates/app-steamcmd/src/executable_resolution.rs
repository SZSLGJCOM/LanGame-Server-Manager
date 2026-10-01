use std::path::{Path, PathBuf};
use std::{env, fs};

use app_core::ProcessSpec;

#[cfg(windows)]
use super::xna_framework_is_installed;

pub(super) fn resolve_probe_process_executable(
    install_root: &Path,
    process: &ProcessSpec,
    verification_path: Option<&Path>,
) -> PathBuf {
    if process.executable.contains("{{")
        && process.executable.contains("}}")
        && let Some(verification_path) = verification_path
        && is_executable_verification_path(verification_path)
        && let Some(file_name) = verification_path.file_name().and_then(|name| name.to_str())
    {
        return find_executable_by_name(install_root, file_name)
            .unwrap_or_else(|| verification_path.to_path_buf());
    }

    resolve_process_executable(install_root, &process.executable)
}

fn is_executable_verification_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "exe" | "bat" | "cmd" | "com" | "sh"
            )
        })
        .unwrap_or(false)
}

fn resolve_process_executable(install_root: &Path, configured_executable: &str) -> PathBuf {
    let direct_path = PathBuf::from(configured_executable);
    if direct_path.is_absolute() {
        return direct_path;
    }

    let relative_path = normalized_relative_path(configured_executable);
    let configured_path = install_root.join(&relative_path);
    if configured_path.exists() {
        return configured_path;
    }

    let Some(file_name) = relative_path.file_name().and_then(|name| name.to_str()) else {
        return configured_path;
    };

    // A bundled runtime path must not silently switch to another Java installation.
    if is_path_resolved_runtime(file_name) && relative_path.components().count() > 1 {
        return configured_path;
    }

    find_executable_by_name(install_root, file_name)
        .or_else(|| {
            if is_path_resolved_runtime(file_name) {
                find_executable_on_path(file_name)
            } else {
                None
            }
        })
        .unwrap_or(configured_path)
}

pub(super) fn normalized_relative_path(configured_path: &str) -> PathBuf {
    let mut normalized = PathBuf::new();
    for segment in configured_path.split(['/', '\\']) {
        if !segment.is_empty() {
            normalized.push(segment);
        }
    }
    normalized
}

fn find_executable_by_name(search_root: &Path, file_name: &str) -> Option<PathBuf> {
    if !search_root.exists() {
        return None;
    }

    let mut matches = Vec::new();
    collect_executable_matches(search_root, file_name, &mut matches);
    matches.sort_by(|left, right| compare_executable_candidates(left, right));
    matches.into_iter().next()
}

fn collect_executable_matches(search_root: &Path, file_name: &str, matches: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(search_root) else {
        return;
    };

    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };
        let path = entry.path();

        if path.is_dir() {
            if is_steam_staging_directory(&path) {
                continue;
            }
            collect_executable_matches(&path, file_name, matches);
            continue;
        }

        let matches_file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.eq_ignore_ascii_case(file_name))
            .unwrap_or(false);
        if matches_file_name {
            matches.push(path);
        }
    }
}

fn is_steam_staging_directory(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.eq_ignore_ascii_case("steamapps"))
        .unwrap_or(false)
}

fn compare_executable_candidates(left: &Path, right: &Path) -> std::cmp::Ordering {
    executable_candidate_rank(left)
        .cmp(&executable_candidate_rank(right))
        .then_with(|| left.components().count().cmp(&right.components().count()))
        .then_with(|| {
            left.to_string_lossy()
                .len()
                .cmp(&right.to_string_lossy().len())
        })
        .then_with(|| left.to_string_lossy().cmp(&right.to_string_lossy()))
}

fn executable_candidate_rank(path: &Path) -> (u8, u8, u8) {
    let components = path
        .parent()
        .into_iter()
        .flat_map(|parent| parent.components())
        .filter_map(|component| component.as_os_str().to_str())
        .map(|value| value.to_ascii_lowercase())
        .collect::<Vec<_>>();

    let has_windows = components.iter().any(|value| is_windows_component(value));
    let has_linux = components.iter().any(|value| is_linux_component(value));
    let has_macos = components.iter().any(|value| is_macos_component(value));

    #[cfg(windows)]
    {
        let requires_runtime_fallback =
            is_terraria_windows_candidate(path) && !xna_framework_is_installed();
        (
            requires_runtime_fallback as u8,
            (has_linux || has_macos) as u8,
            (!has_windows) as u8,
        )
    }

    #[cfg(target_os = "macos")]
    {
        ((has_windows || has_linux) as u8, (!has_macos) as u8, 0)
    }

    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        ((has_windows || has_macos) as u8, (!has_linux) as u8, 0)
    }
}

fn is_windows_component(value: &str) -> bool {
    matches!(value, "windows" | "win64" | "win32")
}

fn is_linux_component(value: &str) -> bool {
    value == "linux" || value.starts_with("linux-")
}

fn is_macos_component(value: &str) -> bool {
    matches!(value, "mac" | "macos" | "osx")
}

#[cfg(windows)]
fn is_terraria_windows_candidate(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.eq_ignore_ascii_case("TerrariaServer.exe"))
        .unwrap_or(false)
        && path
            .parent()
            .into_iter()
            .flat_map(|parent| parent.components())
            .filter_map(|component| component.as_os_str().to_str())
            .map(|value| value.to_ascii_lowercase())
            .any(|value| is_windows_component(&value))
}

fn is_path_resolved_runtime(file_name: &str) -> bool {
    file_name.eq_ignore_ascii_case("java") || file_name.eq_ignore_ascii_case("java.exe")
}

fn find_executable_on_path(file_name: &str) -> Option<PathBuf> {
    env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| env::split_paths(&paths).collect::<Vec<_>>())
        .map(|directory| directory.join(file_name))
        .find(|candidate| candidate.exists())
}
