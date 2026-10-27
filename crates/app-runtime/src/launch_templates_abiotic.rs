use super::*;

const DOWNLOAD_DIRECTORY: &str = "AbioticFactor/Saved/mod.io";
const SIDELOAD_DIRECTORY: &str = "AbioticFactor/Mods";

fn setting_text<'a>(settings: &'a Value, key: &str, fallback: &'a str) -> &'a str {
    settings
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or(fallback)
        .trim()
}

fn mod_ids(settings: &Value) -> Result<Vec<&str>, ()> {
    let mut ids = Vec::new();
    for id in setting_text(settings, "mod_ids", "")
        .split(|character: char| character.is_whitespace() || matches!(character, ',' | ';'))
        .filter(|id| !id.is_empty())
    {
        if id.starts_with('0')
            || !id.bytes().all(|byte| byte.is_ascii_digit())
            || id.parse::<u64>().is_err()
        {
            return Err(());
        }
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    Ok(ids)
}

fn directory_parts(value: &str, download: bool) -> Option<Vec<&str>> {
    let parts = value.split(['/', '\\']).collect::<Vec<_>>();
    if parts.iter().any(|part| {
        part.is_empty()
            || *part == "."
            || *part == ".."
            || part.ends_with(['.', ' '])
            || part.chars().any(|character| {
                character.is_control()
                    || matches!(character, '<' | '>' | ':' | '"' | '|' | '?' | '*')
            })
            || is_windows_reserved_name(part)
    }) {
        return None;
    }
    let prefix = if download { "Saved" } else { "Mods" };
    if parts.len() < if download { 3 } else { 2 }
        || !parts[0].eq_ignore_ascii_case("AbioticFactor")
        || !parts[1].eq_ignore_ascii_case(prefix)
    {
        return None;
    }
    Some(parts)
}

fn is_windows_reserved_name(part: &str) -> bool {
    let name = part
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    matches!(name.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || name
            .strip_prefix("COM")
            .or_else(|| name.strip_prefix("LPT"))
            .is_some_and(|number| {
                matches!(number, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
            })
}

fn valid_world_save_name(value: &str) -> bool {
    !value.is_empty()
        && (!value.ends_with(['.', ' '])
            && !value.chars().any(|character| {
                matches!(
                    character,
                    '\0'..='\u{1f}'
                        | '\u{7f}'
                        | '<'
                        | '>'
                        | ':'
                        | '"'
                        | '/'
                        | '\\'
                        | '|'
                        | '?'
                        | '*'
                )
            })
            && !is_windows_reserved_name(value))
}

fn issue(field: &str, code: &str, message: &str) -> LaunchValidationIssue {
    LaunchValidationIssue {
        code: code.into(),
        context: BTreeMap::from([("setting_key".into(), field.into())]),
        severity: "error".into(),
        message: message.into(),
        path: None,
    }
}

pub(crate) fn collect_abioticfactor_launch_setting_issues(
    settings: &Value,
) -> Vec<LaunchValidationIssue> {
    let mut issues = Vec::new();
    for field in [
        "server_name",
        "world_save_name",
        "server_password",
        "admin_password",
    ] {
        let Some(value) = settings.get(field).and_then(Value::as_str) else {
            continue;
        };
        // The native value parser stops at the first literal quote and performs
        // no escape decoding. Never start with a silently shortened credential.
        if value.contains('"') {
            issues.push(issue(
                field,
                "abiotic_launch_value_quote_unsupported",
                "Double quotes cannot be passed to Abiotic Factor in this setting. Remove the double quote before starting.",
            ));
        } else if field == "world_save_name" && !valid_world_save_name(value) {
            issues.push(issue(
                field,
                "abiotic_world_save_name_invalid",
                "Use a single non-empty Windows folder name, such as the instance ID. Do not use path separators, reserved names, or a trailing dot or space.",
            ));
        }
    }
    let ids = mod_ids(settings);
    if ids.is_err() {
        issues.push(issue(
            "mod_ids",
            "abiotic_mod_ids_invalid",
            "Use positive mod.io numeric IDs separated by newlines, commas or semicolons.",
        ));
    }
    let enabled = ids.as_ref().is_ok_and(|ids| !ids.is_empty());
    let sideload = lookup_json_bool(settings, "sideload_mods").unwrap_or(false);
    let token = setting_text(settings, "modio_token", "");
    if token
        .chars()
        .any(|character| character.is_whitespace() || character.is_control() || character == '"')
    {
        issues.push(issue(
            "modio_token",
            "abiotic_modio_token_invalid",
            "The mod.io token cannot contain whitespace, control characters or double quotes.",
        ));
    }
    for (field, fallback, download) in [
        ("mods_directory", DOWNLOAD_DIRECTORY, true),
        ("sideload_mods_path", SIDELOAD_DIRECTORY, false),
    ] {
        if directory_parts(setting_text(settings, field, fallback), download).is_none() {
            issues.push(issue(field, "abiotic_mod_directory_invalid", if download {
                "Choose a relative subdirectory inside this instance's AbioticFactor/Saved directory. Absolute paths, parent traversal and Windows special names are not allowed."
            } else {
                "Choose AbioticFactor/Mods or one of its subdirectories in this instance. Absolute paths, parent traversal and Windows special names are not allowed."
            }));
        }
    }
    if sideload && !enabled {
        issues.push(issue(
            "mod_ids",
            "abiotic_sideload_ids_required",
            "Sideloaded Mods still require their mod.io IDs in the official Mod list.",
        ));
    }
    if enabled
        && !sideload
        && token.is_empty()
        && setting_text(settings, "mods_directory", DOWNLOAD_DIRECTORY) != DOWNLOAD_DIRECTORY
    {
        issues.push(issue("mods_directory", "abiotic_mod_directory_requires_token", "A custom download directory requires a mod.io token. Without a token, the server uses its default directory and downloads Mods again at each startup."));
    }
    if settings
        .get("mod_install_timeout_seconds")
        .is_some_and(|value| value.as_u64().is_none_or(|value| value == 0))
    {
        issues.push(issue(
            "mod_install_timeout_seconds",
            "abiotic_mod_timeout_invalid",
            "The Mod installation timeout must be a positive whole number of seconds.",
        ));
    }
    issues
}

pub(super) fn render_abioticfactor_password_flag(
    settings: &Value,
    field: &str,
    prefix: &str,
) -> String {
    // Unlike numeric IDs and directory inputs, native passwords are literal
    // values. Trimming would silently change a valid join/admin credential.
    settings
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(|value| format!("{prefix}{value}"))
        .unwrap_or_default()
}

fn resolved_directory(root: &Path, value: &str, download: bool) -> Option<PathBuf> {
    let parts = directory_parts(value, download)?;
    Some(
        parts
            .iter()
            .fold(root.to_path_buf(), |path, part| path.join(part)),
    )
}

pub(super) fn render_abioticfactor_mod_args(context: &TemplateContext<'_>) -> Vec<String> {
    // Validation rejects malformed state before spawning. Never partially load a
    // valid subset of an invalid Mod list, or pass an unused account token.
    if !collect_abioticfactor_launch_setting_issues(context.settings).is_empty() {
        return Vec::new();
    }
    let ids = mod_ids(context.settings).unwrap_or_default();
    if ids.is_empty() {
        return Vec::new();
    }
    let mut args = vec![format!("-Mods={}", ids.join(";"))];
    let sideload = lookup_json_bool(context.settings, "sideload_mods").unwrap_or(false);
    if sideload {
        args.push("-SideloadMods".into());
        if let Some(path) = resolved_directory(
            context.install_root,
            setting_text(context.settings, "sideload_mods_path", SIDELOAD_DIRECTORY),
            false,
        ) {
            args.push(format!("-SideloadModsPath={}", path.display()));
        }
    } else {
        let token = setting_text(context.settings, "modio_token", "");
        if !token.is_empty() {
            args.push(format!("-ModioToken={token}"));
            if let Some(path) = resolved_directory(
                context.install_root,
                setting_text(context.settings, "mods_directory", DOWNLOAD_DIRECTORY),
                true,
            ) {
                args.push(format!("-ModsDir={}", path.display()));
            }
        }
        let timeout = context
            .settings
            .get("mod_install_timeout_seconds")
            .and_then(Value::as_u64)
            .unwrap_or(900);
        args.push(format!("-ModInstallTimeout={timeout}"));
    }
    if lookup_json_bool(context.settings, "override_mod_warnings").unwrap_or(false) {
        args.push("-OverrideModWarnings".into());
    }
    args
}

pub(crate) fn collect_abioticfactor_mod_path_issues(
    context: &TemplateContext<'_>,
) -> Vec<LaunchValidationIssue> {
    if context.instance.summary.module_id != "abioticfactor"
        || !mod_ids(context.settings).is_ok_and(|ids| !ids.is_empty())
    {
        return Vec::new();
    }
    let sideload = lookup_json_bool(context.settings, "sideload_mods").unwrap_or(false);
    let (field, fallback, download) = if sideload {
        ("sideload_mods_path", SIDELOAD_DIRECTORY, false)
    } else {
        ("mods_directory", DOWNLOAD_DIRECTORY, true)
    };
    let relative = if !sideload && setting_text(context.settings, "modio_token", "").is_empty() {
        DOWNLOAD_DIRECTORY
    } else {
        setting_text(context.settings, field, fallback)
    };
    let Some(parts) = directory_parts(relative, download) else {
        return Vec::new();
    };
    let mut path = context.install_root.to_path_buf();
    for part in std::iter::once(None).chain(parts.into_iter().map(Some)) {
        if let Some(part) = part {
            path.push(part);
        }
        match fs::symlink_metadata(&path) {
            Ok(_) if !is_preparation_directory(&path) => {
                return vec![issue(
                    field,
                    "abiotic_mod_directory_unsafe",
                    "The Mod directory must contain only ordinary directories inside this instance. A file, symbolic link or reparse point was preserved; select an instance-owned directory before starting.",
                )];
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(_) => {
                return vec![issue(
                    field,
                    "abiotic_mod_directory_unreadable",
                    "The Mod directory could not be inspected. Check its access permissions before starting.",
                )];
            }
        }
    }
    Vec::new()
}

#[cfg(test)]
#[path = "launch_templates_abiotic_tests.rs"]
mod tests;
