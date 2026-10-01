use super::*;

#[path = "launch_templates_ark.rs"]
mod launch_templates_ark;
pub(super) use launch_templates_ark::collect_ark_cluster_launch_issues;
use launch_templates_ark::*;

pub(super) struct TemplateContext<'a> {
    pub(super) instance: &'a InstanceDetails,
    pub(super) settings: &'a Value,
    pub(super) install_root: &'a Path,
    pub(super) config_dir: &'a Path,
    pub(super) data_dir: &'a Path,
    pub(super) logs_dir: &'a Path,
    pub(super) saves_dir: &'a Path,
}

pub(super) fn can_prepare_launch_executable(
    module_id: &str,
    context: &TemplateContext<'_>,
    executable_path: &Path,
) -> bool {
    // These two entrypoints are materialized by storage before process launch.
    // Only recognize their exact managed path and an installed source program;
    // a missing custom executable must remain a blocking installation error.
    match module_id {
        "barotrauma" => {
            executable_path == context.config_dir.join("DedicatedServer.exe")
                && context.install_root.join("DedicatedServer.exe").is_file()
                && ["Content", "Data"]
                    .iter()
                    .all(|name| is_preparation_directory(&context.install_root.join(name)))
                && fs::canonicalize(context.install_root)
                    .ok()
                    .zip(fs::canonicalize(context.config_dir).ok())
                    .is_some_and(|(source, target)| {
                        !source.starts_with(&target) && !target.starts_with(&source)
                    })
        }
        "romestead" => {
            executable_path == context.install_root.join("start-romestead.bat")
                && context.install_root.join("Server.exe").is_file()
        }
        _ => false,
    }
}

fn is_preparation_directory(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| {
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return false;
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            metadata.file_attributes() & 0x400 == 0
        }
        #[cfg(not(windows))]
        true
    })
}

pub(super) fn resolve_template(template: &str, context: &TemplateContext<'_>) -> String {
    let mut resolved = String::new();
    let mut cursor = template;

    while let Some(open_index) = cursor.find("{{") {
        resolved.push_str(&cursor[..open_index]);
        let token_start = open_index + 2;

        if let Some(close_offset) = cursor[token_start..].find("}}") {
            let close_index = token_start + close_offset;
            let token = cursor[token_start..close_index].trim();
            let replacement =
                resolve_token(token, context).unwrap_or_else(|| format!("{{{{{token}}}}}"));
            resolved.push_str(&replacement);
            cursor = &cursor[close_index + 2..];
        } else {
            resolved.push_str(&cursor[open_index..]);
            return resolved;
        }
    }

    resolved.push_str(cursor);
    resolved
}

pub(super) fn expand_resolved_argument_segments(
    template: &str,
    context: &TemplateContext<'_>,
) -> Vec<String> {
    let resolved = resolve_template(template, context);
    #[cfg(windows)]
    let resolved = normalize_managed_path_argument(template, resolved);
    resolved
        .lines()
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .map(String::from)
        .collect()
}

pub(super) fn rendered_path(value: String) -> PathBuf {
    #[cfg(windows)]
    let value = if value.starts_with(r"\\?\") {
        value.replace('/', "\\")
    } else {
        value
    };
    PathBuf::from(value)
}

#[cfg(windows)]
fn normalize_managed_path_argument(template: &str, resolved: String) -> String {
    let Some((prefix, remainder)) = template.split_once("{{paths.") else {
        return resolved;
    };
    let Some((token, suffix)) = remainder.split_once("}}") else {
        return resolved;
    };
    if !matches!(
        token.trim(),
        "install_root" | "instance_root" | "config_dir" | "data_dir" | "logs_dir" | "saves_dir"
    ) || (!suffix.is_empty() && !suffix.starts_with(['/', '\\']))
        || suffix.chars().any(char::is_whitespace)
        || suffix.contains(['{', '}', ':'])
    {
        return resolved;
    }
    if !prefix.is_empty()
        && !prefix.strip_suffix('=').is_some_and(|flag| {
            !flag.is_empty()
                && flag.chars().all(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '-' | '+' | '_' | '.')
                })
        })
    {
        return resolved;
    }
    let Some(path) = resolved.strip_prefix(prefix) else {
        return resolved;
    };
    if !path.starts_with(r"\\?\") {
        return resolved;
    }
    // Only a declared managed path and its static suffix constitute this argument.
    // URLs, setting values, and free-form launch arguments retain their spelling.
    let path = compatible_native_path(rendered_path(path.to_owned()));
    format!("{prefix}{}", path.to_string_lossy())
}

#[cfg(windows)]
pub(super) fn native_path_candidate(path: &Path) -> Option<PathBuf> {
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Component, Prefix};

    let value = path.to_str()?;
    let candidate = match path.components().next()? {
        Component::Prefix(prefix) => match prefix.kind() {
            Prefix::VerbatimDisk(_) => PathBuf::from(value.strip_prefix(r"\\?\")?),
            Prefix::VerbatimUNC(_, _) => PathBuf::from(format!(r"\\{}", &value[8..])),
            _ => return None,
        },
        _ => return None,
    };
    // Below this boundary Rust and native runtimes use the ordinary spelling.
    if candidate.as_os_str().encode_wide().count() >= 247 {
        return None;
    }
    if candidate.components().any(|component| match component {
        Component::Normal(name) => name
            .to_str()
            .is_some_and(|value| value.ends_with(['.', ' '])),
        Component::Prefix(prefix) => match prefix.kind() {
            Prefix::UNC(server, share) => [server, share].iter().any(|name| {
                name.to_str()
                    .is_some_and(|value| value.ends_with(['.', ' ']))
            }),
            _ => false,
        },
        _ => false,
    }) {
        return None;
    }
    Some(candidate)
}

#[cfg(windows)]
pub(super) fn compatible_native_path(path: PathBuf) -> PathBuf {
    let Some(candidate) = native_path_candidate(&path) else {
        return path;
    };
    // A shorter spelling is usable only when it identifies the same existing
    // object. Keep long paths and namespace-sensitive names intact.
    match (fs::canonicalize(&path), fs::canonicalize(&candidate)) {
        (Ok(original), Ok(compatible)) if original == compatible => candidate,
        _ => path,
    }
}

#[cfg(windows)]
pub(super) fn native_working_directory_is_incompatible(
    module_id: &str,
    executable_path: &Path,
    args: &[String],
    working_directory: &Path,
) -> bool {
    use std::path::{Component, Prefix};

    let prefix = match working_directory.components().next() {
        Some(Component::Prefix(prefix)) => prefix.kind(),
        _ => return false,
    };
    let verbatim = matches!(
        prefix,
        Prefix::Verbatim(_) | Prefix::VerbatimDisk(_) | Prefix::VerbatimUNC(_, _)
    );
    let java = module_id == "necesse"
        || executable_path.file_name().is_some_and(|name| {
            name.to_str().is_some_and(|name| {
                name.eq_ignore_ascii_case("java.exe") || name.eq_ignore_ascii_case("javaw.exe")
            })
        });
    // CMD rejects both verbatim paths and ordinary UNC working directories;
    // Java's default filesystem rejects the remaining verbatim spelling.
    (java && verbatim)
        || (is_script_entrypoint(executable_path, args)
            && (verbatim || matches!(prefix, Prefix::UNC(_, _) | Prefix::DeviceNS(_))))
}

pub(super) fn resolve_token(token: &str, context: &TemplateContext<'_>) -> Option<String> {
    match token {
        "instance.id" => Some(context.instance.summary.id.clone()),
        "instance.name" => Some(context.instance.summary.name.clone()),
        "instance.bind_ip" => Some(context.instance.summary.bind_ip.clone()),
        "paths.install_root" => Some(context.install_root.to_string_lossy().into_owned()),
        "paths.instance_root" => Some(
            context
                .config_dir
                .parent()
                .unwrap_or(context.config_dir)
                .to_string_lossy()
                .into_owned(),
        ),
        "paths.config_dir" => Some(context.config_dir.to_string_lossy().into_owned()),
        "paths.data_dir" => Some(context.data_dir.to_string_lossy().into_owned()),
        "paths.logs_dir" => Some(context.logs_dir.to_string_lossy().into_owned()),
        "paths.saves_dir" => Some(context.saves_dir.to_string_lossy().into_owned()),
        _ => {
            if let Some(path) = token.strip_prefix("launch.") {
                lookup_extra_launch_args_token(context, path)
            } else if let Some(path) = token.strip_prefix("settings.") {
                lookup_json_path(context.settings, path)
            } else if let Some(path) = token.strip_prefix("abioticfactor.") {
                lookup_abioticfactor_launch_token(context, path)
            } else if let Some(path) = token.strip_prefix("arksa.") {
                lookup_ark_ascended_launch_token(context, path)
            } else if let Some(path) = token.strip_prefix("arkse.") {
                lookup_ark_evolved_launch_token(context, path)
            } else if let Some(path) = token.strip_prefix("soulmask.") {
                lookup_soulmask_launch_token(context, path)
            } else if let Some(path) = token.strip_prefix("terraria.") {
                lookup_terraria_launch_token(context, path)
            } else if let Some(path) = token.strip_prefix("conanexiles.") {
                lookup_conan_launch_token(context, path)
            } else if let Some(path) = token.strip_prefix("corekeeper.") {
                lookup_corekeeper_launch_token(context, path)
            } else if let Some(path) = token.strip_prefix("dontstarve.") {
                lookup_dontstarve_launch_token(context, path)
            } else if let Some(path) = token.strip_prefix("necesse.") {
                lookup_necesse_launch_token(context, path)
            } else if let Some(path) = token.strip_prefix("projectzomboid.") {
                lookup_projectzomboid_launch_token(context, path)
            } else if let Some(path) = token.strip_prefix("rust.") {
                lookup_rust_launch_token(context, path)
            } else if let Some(path) = token.strip_prefix("palworld.") {
                lookup_palworld_launch_token(context, path)
            } else if let Some(path) = token.strip_prefix("astroneer.") {
                lookup_extra_launch_args_token(context, path)
            } else if let Some(path) = token.strip_prefix("nightingale.") {
                lookup_nightingale_launch_token(context, path)
            } else if let Some(path) = token.strip_prefix("romestead.") {
                lookup_extra_launch_args_token(context, path)
            } else if let Some(path) = token.strip_prefix("runescapedragonwilds.") {
                lookup_extra_launch_args_token(context, path)
            } else if let Some(path) = token.strip_prefix("satisfactory.") {
                lookup_satisfactory_launch_token(context, path)
            } else if let Some(path) = token.strip_prefix("unturned.") {
                lookup_unturned_launch_token(context, path)
            } else if let Some(path) = token.strip_prefix("valheim.") {
                lookup_valheim_launch_token(context, path)
            } else if let Some(path) = token.strip_prefix("vrising.") {
                lookup_vrising_launch_token(context, path)
            } else if let Some(path) = token.strip_prefix("ports.") {
                lookup_port_path(&context.instance.ports, path)
            } else {
                None
            }
        }
    }
}

pub(super) fn lookup_dontstarve_launch_token(
    context: &TemplateContext<'_>,
    path: &str,
) -> Option<String> {
    match path {
        "launch_args" => Some(render_dontstarve_launch_args(context.settings).join("\n")),
        _ => None,
    }
}

pub(super) fn render_dontstarve_launch_args(settings: &Value) -> Vec<String> {
    let mut args = Vec::new();
    if lookup_json_bool(settings, "disable_data_collection").unwrap_or(false) {
        args.push(String::from("-disabledatacollection"));
    }

    args.push(String::from("-backup_log_count"));
    args.push(
        lookup_json_value(settings, "backup_log_count")
            .and_then(Value::as_i64)
            .unwrap_or(100)
            .to_string(),
    );
    args.push(String::from("-backup_log_period"));
    args.push(
        lookup_json_value(settings, "backup_log_period")
            .and_then(Value::as_i64)
            .unwrap_or(86_400)
            .to_string(),
    );

    if lookup_json_bool(settings, "friends_only").unwrap_or(false) {
        args.push(String::from("-fo"));
    }
    if lookup_json_bool(settings, "allow_ioopenwrite_sandbox_escape").unwrap_or(false) {
        args.push(String::from("-allow_ioopenwrite_sandbox_escape"));
    }
    args
}

pub(super) fn collect_dontstarve_launch_setting_issues(
    settings: &Value,
    ports: &[PortBinding],
) -> Vec<LaunchValidationIssue> {
    const LAN_DISCOVERY_PORT_MIN: u16 = 10_998;
    const LAN_DISCOVERY_PORT_MAX: u16 = 11_018;

    let offline = lookup_json_bool(settings, "offline_cluster").unwrap_or(false);
    let lan_only = lookup_json_bool(settings, "lan_only_cluster").unwrap_or(false);
    let mut issues = Vec::new();

    if !offline
        && lookup_json_value(settings, "cluster_token")
            .and_then(Value::as_str)
            .is_none_or(|token| token.trim().is_empty())
    {
        issues.push(LaunchValidationIssue {
            code: String::from("dst_cluster_token_missing"),
            context: BTreeMap::from([(
                String::from("field"),
                String::from("cluster_token"),
            )]),
            severity: String::from("error"),
            message: String::from(
                "Online Don't Starve Together servers require a Klei cluster token. Add the token under Room and access, or enable offline mode before starting.",
            ),
            path: None,
        });
    }

    if !offline && lookup_json_bool(settings, "disable_data_collection").unwrap_or(false) {
        issues.push(LaunchValidationIssue {
            code: String::from("dst_data_collection_requires_offline"),
            context: BTreeMap::from([(
                String::from("field"),
                String::from("disable_data_collection"),
            )]),
            severity: String::from("error"),
            message: String::from(
                "Klei only supports disabling data collection in offline mode. Enable offline mode or turn data collection back on before starting.",
            ),
            path: None,
        });
    }

    let shards = match app_core::dst_shards::dst_shards(settings) {
        Ok(shards) => shards,
        Err(message) => {
            issues.push(LaunchValidationIssue {
                code: String::from("dst_shard_layout_invalid"),
                context: BTreeMap::from([(String::from("field"), String::from("shard_layout"))]),
                severity: String::from("error"),
                message,
                path: None,
            });
            return issues;
        }
    };
    if offline || lan_only {
        for binding in ports
            .iter()
            .filter(|binding| shards.iter().any(|shard| shard.game_port == binding.name))
        {
            if !(LAN_DISCOVERY_PORT_MIN..=LAN_DISCOVERY_PORT_MAX).contains(&binding.port) {
                issues.push(LaunchValidationIssue {
                    code: String::from("dst_lan_port_out_of_range"),
                    context: BTreeMap::from([
                        (String::from("port_name"), binding.name.clone()),
                        (String::from("port"), binding.port.to_string()),
                        (
                            String::from("allowed_range"),
                            format!("{LAN_DISCOVERY_PORT_MIN}-{LAN_DISCOVERY_PORT_MAX}"),
                        ),
                    ]),
                    severity: String::from("error"),
                    message: format!(
                        "DST {} port {} is outside Klei's 10998-11018 LAN discovery range. Choose an unused port in that range before starting this LAN/offline server.",
                        binding.name, binding.port
                    ),
                    path: None,
                });
            }
        }
    }

    issues
}

#[cfg(test)]
#[path = "dst_shard_launch_tests.rs"]
mod dst_shard_tests;

pub(super) fn lookup_json_path(value: &Value, path: &str) -> Option<String> {
    let current = lookup_json_value(value, path)?;

    match current {
        Value::Null => Some(String::new()),
        Value::Bool(boolean) => Some(boolean.to_string()),
        Value::Number(number) => Some(number.to_string()),
        Value::String(text) => Some(text.clone()),
        other => Some(other.to_string()),
    }
}

pub(super) fn lookup_json_value<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = value;

    for segment in path.split('.') {
        current = current.get(segment)?;
    }

    Some(current)
}

pub(super) fn lookup_json_bool(value: &Value, path: &str) -> Option<bool> {
    match lookup_json_value(value, path)? {
        Value::Bool(boolean) => Some(*boolean),
        Value::Number(number) => number.as_i64().map(|candidate| candidate != 0),
        Value::String(text) => {
            let trimmed = text.trim();
            if trimmed.eq_ignore_ascii_case("true")
                || trimmed.eq_ignore_ascii_case("1")
                || trimmed.eq_ignore_ascii_case("yes")
                || trimmed.eq_ignore_ascii_case("on")
            {
                Some(true)
            } else if trimmed.eq_ignore_ascii_case("false")
                || trimmed.eq_ignore_ascii_case("0")
                || trimmed.eq_ignore_ascii_case("no")
                || trimmed.eq_ignore_ascii_case("off")
            {
                Some(false)
            } else {
                None
            }
        }
        _ => None,
    }
}

pub(super) fn lookup_json_text(value: &Value, path: &str) -> Option<String> {
    lookup_json_path(value, path)
}

pub(super) fn lookup_terraria_launch_token(
    context: &TemplateContext<'_>,
    path: &str,
) -> Option<String> {
    match path {
        "server_executable" => Some(render_terraria_server_executable(context)),
        "working_directory" => Some(render_terraria_working_directory(context)),
        "launch_args" => Some(render_terraria_launch_args(context).join("\n")),
        _ => None,
    }
}

fn terraria_uses_tmodloader(context: &TemplateContext<'_>) -> bool {
    lookup_json_text(context.settings, "server_runtime")
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("tmodloader"))
}

fn terraria_tmodloader_root(context: &TemplateContext<'_>) -> PathBuf {
    context
        .config_dir
        .parent()
        .unwrap_or(context.config_dir)
        .join("tmodloader")
}

fn terraria_tmodloader_runtime_dir(context: &TemplateContext<'_>) -> PathBuf {
    if let Some(value) = lookup_json_text(context.settings, "tmodloader_runtime_dir") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            let configured = PathBuf::from(trimmed);
            return if configured.is_absolute() {
                configured
            } else {
                terraria_tmodloader_root(context).join(configured)
            };
        }
    }
    terraria_tmodloader_root(context).join("runtime")
}

fn render_terraria_server_executable(context: &TemplateContext<'_>) -> String {
    if terraria_uses_tmodloader(context) {
        return terraria_tmodloader_runtime_dir(context)
            .join("start-tModLoaderServer.bat")
            .to_string_lossy()
            .into_owned();
    }
    String::from("TerrariaServer.exe")
}

fn render_terraria_working_directory(context: &TemplateContext<'_>) -> String {
    if terraria_uses_tmodloader(context) {
        return terraria_tmodloader_runtime_dir(context)
            .to_string_lossy()
            .into_owned();
    }
    context
        .install_root
        .join("1458")
        .join("Windows")
        .to_string_lossy()
        .into_owned()
}

fn render_terraria_launch_args(context: &TemplateContext<'_>) -> Vec<String> {
    let mut args = Vec::new();
    if terraria_uses_tmodloader(context) {
        args.push(String::from("-nosteam"));
    }
    args.push(String::from("-config"));
    args.push(
        context
            .config_dir
            .join("serverconfig.txt")
            .to_string_lossy()
            .into_owned(),
    );
    if terraria_uses_tmodloader(context) {
        args.push(String::from("-tmlsavedirectory"));
        args.push(
            terraria_tmodloader_root(context)
                .to_string_lossy()
                .into_owned(),
        );
    }
    args.push(String::from("-ip"));
    args.push(context.instance.summary.bind_ip.clone());
    args
}

pub(super) fn lookup_abioticfactor_launch_token(
    context: &TemplateContext<'_>,
    path: &str,
) -> Option<String> {
    match path {
        "sandbox_ini_flag" => Some(format!(
            "-SandboxIniPath=Config/WindowsServer/LanGame/{}-SandboxSettings.ini",
            context.instance.summary.id
        )),
        "admin_ini_flag" => Some(format!(
            "-AdminIniPath=SaveGames/Server/LanGame/{}-Admin.ini",
            context.instance.summary.id
        )),
        "server_password_flag" => Some(render_launch_flag_with_value(
            context.settings,
            "server_password",
            "-ServerPassword=",
        )),
        "admin_password_flag" => Some(render_launch_flag_with_value(
            context.settings,
            "admin_password",
            "-AdminPassword=",
        )),
        "lan_only_flag" => Some(render_optional_launch_flag(
            context.settings,
            "lan_only",
            "-LANOnly",
        )),
        "platform_limited_flag" => Some(render_abioticfactor_platform_limited_flag(context)),
        "multihome_flag" => Some(render_bind_ip_flag(context, "-MultiHome=")),
        "use_local_ips_flag" => Some(render_optional_launch_flag(
            context.settings,
            "use_local_ips",
            "-UseLocalIPs",
        )),
        "use_perf_threads_flag" => Some(render_optional_launch_flag(
            context.settings,
            "use_perf_threads",
            "-useperfthreads",
        )),
        "no_async_loading_thread_flag" => Some(render_optional_launch_flag(
            context.settings,
            "disable_async_loading_thread",
            "-DisableAsyncLoadingThread",
        )),
        _ => None,
    }
}

pub(super) fn lookup_conan_launch_token(
    context: &TemplateContext<'_>,
    path: &str,
) -> Option<String> {
    match path {
        "multihome_flag" => Some(render_bind_ip_flag(context, "-MULTIHOME=")),
        "custom_launch_flags" => Some(render_split_launch_flags(
            context.settings,
            "custom_launch_flags",
        )),
        _ => None,
    }
}

pub(super) fn lookup_soulmask_launch_token(
    context: &TemplateContext<'_>,
    path: &str,
) -> Option<String> {
    match path {
        "workshop_mods_arg" => Some(render_soulmask_workshop_mods_arg(context.settings)),
        _ => None,
    }
}

pub(super) fn render_soulmask_workshop_mods_arg(settings: &Value) -> String {
    let ids = parse_steam_workshop_ids_from_setting(settings, "mod_workshop_ids");
    if ids.is_empty() {
        return String::new();
    }

    format!(r#"-mod="{}""#, ids.join(","))
}

fn parse_steam_workshop_ids_from_setting(settings: &Value, key: &str) -> Vec<String> {
    let Some(raw_value) = lookup_json_text(settings, key) else {
        return Vec::new();
    };

    let mut seen = std::collections::HashSet::new();
    let mut ids = Vec::new();
    for candidate in raw_value
        .split(|character: char| character.is_ascii_whitespace() || matches!(character, ',' | ';'))
        .map(str::trim)
        .filter(|candidate| !candidate.is_empty())
    {
        if let Some(id) = extract_steam_workshop_item_id(candidate)
            && seen.insert(id.clone())
        {
            ids.push(id);
        }
    }
    ids
}

fn extract_steam_workshop_item_id(reference: &str) -> Option<String> {
    let trimmed = reference.trim_matches(|character: char| {
        character.is_ascii_whitespace() || matches!(character, '"' | '\'' | ',' | ';')
    });
    if trimmed.is_empty() {
        return None;
    }

    if let Some(id) = extract_steam_workshop_query_id(trimmed) {
        return Some(id);
    }

    if is_steam_workshop_item_id(trimmed) {
        return Some(trimmed.to_string());
    }

    let lowercase = trimmed.to_ascii_lowercase();
    if !lowercase.contains("workshop")
        && !lowercase.contains("sharedfiles")
        && !lowercase.contains("filedetails")
    {
        return None;
    }

    extract_last_digit_run(trimmed).filter(|id| is_steam_workshop_item_id(id))
}

fn extract_steam_workshop_query_id(value: &str) -> Option<String> {
    let lowercase = value.to_ascii_lowercase();
    for marker in ["id=", "id%3d"] {
        if let Some(index) = lowercase.find(marker) {
            let start = index + marker.len();
            let id = value[start..]
                .chars()
                .take_while(|character| character.is_ascii_digit())
                .collect::<String>();
            if is_steam_workshop_item_id(&id) {
                return Some(id);
            }
        }
    }
    None
}

fn is_steam_workshop_item_id(value: &str) -> bool {
    value.len() >= 5 && value.chars().all(|character| character.is_ascii_digit())
}

fn extract_last_digit_run(value: &str) -> Option<String> {
    let mut current = String::new();
    let mut last = None;
    for character in value.chars() {
        if character.is_ascii_digit() {
            current.push(character);
            continue;
        }
        if is_steam_workshop_item_id(&current) {
            last = Some(std::mem::take(&mut current));
        } else {
            current.clear();
        }
    }
    if is_steam_workshop_item_id(&current) {
        Some(current)
    } else {
        last
    }
}

pub(super) fn lookup_corekeeper_launch_token(
    context: &TemplateContext<'_>,
    path: &str,
) -> Option<String> {
    match path {
        "log_path" => Some(
            context
                .logs_dir
                .join("CoreKeeperServer.log")
                .to_string_lossy()
                .into_owned(),
        ),
        "direct_ip_flag" => Some(render_corekeeper_direct_flag(context, "-ip")),
        "direct_ip_value" => Some(render_corekeeper_direct_value(
            context,
            context.instance.summary.bind_ip.clone(),
        )),
        "direct_port_flag" => Some(render_corekeeper_direct_flag(context, "-port")),
        "direct_port_value" => Some(render_corekeeper_direct_value(
            context,
            lookup_port_path(&context.instance.ports, "game.port").unwrap_or_default(),
        )),
        "direct_password_flag" => Some(render_corekeeper_direct_flag(context, "-password")),
        "direct_password_value" => Some(render_corekeeper_direct_value(
            context,
            lookup_json_text(context.settings, "join_password").unwrap_or_default(),
        )),
        "direct_allowed_platform_flag" => Some(render_corekeeper_allowed_platform_flag(context)),
        "direct_allowed_platform_value" => Some(render_corekeeper_allowed_platform_value(context)),
        _ => None,
    }
}

pub(super) fn lookup_necesse_launch_token(
    context: &TemplateContext<'_>,
    path: &str,
) -> Option<String> {
    match path {
        "owner_args" => Some(render_launch_arg_pair_with_value(
            context.settings,
            "owner_name",
            "-owner",
        )),
        "pause_when_empty_value" => Some(render_boolean_launch_value(
            context.settings,
            "pause_when_empty",
            true,
        )),
        "strict_server_authority_value" => Some(render_boolean_launch_value(
            context.settings,
            "strict_server_authority",
            true,
        )),
        "logging_enabled_value" => Some(render_boolean_launch_value(
            context.settings,
            "logging_enabled",
            true,
        )),
        "zip_saves_value" => Some(render_boolean_launch_value(
            context.settings,
            "zip_saves",
            true,
        )),
        "ignore_seasons_flag" => Some(render_optional_launch_flag(
            context.settings,
            "ignore_seasons",
            "-ignoreseasons",
        )),
        "custom_launch_flags" => Some(render_split_launch_flags(
            context.settings,
            "custom_launch_flags",
        )),
        _ => None,
    }
}

pub(super) fn lookup_projectzomboid_launch_token(
    context: &TemplateContext<'_>,
    path: &str,
) -> Option<String> {
    match path {
        "classpath" => Some(render_projectzomboid_classpath(context.install_root)),
        _ => None,
    }
}

pub(super) fn lookup_rust_launch_token(
    context: &TemplateContext<'_>,
    path: &str,
) -> Option<String> {
    match path {
        "insecure_flag" => Some(
            if context.settings.get("secure").and_then(Value::as_bool) == Some(false) {
                "-insecure".to_owned()
            } else {
                String::new()
            },
        ),
        "world_configfile_args" => Some(render_rust_world_configfile_args(context.settings)),
        "use_new_navmesh_flag" => Some(render_optional_launch_flag(
            context.settings,
            "use_new_navmesh",
            "-useNewNavmesh",
        )),
        "custom_launch_flags" => Some(render_split_launch_flags(
            context.settings,
            "custom_launch_flags",
        )),
        _ => None,
    }
}

pub(super) fn lookup_palworld_launch_token(
    context: &TemplateContext<'_>,
    path: &str,
) -> Option<String> {
    match path {
        "gamedata_api_flag" => Some(render_optional_launch_flag(
            context.settings,
            "gamedata_api_enabled",
            "-enable-gamedata-api",
        )),
        "public_lobby_flag" => Some(render_optional_launch_flag(
            context.settings,
            "community_server",
            "-publiclobby",
        )),
        "public_ip_flag" => Some(render_palworld_public_ip_flag(context.settings)),
        "public_port_flag" => Some(render_palworld_public_port_flag(context.settings)),
        "use_perf_threads_flag" => Some(render_optional_launch_flag(
            context.settings,
            "launch_perf_threads",
            "-useperfthreads",
        )),
        "no_async_loading_thread_flag" => Some(render_optional_launch_flag(
            context.settings,
            "launch_perf_threads",
            "-NoAsyncLoadingThread",
        )),
        "use_multithread_for_ds_flag" => Some(render_optional_launch_flag(
            context.settings,
            "launch_perf_threads",
            "-UseMultithreadForDS",
        )),
        "worker_thread_count_flag" => Some(render_palworld_worker_thread_flag(context.settings)),
        _ => None,
    }
}

pub(super) fn lookup_satisfactory_launch_token(
    context: &TemplateContext<'_>,
    path: &str,
) -> Option<String> {
    match path {
        "external_reliable_port_flag" => {
            let port = lookup_json_value(context.settings, "external_reliable_port")
                .and_then(Value::as_u64)
                .unwrap_or_default();
            Some(if port > 0 {
                format!("-ExternalReliablePort={port}")
            } else {
                String::new()
            })
        }
        "disable_packet_routing_flag" => Some(render_optional_launch_flag(
            context.settings,
            "disable_packet_routing",
            "-DisablePacketRouting",
        )),
        "disable_seasonal_events_flag" => Some(render_optional_launch_flag(
            context.settings,
            "disable_seasonal_events",
            "-DisableSeasonalEvents",
        )),
        "insecure_local_api_flag" => {
            if lookup_json_bool(context.settings, "allow_insecure_local_api").unwrap_or(false) {
                Some(String::from(
                    "-ini:Engine:[SystemSettings]:FG.DedicatedServer.AllowInsecureLocalAccess=1",
                ))
            } else {
                Some(String::new())
            }
        }
        "custom_launch_flags" => Some(render_split_launch_flags(
            context.settings,
            "custom_launch_flags",
        )),
        _ => None,
    }
}

pub(super) fn lookup_nightingale_launch_token(
    context: &TemplateContext<'_>,
    path: &str,
) -> Option<String> {
    match path {
        "enable_cheats_flag" => Some(render_optional_launch_flag(
            context.settings,
            "enable_cheats",
            "-EnableCheats",
        )),
        "status_endpoint_args" => {
            if !lookup_json_bool(context.settings, "status_endpoint_enabled").unwrap_or(false) {
                return Some(String::new());
            }
            let port = lookup_port_path(&context.instance.ports, "status.port")?;
            Some(format!(
                "-statusPort={port}\n-ini:Engine:[HTTPServer.Listeners]:+ListenerOverrides=(Port={port},BindAddress={})",
                context.instance.summary.bind_ip
            ))
        }
        "json_logging_args" => {
            if !lookup_json_bool(context.settings, "json_logging").unwrap_or(false) {
                return Some(String::new());
            }
            Some(String::from(
                "-ini:Engine:[JsonLogger]:bEnable=true\n-ini:Engine:[JsonLogger]:bStdout=true\n-noconsole",
            ))
        }
        "extra_launch_args" => Some(render_split_launch_flags(
            context.settings,
            "extra_launch_args",
        )),
        _ => None,
    }
}

fn lookup_extra_launch_args_token(context: &TemplateContext<'_>, path: &str) -> Option<String> {
    match path {
        "extra_args" | "extra_launch_args" => Some(render_split_launch_flags(
            context.settings,
            "extra_launch_args",
        )),
        _ => None,
    }
}

pub(super) fn lookup_unturned_launch_token(
    context: &TemplateContext<'_>,
    path: &str,
) -> Option<String> {
    match path {
        "server_launch_mode" => {
            let mode = if lookup_json_bool(context.settings, "internet_server").unwrap_or(true) {
                "InternetServer"
            } else {
                "LanServer"
            };
            Some(format!("+{mode}/{}", context.instance.summary.id))
        }
        "no_level_config_overrides_flag" => Some(render_optional_launch_flag(
            context.settings,
            "no_level_config_overrides",
            "-NoLevelConfigOverrides",
        )),
        "log_gameplay_config_flag" => Some(render_optional_launch_flag(
            context.settings,
            "log_gameplay_config",
            "-LogGameplayConfig",
        )),
        "gameplay_config_no_generated_comments_flag" => Some(render_optional_launch_flag(
            context.settings,
            "gameplay_config_no_generated_comments",
            "-GameplayConfigNoGeneratedComments",
        )),
        "gameplay_config_no_empty_values_flag" => Some(render_optional_launch_flag(
            context.settings,
            "gameplay_config_no_empty_values",
            "-GameplayConfigNoEmptyValues",
        )),
        "custom_launch_flags" => Some(render_split_launch_flags(
            context.settings,
            "custom_launch_flags",
        )),
        _ => None,
    }
}

pub(super) fn lookup_valheim_launch_token(
    context: &TemplateContext<'_>,
    path: &str,
) -> Option<String> {
    match path {
        "world_preset_args" => Some(render_launch_arg_pair_with_value(
            context.settings,
            "world_preset",
            "-preset",
        )),
        "world_modifier_args" => Some(render_valheim_world_modifier_args(context.settings)),
        "world_setkey_args" => Some(render_repeated_launch_arg_pairs(
            context.settings,
            "world_set_keys",
            "-setkey",
        )),
        "crossplay_flag" => Some(render_optional_launch_flag(
            context.settings,
            "crossplay_enabled",
            "-crossplay",
        )),
        "instance_id_args" => Some(render_launch_arg_pair_with_value(
            context.settings,
            "instance_id",
            "-instanceid",
        )),
        "log_file_args" => Some(render_launch_arg_pair_with_value(
            context.settings,
            "log_file",
            "-logFile",
        )),
        "custom_launch_flags" => Some(render_split_launch_flags(
            context.settings,
            "custom_launch_flags",
        )),
        _ => None,
    }
}

pub(super) fn lookup_vrising_launch_token(
    context: &TemplateContext<'_>,
    path: &str,
) -> Option<String> {
    match path {
        "bind_address_flag" => Some(render_vrising_bind_token(context, "-bindAddress")),
        "bind_address_value" => Some(render_vrising_bind_value(context)),
        _ => None,
    }
}

pub(super) fn render_optional_launch_flag(settings: &Value, path: &str, flag: &str) -> String {
    if lookup_json_bool(settings, path).unwrap_or(false) {
        String::from(flag)
    } else {
        String::new()
    }
}

pub(super) fn render_launch_flag_with_value(settings: &Value, path: &str, prefix: &str) -> String {
    let Some(value) = lookup_json_text(settings, path) else {
        return String::new();
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("{prefix}{trimmed}")
    }
}

pub(super) fn render_launch_arg_pair_with_value(
    settings: &Value,
    path: &str,
    flag: &str,
) -> String {
    let Some(value) = lookup_json_text(settings, path) else {
        return String::new();
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("{flag}\n{trimmed}")
    }
}

pub(super) fn render_bind_ip_flag(context: &TemplateContext<'_>, prefix: &str) -> String {
    let bind_ip = context.instance.summary.bind_ip.trim();
    if bind_ip.is_empty() || bind_ip == "0.0.0.0" {
        String::new()
    } else {
        format!("{prefix}{bind_ip}")
    }
}

pub(super) fn render_bind_ip_presence_flag(context: &TemplateContext<'_>, flag: &str) -> String {
    let bind_ip = context.instance.summary.bind_ip.trim();
    if bind_ip.is_empty() || bind_ip == "0.0.0.0" {
        String::new()
    } else {
        String::from(flag)
    }
}

pub(super) fn render_boolean_launch_value(
    settings: &Value,
    path: &str,
    true_is_one: bool,
) -> String {
    let enabled = lookup_json_bool(settings, path).unwrap_or(false);
    match (enabled, true_is_one) {
        (true, true) | (false, false) => String::from("1"),
        _ => String::from("0"),
    }
}

pub(super) fn render_projectzomboid_classpath(install_root: &Path) -> String {
    let java_root = install_root.join("java");
    let mut entries = fs::read_dir(&java_root)
        .ok()
        .into_iter()
        .flat_map(|iter| iter.filter_map(Result::ok))
        .filter_map(|entry| {
            let file_type = entry.file_type().ok()?;
            if !file_type.is_file() {
                return None;
            }

            let file_name = entry.file_name();
            let file_name = file_name.to_string_lossy();
            if !file_name.to_ascii_lowercase().ends_with(".jar") {
                return None;
            }

            Some(format!("java/{file_name}"))
        })
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.to_ascii_lowercase());
    entries.push(String::from("java/"));
    entries.join(";")
}

pub(super) fn render_abioticfactor_platform_limited_flag(context: &TemplateContext<'_>) -> String {
    let Some(value) = lookup_json_text(context.settings, "platform_limited") else {
        return String::new();
    };
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("all") {
        String::new()
    } else {
        format!("-PlatformLimited={trimmed}")
    }
}

pub(super) fn parse_ark_ascended_mod_ids(raw: &str) -> Vec<String> {
    let mut mod_ids = Vec::new();
    let mut seen = HashSet::new();

    for token in raw.split(is_ark_ascended_mod_id_separator) {
        for candidate in extract_ark_ascended_mod_id_candidates(token) {
            if seen.insert(candidate.clone()) {
                mod_ids.push(candidate);
            }
        }
    }

    mod_ids
}

pub(super) fn is_ark_ascended_mod_id_separator(ch: char) -> bool {
    matches!(
        ch,
        ',' | ';' | '\n' | '\r' | '\t' | ' ' | '\u{3001}' | '\u{ff0c}' | '\u{ff1b}'
    )
}

pub(super) fn extract_ark_ascended_mod_id_candidates(token: &str) -> Vec<String> {
    let token = trim_mod_id_candidate(token);
    if token.is_empty() {
        return Vec::new();
    }

    let mut candidates = Vec::new();
    push_numeric_mod_id(token, &mut candidates);

    let lowercase = token.to_ascii_lowercase();
    for prefix in [
        "cf-",
        "cf:",
        "curseforge-",
        "curseforge:",
        "project-",
        "project:",
        "projectid=",
        "project_id=",
        "mod-",
        "mod:",
        "modid=",
        "mod_id=",
        "id=",
    ] {
        if lowercase.starts_with(prefix) {
            push_numeric_mod_id(&token[prefix.len()..], &mut candidates);
        }
    }

    if let Some(query) = token.split_once('?').map(|(_, query)| query) {
        for pair in query.split('&') {
            push_supported_mod_id_key_value(pair, &mut candidates);
        }
    } else {
        push_supported_mod_id_key_value(token, &mut candidates);
    }

    let path = token.split_once('?').map_or(token, |(path, _)| path);
    let mut previous_was_mod_container = false;
    for segment in path.split(['/', '\\']) {
        let segment = trim_mod_id_candidate(segment);
        if previous_was_mod_container {
            push_numeric_mod_id(segment, &mut candidates);
        }
        previous_was_mod_container = is_ark_ascended_mod_container_segment(segment);
    }

    candidates
}

pub(super) fn push_supported_mod_id_key_value(pair: &str, candidates: &mut Vec<String>) {
    let Some((key, value)) = pair.split_once('=') else {
        return;
    };
    if is_supported_ark_ascended_mod_id_key(key) {
        push_numeric_mod_id(value, candidates);
    }
}

pub(super) fn is_supported_ark_ascended_mod_id_key(key: &str) -> bool {
    let normalized = key
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    matches!(normalized.as_str(), "id" | "modid" | "projectid")
}

pub(super) fn is_ark_ascended_mod_container_segment(segment: &str) -> bool {
    matches!(
        segment.to_ascii_lowercase().as_str(),
        "mod" | "mods" | "project" | "projects"
    )
}

pub(super) fn push_numeric_mod_id(candidate: &str, candidates: &mut Vec<String>) {
    let candidate = trim_mod_id_candidate(candidate);
    if !candidate.is_empty() && candidate.chars().all(|ch| ch.is_ascii_digit()) {
        candidates.push(String::from(candidate));
    }
}

pub(super) fn trim_mod_id_candidate(candidate: &str) -> &str {
    candidate.trim().trim_matches(|ch| {
        matches!(
            ch,
            '"' | '\'' | '`' | '[' | ']' | '(' | ')' | '<' | '>' | '{' | '}' | '.'
        )
    })
}

pub(super) fn render_corekeeper_direct_flag(context: &TemplateContext<'_>, flag: &str) -> String {
    if corekeeper_direct_connection_enabled(context.settings) {
        String::from(flag)
    } else {
        String::new()
    }
}

pub(super) fn render_corekeeper_direct_value(
    context: &TemplateContext<'_>,
    value: String,
) -> String {
    if corekeeper_direct_connection_enabled(context.settings) {
        value.trim().to_string()
    } else {
        String::new()
    }
}

pub(super) fn lookup_corekeeper_allowed_platform_name(settings: &Value) -> Option<&'static str> {
    let raw = lookup_json_text(settings, "allowed_platform_code")?;
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "steam" => Some("Steam"),
        "2" | "epic" => Some("Epic"),
        "3" | "microsoft" | "microsoft store" | "microsoftstore" => Some("Microsoft"),
        "4" | "gog" => Some("GOG"),
        _ => None,
    }
}

pub(super) fn render_corekeeper_allowed_platform_flag(context: &TemplateContext<'_>) -> String {
    if corekeeper_direct_connection_enabled(context.settings)
        && lookup_corekeeper_allowed_platform_name(context.settings).is_some()
    {
        String::from("-allowonlyplatform")
    } else {
        String::new()
    }
}

pub(super) fn render_corekeeper_allowed_platform_value(context: &TemplateContext<'_>) -> String {
    if corekeeper_direct_connection_enabled(context.settings) {
        lookup_corekeeper_allowed_platform_name(context.settings)
            .unwrap_or_default()
            .to_string()
    } else {
        String::new()
    }
}

pub(super) fn corekeeper_direct_connection_enabled(settings: &Value) -> bool {
    lookup_json_bool(settings, "direct_connection_enabled").unwrap_or(true)
}

pub(super) fn render_vrising_bind_token(context: &TemplateContext<'_>, flag: &str) -> String {
    let bind_ip = context.instance.summary.bind_ip.trim();
    if bind_ip.is_empty() || bind_ip == "0.0.0.0" {
        String::new()
    } else {
        String::from(flag)
    }
}

pub(super) fn render_vrising_bind_value(context: &TemplateContext<'_>) -> String {
    let bind_ip = context.instance.summary.bind_ip.trim();
    if bind_ip.is_empty() || bind_ip == "0.0.0.0" {
        String::new()
    } else {
        bind_ip.to_string()
    }
}

pub(super) fn render_split_launch_flags(settings: &Value, path: &str) -> String {
    let Some(value) = lookup_json_text(settings, path) else {
        return String::new();
    };
    parse_custom_launch_flags(&value).join("\n")
}

pub(super) fn render_repeated_launch_arg_pairs(settings: &Value, path: &str, flag: &str) -> String {
    let Some(value) = lookup_json_text(settings, path) else {
        return String::new();
    };

    let entries = value
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .split(['\n', ',', ';'])
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(String::from)
        .collect::<Vec<_>>();

    if entries.is_empty() {
        return String::new();
    }

    let mut parts = Vec::with_capacity(entries.len() * 2);
    for entry in entries {
        parts.push(String::from(flag));
        parts.push(entry);
    }

    parts.join("\n")
}

pub(super) fn render_valheim_world_modifier_args(settings: &Value) -> String {
    let Some(value) = lookup_json_text(settings, "world_modifiers") else {
        return String::new();
    };
    // Valheim consumes the modifier name and value as separate argv entries.
    // The schema validates the supported pairs before settings are saved or launched.
    value
        .split(['\r', '\n', ',', ';'])
        .filter(|entry| !entry.trim().is_empty())
        .flat_map(|entry| std::iter::once("-modifier").chain(entry.split_whitespace()))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn render_rust_world_configfile_args(settings: &Value) -> String {
    let Some(value) = lookup_json_text(settings, "world_config_json") else {
        return String::new();
    };
    if value.trim().is_empty() {
        String::new()
    } else {
        String::from("+world.configfile\nworld-config.json")
    }
}

pub(super) fn parse_custom_launch_flags(raw: &str) -> Vec<String> {
    let mut flags = Vec::new();
    let mut current = String::new();
    let mut in_single_quotes = false;
    let mut in_double_quotes = false;
    let mut chars = raw.chars().peekable();

    while let Some(character) = chars.next() {
        match character {
            '"' if !in_single_quotes => {
                in_double_quotes = !in_double_quotes;
            }
            '\'' if !in_double_quotes => {
                in_single_quotes = !in_single_quotes;
            }
            '\\' if in_double_quotes => {
                if let Some(next) = chars.peek().copied() {
                    if next == '"' || next == '\\' {
                        current.push(next);
                        chars.next();
                    } else {
                        current.push(character);
                    }
                } else {
                    current.push(character);
                }
            }
            character if character.is_whitespace() && !in_single_quotes && !in_double_quotes => {
                if !current.is_empty() {
                    flags.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(character),
        }
    }

    if !current.is_empty() {
        flags.push(current);
    }

    flags
}

pub(super) fn custom_launch_flags_contain_managed_option(
    raw: &str,
    managed_options: &[&str],
) -> bool {
    parse_custom_launch_flags(raw).iter().any(|argument| {
        let option = argument
            .split_once('=')
            .map_or(argument.as_str(), |(option, _)| option);
        managed_options
            .iter()
            .any(|managed| option.eq_ignore_ascii_case(managed))
    })
}

pub(super) fn render_palworld_public_ip_flag(settings: &Value) -> String {
    if !lookup_json_bool(settings, "community_server").unwrap_or(false) {
        return String::new();
    }

    let Some(value) = lookup_json_text(settings, "public_ip") else {
        return String::new();
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("-publicip={trimmed}")
    }
}

pub(super) fn render_palworld_public_port_flag(settings: &Value) -> String {
    if !lookup_json_bool(settings, "community_server").unwrap_or(false) {
        return String::new();
    }

    let Some(value) = lookup_json_text(settings, "public_port") else {
        return String::new();
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("-publicport={trimmed}")
    }
}

pub(super) fn render_palworld_worker_thread_flag(settings: &Value) -> String {
    if !lookup_json_bool(settings, "launch_perf_threads").unwrap_or(false)
        || !lookup_json_bool(settings, "launch_worker_threads_enabled").unwrap_or(false)
    {
        return String::new();
    }

    let Some(value) = lookup_json_text(settings, "worker_thread_count") else {
        return String::new();
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("-NumberOfWorkerThreadsServer={trimmed}")
    }
}

pub(super) fn lookup_port_path(ports: &[PortBinding], path: &str) -> Option<String> {
    let mut segments = path.split('.');
    let port_name = segments.next()?;
    let field = segments.next()?;
    let port = ports
        .iter()
        .find(|candidate| candidate.name == port_name)
        .or_else(|| match port_name {
            "caves" => ports.iter().find(|candidate| candidate.name == "backup"),
            "backup" => ports.iter().find(|candidate| candidate.name == "caves"),
            _ => None,
        })?;
    match field {
        "name" => Some(port.name.clone()),
        "protocol" => Some(port.protocol.clone()),
        "port" => Some(port.port.to_string()),
        _ => None,
    }
}

pub(super) fn resolve_process_executable(
    install_root: &Path,
    configured_executable: &str,
) -> PathBuf {
    let direct_path = rendered_path(configured_executable.to_owned());
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

#[cfg(windows)]
pub(super) fn compatible_java_executable_path(path: PathBuf) -> PathBuf {
    use std::path::{Component, Prefix};

    let is_java = path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.eq_ignore_ascii_case("java.exe") || name.eq_ignore_ascii_case("javaw.exe")
        });
    if !is_java
        || !matches!(path.components().next(),
        Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::VerbatimDisk(_)))
    {
        return path;
    }
    let Some(candidate) = path.to_str().and_then(|value| value.strip_prefix(r"\\?\")) else {
        return path;
    };
    // Rust adds a verbatim prefix during filesystem lookup at 248 units,
    // including the terminator. Stay below that boundary for the comparison.
    if candidate.encode_utf16().count() >= 247 {
        return path;
    }
    let candidate = PathBuf::from(candidate);
    if candidate.components().any(|component| {
        matches!(component, Component::Normal(name)
            if name.to_str().is_some_and(|value| value.ends_with(['.', ' '])))
    }) {
        return path;
    }
    // The JVM cannot locate lib/modules through a verbatim executable path.
    // Convert only when Windows resolves both spellings to the same file;
    // special names, unavailable files, and paths needing the prefix stay intact.
    match (fs::canonicalize(&path), fs::canonicalize(&candidate)) {
        (Ok(original), Ok(compatible)) if original == compatible => candidate,
        _ => path,
    }
}

pub(super) fn find_executable_by_name(search_root: &Path, file_name: &str) -> Option<PathBuf> {
    if !search_root.exists() {
        return None;
    }

    let mut matches = Vec::new();
    collect_executable_matches(search_root, file_name, &mut matches);
    matches.sort_by(|left, right| compare_executable_candidates(left, right));
    matches.into_iter().next()
}

pub(super) fn collect_executable_matches(
    search_root: &Path,
    file_name: &str,
    matches: &mut Vec<PathBuf>,
) {
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

pub(super) fn is_steam_staging_directory(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.eq_ignore_ascii_case("steamapps"))
        .unwrap_or(false)
}

pub(super) fn compare_executable_candidates(left: &Path, right: &Path) -> std::cmp::Ordering {
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

pub(super) fn executable_candidate_rank(path: &Path) -> (u8, u8, u8) {
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

pub(super) fn is_windows_component(value: &str) -> bool {
    matches!(value, "windows" | "win64" | "win32")
}

pub(super) fn is_linux_component(value: &str) -> bool {
    value == "linux" || value.starts_with("linux-")
}

pub(super) fn is_macos_component(value: &str) -> bool {
    matches!(value, "mac" | "macos" | "osx")
}

pub(super) fn is_terraria_windows_candidate(path: &Path) -> bool {
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

#[cfg(windows)]
pub(super) fn xna_framework_is_installed() -> bool {
    [
        r"C:\Windows\Microsoft.NET\assembly\GAC_MSIL\Microsoft.Xna.Framework",
        r"C:\Windows\assembly\GAC_MSIL\Microsoft.Xna.Framework",
    ]
    .iter()
    .any(|path| Path::new(path).exists())
}

#[cfg(not(windows))]
pub(super) fn xna_framework_is_installed() -> bool {
    true
}

pub(super) fn is_path_resolved_runtime(file_name: &str) -> bool {
    file_name.eq_ignore_ascii_case("java") || file_name.eq_ignore_ascii_case("java.exe")
}

pub(super) fn find_executable_on_path(file_name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .map(|directory| directory.join(file_name))
        .find(|candidate| candidate.exists())
}

pub(super) fn build_command_line(executable_path: &Path, args: &[String]) -> String {
    #[cfg(windows)]
    if is_windows_batch_script(executable_path) {
        let mut segments = Vec::with_capacity(args.len() + 3);
        segments.push(String::from("cmd.exe"));
        segments.push(String::from("/D"));
        segments.push(String::from("/C"));
        segments.push(quote_command_segment(&executable_path.to_string_lossy()));
        segments.extend(args.iter().map(|segment| quote_command_segment(segment)));
        return segments.join(" ");
    }

    let mut segments = Vec::with_capacity(args.len() + 1);
    segments.push(quote_command_segment(&executable_path.to_string_lossy()));
    segments.extend(args.iter().map(|segment| quote_command_segment(segment)));
    segments.join(" ")
}
pub(super) fn build_spawn_command(
    executable_path: &Path,
    working_directory: &Path,
    args: &[String],
) -> Result<(String, Vec<String>), std::io::Error> {
    #[cfg(windows)]
    {
        if is_windows_batch_script(executable_path) {
            let batch_entry = resolve_windows_batch_spawn_entry(executable_path, working_directory);
            validate_shell_value("batch entry", &batch_entry)?;
            for (index, argument) in args.iter().enumerate() {
                validate_shell_value(&format!("batch argument {index}"), argument)?;
            }
            return Ok((
                windows_command_interpreter()?,
                [String::from("/D"), String::from("/C")]
                    .into_iter()
                    .chain(std::iter::once(batch_entry))
                    .chain(args.iter().cloned())
                    .collect(),
            ));
        }
    }
    Ok((
        executable_path.to_string_lossy().into_owned(),
        args.to_vec(),
    ))
}

#[cfg(windows)]
pub(super) fn windows_command_interpreter() -> std::io::Result<String> {
    crate::windows_system_directory()?
        .join("cmd.exe")
        .into_os_string()
        .into_string()
        .map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Windows command interpreter path is not Unicode",
            )
        })
}
#[cfg(windows)]
pub(super) fn is_windows_batch_script(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase()),
        Some(ext) if ext == "bat" || ext == "cmd"
    )
}

#[cfg(windows)]
pub(super) fn is_script_entrypoint(executable_path: &Path, args: &[String]) -> bool {
    is_windows_batch_script(executable_path) || is_cmd_entrypoint(executable_path, args)
}

#[cfg(windows)]
pub(super) fn is_cmd_entrypoint(executable_path: &Path, args: &[String]) -> bool {
    let is_cmd = executable_path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            let name = name.to_ascii_lowercase();
            name == "cmd" || name == "cmd.exe"
        });
    if !is_cmd {
        return false;
    }

    matches!(
        args.first(),
        Some(arg) if arg.eq_ignore_ascii_case("/c") || arg.eq_ignore_ascii_case("/k")
    )
}

#[cfg(not(windows))]
pub(super) fn is_script_entrypoint(_executable_path: &Path, _args: &[String]) -> bool {
    false
}

pub(super) fn quote_command_segment(segment: &str) -> String {
    if !segment.is_empty()
        && !segment
            .chars()
            .any(|character| character.is_whitespace() || character == '"')
    {
        return segment.to_string();
    }

    let mut quoted = String::with_capacity(segment.len() + 2);
    quoted.push('"');
    let mut pending_backslashes = 0usize;
    for character in segment.chars() {
        match character {
            '\\' => pending_backslashes += 1,
            '"' => {
                quoted.extend(std::iter::repeat_n('\\', pending_backslashes * 2 + 1));
                quoted.push('"');
                pending_backslashes = 0;
            }
            _ => {
                quoted.extend(std::iter::repeat_n('\\', pending_backslashes));
                pending_backslashes = 0;
                quoted.push(character);
            }
        }
    }
    quoted.extend(std::iter::repeat_n('\\', pending_backslashes * 2));
    quoted.push('"');
    quoted
}

#[cfg(windows)]
pub(super) fn resolve_windows_batch_spawn_entry(
    executable_path: &Path,
    working_directory: &Path,
) -> String {
    let executable_parent = executable_path.parent().unwrap_or_else(|| Path::new("."));
    if executable_parent == working_directory
        && let Some(file_name) = executable_path.file_name().and_then(|name| name.to_str())
    {
        return file_name.to_string();
    }

    executable_path.to_string_lossy().into_owned()
}

fn validate_shell_value(field: &str, value: &str) -> Result<(), std::io::Error> {
    let unsafe_character = value.chars().find(|character| {
        character.is_control()
            || matches!(
                character,
                '&' | '|' | '<' | '>' | '^' | '(' | ')' | '%' | '!' | '"'
            )
    });
    let Some(unsafe_character) = unsafe_character else {
        return Ok(());
    };

    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        format!(
            "{field} contains a character unsafe for cmd.exe execution: U+{:04X}",
            u32::from(unsafe_character)
        ),
    ))
}
