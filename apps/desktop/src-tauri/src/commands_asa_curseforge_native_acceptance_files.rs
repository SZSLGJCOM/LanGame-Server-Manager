use super::{AcceptanceResult, MOD_IDS};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::fs;
use std::io::BufRead;
use std::os::windows::fs::MetadataExt;
use std::path::{Component, Path};
use std::time::{Duration, Instant};

pub(super) fn plain_path(path: &Path) -> AcceptanceResult<()> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return Err("native acceptance requires an absolute ordinary path".into());
    }
    for ancestor in path.ancestors().filter(|item| !item.as_os_str().is_empty()) {
        let metadata = fs::symlink_metadata(ancestor)?;
        if metadata.file_type().is_symlink()
            || metadata.file_attributes() & 0x400 != 0
            || (ancestor != path && !metadata.is_dir())
        {
            return Err(
                "native acceptance refuses reparse points or non-directory ancestors".into(),
            );
        }
    }
    Ok(())
}

pub(super) fn check_source_user_data(root: &Path) -> AcceptanceResult<()> {
    plain_path(root)?;
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if entry.file_name() != "83374" {
            return Err("native source contains SDK user context".into());
        }
        plain_path(&entry.path())?;
        for child in fs::read_dir(entry.path())? {
            let child = child?;
            if child.file_name() != "library.json" || !child.file_type()?.is_file() {
                return Err("native source must contain only the system library manifest".into());
            }
            plain_path(&child.path())?;
        }
    }
    let library = read_library(root.parent().ok_or("native SDK root is missing")?)?;
    if library["installedMods"].as_array().is_none_or(|items| {
        items.iter().any(|item| {
            item.get("downloadInfo").is_some()
                || item["users"]
                    .as_array()
                    .is_none_or(|users| !users.is_empty())
        })
    }) {
        return Err(
            "native source library must omit downloadInfo and have empty users associations".into(),
        );
    }
    Ok(())
}

fn read_library(sdk_root: &Path) -> AcceptanceResult<Value> {
    let path = sdk_root.join("ModsUserData/83374/library.json");
    plain_path(&path)?;
    if !fs::metadata(&path)?.is_file() || fs::metadata(&path)?.len() > 16 * 1024 * 1024 {
        return Err("native library manifest is not a bounded ordinary file".into());
    }
    let bytes = fs::read(path)?;
    // CFCore writes this manifest as UTF-8 with a BOM after a native run.
    // Remove only that exact prefix for parsing; retain the source bytes.
    let library: Value =
        serde_json::from_slice(bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes))?;
    if library["installedMods"]
        .as_array()
        .is_none_or(|items| items.len() > 8192)
    {
        return Err("native library has no bounded installedMods array".into());
    }
    Ok(library)
}

fn positive(value: &Value) -> AcceptanceResult<i64> {
    value
        .as_i64()
        .filter(|id| *id > 0)
        .ok_or_else(|| "native library has an invalid numeric Mod/file identity".into())
}

pub(super) fn library_projection(sdk_root: &Path, ids: &[i64]) -> AcceptanceResult<Vec<Value>> {
    let library = read_library(sdk_root)?;
    let items = library["installedMods"]
        .as_array()
        .ok_or("native installedMods array is missing")?;
    let mut result = Vec::new();
    for id in ids {
        let matches = items
            .iter()
            .filter(|item| item["details"]["iD"].as_i64() == Some(*id))
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            return Err("native library must contain exactly one record per selected Mod".into());
        }
        let item = matches[0];
        if item["details"]["gameId"].as_i64() != Some(83374) {
            return Err("native library Mod belongs to another game".into());
        }
        let installed = positive(&item["installedFile"]["iD"])?;
        let latest = positive(&item["latestUpdatedFile"]["iD"])?;
        let main = positive(&item["details"]["mainFileId"])?;
        let path = item["pathOnDisk"]
            .as_str()
            .ok_or("native library pathOnDisk is missing")?;
        let relative = Path::new(path);
        if relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
            || relative.components().count() != 2
            || relative != Path::new("83374").join(format!("{id}_{installed}"))
        {
            return Err("native library pathOnDisk escapes its installed Mod wrapper".into());
        }
        let payload = sdk_root.join("Mods").join(relative);
        plain_path(&payload)?;
        if !fs::metadata(payload)?.is_dir() {
            return Err("native installed Mod payload is missing".into());
        }
        let status = item["status"]
            .as_str()
            .ok_or("native library status is missing")?;
        result.push(
            json!({"project_id":id,"installed_file_id":installed,"latest_updated_file_id":latest,
            "main_file_id":main,"status":status,"path_on_disk":path}),
        );
    }
    Ok(result)
}

pub(super) fn require_current_installed(cache: &[Value]) -> AcceptanceResult<()> {
    if cache.len() != MOD_IDS.len()
        || cache.iter().any(|item| {
            item["status"] != "Normal" || item["installed_file_id"] != item["main_file_id"]
        })
    {
        return Err("native installed cache does not match its selected current main file".into());
    }
    // latestUpdatedFile may retain a stale warm value in CFCore. The current
    // native validation and selected main file establish the required revision.
    Ok(())
}

pub(super) fn seed_cache(
    source_sdk: &Path,
    destination_sdk: &Path,
    ids: &[i64],
) -> AcceptanceResult<()> {
    let before = library_projection(source_sdk, ids)?;
    plain_path(destination_sdk)?;
    if dunce::canonicalize(source_sdk)? == dunce::canonicalize(destination_sdk)? {
        return Ok(());
    }
    let mut library = read_library(source_sdk)?;
    let selected = library["installedMods"]
        .as_array()
        .ok_or("native library is missing")?
        .iter()
        .filter(|item| ids.contains(&item["details"]["iD"].as_i64().unwrap_or(0)))
        .cloned()
        .map(|mut item| {
            if let Some(object) = item.as_object_mut() {
                object.insert("users".into(), json!([]));
                object.remove("downloadInfo");
            }
            item
        })
        .collect::<Vec<_>>();
    library["installedMods"] = json!(selected);
    let mut budget = (0_usize, 0_u64);
    let started = Instant::now();
    for item in &before {
        let relative = item["path_on_disk"]
            .as_str()
            .ok_or("selected cache path is missing")?;
        let source = source_sdk.join("Mods").join(relative);
        let target = destination_sdk.join("Mods").join(relative);
        fs::create_dir_all(&target)?;
        plain_path(&target)?;
        copy_tree(&source, &target, &mut budget, started, 0)?;
    }
    let user_data = destination_sdk.join("ModsUserData/83374");
    fs::create_dir_all(&user_data)?;
    plain_path(&user_data)?;
    let manifest = user_data.join("library.json");
    if manifest.try_exists()? {
        plain_path(&manifest)?;
    }
    fs::write(manifest, serde_json::to_vec(&library)?)?;
    if library_projection(destination_sdk, ids)? != before {
        return Err("seeded cache projection differs from the source".into());
    }
    Ok(())
}

fn copy_tree(
    source: &Path,
    target: &Path,
    budget: &mut (usize, u64),
    started: Instant,
    depth: usize,
) -> AcceptanceResult<()> {
    plain_path(source)?;
    plain_path(target)?;
    if depth > 64 || started.elapsed() > Duration::from_secs(1200) {
        return Err("native cache copy exceeded its depth/time budget".into());
    }
    for entry in fs::read_dir(source)? {
        budget.0 += 1;
        if budget.0 > 200_000 {
            return Err("native cache copy exceeded its entry budget".into());
        }
        let entry = entry?;
        let from = entry.path();
        plain_path(&from)?;
        let to = target.join(entry.file_name());
        if to.try_exists()? {
            plain_path(&to)?;
        }
        let metadata = fs::symlink_metadata(&from)?;
        if metadata.is_dir() {
            fs::create_dir_all(&to)?;
            copy_tree(&from, &to, budget, started, depth + 1)?;
        } else if metadata.is_file() {
            budget.1 = budget
                .1
                .checked_add(metadata.len())
                .ok_or("native cache copy byte count overflow")?;
            if budget.1 > 64 * 1024 * 1024 * 1024 {
                return Err("native cache copy exceeded its byte budget".into());
            }
            fs::copy(from, to)?;
        } else {
            return Err("native cache contains a non-ordinary entry".into());
        }
    }
    Ok(())
}

pub(super) fn native_evidence(
    path: &Path,
    since_ms: u64,
) -> AcceptanceResult<(Vec<Value>, BTreeSet<i64>, bool)> {
    plain_path(path)?;
    if fs::metadata(path)?.len() > 64 * 1024 * 1024 {
        return Err("native evidence log exceeded 64 MiB".into());
    }
    let mut selected = Vec::new();
    let mut valid = BTreeSet::new();
    let mut terminal = false;
    for line in std::io::BufReader::new(fs::File::open(path)?).lines() {
        let line = line?;
        if line.len() > 16 * 1024 {
            continue;
        }
        let Some((timestamp, message)) = app_storage::ark_native_log_message(&line) else {
            continue;
        };
        if timestamp < since_ms {
            continue;
        }
        if message
            == "ASAMods: Error: Not all mods were installed. Check the log for CFCore errors."
        {
            if selected.len() >= 128 {
                return Err("native selected CF evidence exceeded its record budget".into());
            }
            terminal = true;
            // The game emits this timestamped terminal record before optional
            // unstructured continuation lines. Retain only the known marker.
            selected.push(json!({"timestamp_ms":timestamp, "category":"ASAMods",
                "level":"Error", "marker":"missing_requested_mods"}));
            continue;
        }
        let Some(record) = message.strip_prefix("LogCFCore:").map(str::trim_start) else {
            continue;
        };
        let (level, detail) = ["Display:", "Log:", "Warning:", "Error:"]
            .into_iter()
            .find_map(|level| {
                record
                    .strip_prefix(level)
                    .map(|detail| (level, detail.trim_start()))
            })
            .unwrap_or(("Log:", record));
        let id = detail.strip_prefix("Mod valid: ").and_then(named_id);
        if matches!(level, "Log:" | "Display:")
            && let Some(id) = id
        {
            valid.insert(id);
        }
        let failure = terminal_marker(detail);
        terminal |= failure.is_some();
        if id.is_some() || failure.is_some() {
            if selected.len() >= 128 {
                return Err("native selected CF evidence exceeded its record budget".into());
            }
            // API errors may contain signed URLs or response bodies. Preserve
            // their observed marker, never their tail. A success line is kept
            // only after checking it separately for private information.
            let marker = if id.is_some() {
                "mod_valid"
            } else {
                failure.ok_or("terminal marker is missing")?
            };
            let mut evidence = json!({"timestamp_ms":timestamp, "category":"LogCFCore", "level":level, "marker":marker, "project_id":id});
            if id.is_some() && safe_line(&line) {
                evidence["line"] = json!(line);
            }
            selected.push(evidence);
        }
    }
    Ok((selected, valid, terminal))
}

fn decimal(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn named_id(value: &str) -> Option<i64> {
    let (name, id) = value.rsplit_once(" (")?;
    let id = id.strip_suffix(')')?;
    (!name.is_empty() && decimal(id))
        .then_some(id)?
        .parse()
        .ok()
}

fn terminal_marker(detail: &str) -> Option<&'static str> {
    if detail.starts_with("Error querying server mods: ") {
        Some("error_querying_server_mods")
    } else if detail
        .strip_prefix("Missing installed mods - (")
        .and_then(|value| value.strip_suffix(')'))
        .and_then(|value| value.split_once('/'))
        .is_some_and(|(have, need)| decimal(have) && decimal(need))
    {
        Some("missing_installed_mods")
    } else if detail
        .strip_prefix("Detected an unavailable mod: ")
        .and_then(|value| value.strip_suffix("! Ensuring mod is deleted..."))
        .and_then(named_id)
        .is_some()
    {
        Some("unavailable_mod")
    } else if detail
        .strip_prefix("Mod: ")
        .and_then(|value| value.rsplit_once(" doesn't contain it's main file ("))
        .is_some_and(|(item, file)| {
            named_id(item).is_some() && file.strip_suffix(")!").is_some_and(decimal)
        })
    {
        Some("missing_main_file")
    } else {
        None
    }
}

fn safe_line(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    !super::diagnostics::contains_private_runtime_identity(line)
        && ![
            "token",
            "password",
            "api_key",
            "apikey",
            "authorization",
            "user_info",
            "machineid",
            "account",
            "credential",
            "://",
            "www.",
            "signature",
            "secret",
            "bearer",
            "session",
            "userid",
            "user_id",
        ]
        .iter()
        .any(|marker| lower.contains(marker))
}

pub(super) fn safe_error(error: &str) -> String {
    if !safe_line(error) {
        return "native operation failed; inspect the retained owned native logs".into();
    }
    error.chars().take(1200).collect()
}

pub(super) fn save_report(directory: &Path, report: &Value) -> AcceptanceResult<()> {
    plain_path(directory)?;
    let path = directory.join("report.json");
    if path.try_exists()? {
        plain_path(&path)?;
    }
    fs::write(path, serde_json::to_vec(report)?)?;
    Ok(())
}

pub(super) fn ensure_no_owned_process(executable: &Path) -> AcceptanceResult<()> {
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_NO_MORE_FILES, HANDLE, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    struct Snapshot(HANDLE);
    impl Drop for Snapshot {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    plain_path(executable)?;
    let expected = dunce::canonicalize(executable)?
        .to_string_lossy()
        .to_ascii_lowercase();
    let name = executable
        .file_name()
        .ok_or("native executable has no filename")?
        .to_string_lossy();
    let handle = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if handle == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error().into());
    }
    let snapshot = Snapshot(handle);
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
    let mut available = unsafe { Process32FirstW(snapshot.0, &mut entry) };
    while available != 0 {
        let end = entry
            .szExeFile
            .iter()
            .position(|character| *character == 0)
            .unwrap_or(entry.szExeFile.len());
        if String::from_utf16_lossy(&entry.szExeFile[..end]).eq_ignore_ascii_case(&name)
            && let Some(identity) = app_runtime::inspect_process_identity(entry.th32ProcessID)?
        {
            let image = dunce::canonicalize(identity.image_path)?
                .to_string_lossy()
                .to_ascii_lowercase();
            if image == expected {
                return Err(
                    "native acceptance found a live process using its exact owned executable"
                        .into(),
                );
            }
        }
        available = unsafe { Process32NextW(snapshot.0, &mut entry) };
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() != Some(ERROR_NO_MORE_FILES as i32) {
        return Err(error.into());
    }
    Ok(())
}

#[path = "commands_asa_curseforge_native_acceptance_files_tests.rs"]
mod tests;
