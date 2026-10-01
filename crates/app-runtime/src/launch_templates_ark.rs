use super::*;

#[path = "launch_templates_ark_asa.rs"]
mod asa;
use asa::*;
#[path = "launch_templates_ark_ase.rs"]
mod ase;
use ase::*;

#[cfg(test)]
#[path = "launch_ark_mods_tests.rs"]
mod mod_tests;
#[cfg(test)]
#[path = "launch_ark_cluster_tests.rs"]
mod tests;

pub(crate) type ArkLaunchSetting = (&'static str, &'static str, ArkLaunchSettingKind);

#[derive(Clone, Copy)]
pub(crate) enum ArkLaunchSettingKind {
    Flag,
    InvertedFlag,
    Value,
    ModIds,
}

pub(super) fn lookup_ark_evolved_launch_token(
    context: &TemplateContext<'_>,
    path: &str,
) -> Option<String> {
    match path {
        "server_url" => Some(render_ark_evolved_server_url(context)),
        "native_log_path" => Some(render_ark_native_log_path(context, "evolved")),
        "multihome_flag" => Some(render_bind_ip_presence_flag(context, "-MULTIHOME")),
        "cluster_dir_override_flag" => Some(render_ark_cluster_dir_override_flag(context)),
        "official_launch_flags" => Some(render_ark_official_launch_flags(
            context.settings,
            ARK_ASE_ADDITIONAL_LAUNCH_SETTINGS,
        )),
        "custom_launch_flags" => Some(render_split_launch_flags(
            context.settings,
            "custom_launch_flags",
        )),
        _ => None,
    }
}

pub(super) fn lookup_ark_ascended_launch_token(
    context: &TemplateContext<'_>,
    path: &str,
) -> Option<String> {
    match path {
        "server_url" => Some(render_ark_ascended_server_url(context)),
        "native_log_path" => Some(render_ark_native_log_path(context, "ascended")),
        "multihome_flag" => Some(render_bind_ip_presence_flag(context, "-MULTIHOME")),
        "cluster_dir_override_flag" => Some(render_ark_cluster_dir_override_flag(context)),
        "mod_ids_flag" => Some(render_ark_mod_list_flag(
            context.settings,
            "mod_ids_csv",
            "-mods=",
        )),
        "official_launch_flags" => Some(render_ark_official_launch_flags(
            context.settings,
            ARK_ASA_ADDITIONAL_LAUNCH_SETTINGS,
        )),
        "custom_launch_flags" => Some(render_split_launch_flags(
            context.settings,
            "custom_launch_flags",
        )),
        _ => None,
    }
}

fn render_ark_official_launch_flags(settings: &Value, definitions: &[ArkLaunchSetting]) -> String {
    definitions
        .iter()
        .filter_map(|&(flag, setting_key, kind)| match kind {
            ArkLaunchSettingKind::Flag => lookup_json_bool(settings, setting_key)
                .unwrap_or(false)
                .then(|| flag.to_owned()),
            ArkLaunchSettingKind::InvertedFlag => {
                (!lookup_json_bool(settings, setting_key).unwrap_or(true)).then(|| flag.to_owned())
            }
            ArkLaunchSettingKind::ModIds => {
                let value = render_ark_mod_list_flag(settings, setting_key, &format!("{flag}="));
                (!value.is_empty()).then_some(value)
            }
            ArkLaunchSettingKind::Value => {
                let value = lookup_json_text(settings, setting_key)?;
                if setting_key == "cluster_id"
                    && app_core::ark_cluster::resolve_cluster_directory(&value, "", Path::new(""))
                        .is_err()
                {
                    return None;
                }
                let value = value.trim();
                (!value.is_empty()).then(|| format!("{flag}={value}"))
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_ark_mod_list_flag(settings: &Value, key: &str, prefix: &str) -> String {
    let Some(raw_mod_ids) = lookup_json_text(settings, key) else {
        return String::new();
    };
    let ids = parse_ark_ascended_mod_ids(&raw_mod_ids);
    if ids.is_empty() {
        String::new()
    } else {
        format!("{prefix}{}", ids.join(","))
    }
}

fn render_ark_cluster_dir_override_flag(context: &TemplateContext<'_>) -> String {
    let primary_saves =
        app_core::ark_maps::primary_saves_dir(&context.instance.summary.id, context.saves_dir);
    match app_core::ark_cluster::resolve_cluster_directory(
        &lookup_json_text(context.settings, "cluster_id").unwrap_or_default(),
        &lookup_json_text(context.settings, "cluster_directory").unwrap_or_default(),
        &primary_saves,
    ) {
        Ok(Some(path)) => format!("-ClusterDirOverride={}", path.display()),
        // The same resolver supplies a blocking launch issue; never emit a malformed path.
        Ok(None) | Err(_) => String::new(),
    }
}

fn render_ark_native_log_path(context: &TemplateContext<'_>, edition: &str) -> String {
    lookup_json_text(context.settings, "_managed_ark_native_log")
        .map(|path| {
            compatible_native_path(rendered_path(path))
                .to_string_lossy()
                .into_owned()
        })
        .unwrap_or_else(|| {
            format!(
                "{}/ark-{edition}-server.log",
                compatible_native_path(context.logs_dir.to_owned()).display()
            )
        })
}

fn render_ark_save_directory(context: &TemplateContext<'_>) -> String {
    lookup_json_text(context.settings, "_managed_ark_save_directory")
        .unwrap_or_else(|| context.instance.summary.id.clone())
}

pub(crate) fn collect_ark_cluster_launch_issues(settings: &Value) -> Vec<LaunchValidationIssue> {
    let cluster_id = lookup_json_text(settings, "cluster_id").unwrap_or_default();
    let directory = lookup_json_text(settings, "cluster_directory").unwrap_or_default();
    let raw = lookup_json_text(settings, "custom_launch_flags").unwrap_or_default();
    let raw_arguments = parse_custom_launch_flags(&raw);
    if settings
        .get("additional_maps")
        .and_then(Value::as_array)
        .is_some_and(|maps| !maps.is_empty())
        && raw_arguments
            .iter()
            .any(|argument| ark_map_option_is_managed(argument))
    {
        return vec![ark_cluster_issue(
            "custom_launch_flags",
            "managed_launch_option_conflict",
            "Map servers share custom launch flags. Their ports, save directory, native log, session name and listen address must use the managed per-map inputs. Remove the conflicting custom option before launching the cluster.",
        )];
    }
    let mut raw_id = None;
    let mut raw_directory = None;
    let managed = !cluster_id.trim().is_empty() || !directory.trim().is_empty();
    for (index, argument) in raw_arguments.iter().enumerate() {
        let (key, value) = argument.split_once('=').map_or(
            (
                argument.as_str(),
                raw_arguments.get(index + 1).map(String::as_str),
            ),
            |(key, value)| (key, Some(value)),
        );
        let target = if key.eq_ignore_ascii_case("-clusterid") {
            &mut raw_id
        } else if key.eq_ignore_ascii_case("-ClusterDirOverride") {
            &mut raw_directory
        } else {
            continue;
        };
        if managed
            || target.is_some()
            || value.is_none_or(|value| value.trim().is_empty() || value.starts_with('-'))
        {
            return vec![ark_cluster_issue(
                "custom_launch_flags",
                "managed_launch_option_conflict",
                "Cluster options must have one source. Use the Cluster ID and shared directory fields, or clear both fields and specify each native cluster option once in custom launch flags.",
            )];
        }
        *target = value;
    }
    let resolved = app_core::ark_cluster::resolve_cluster_directory(
        raw_id.unwrap_or(&cluster_id),
        raw_directory.unwrap_or(&directory),
        Path::new(""),
    );
    match resolved {
        Ok(_) => Vec::new(),
        Err(error) => vec![ark_cluster_issue(
            if raw_id.is_some() || raw_directory.is_some() {
                "custom_launch_flags"
            } else {
                error.field
            },
            "ark_cluster_invalid",
            error.message,
        )],
    }
}

fn ark_map_option_is_managed(argument: &str) -> bool {
    let is_managed = |option: &str| {
        let key = option
            .split_once('=')
            .map_or(option, |(key, _)| key)
            .trim_start_matches(['-', '?']);
        [
            "port",
            "queryport",
            "rconport",
            "rconenabled",
            "altsavedirectoryname",
            "abslog",
            "sessionname",
            "multihome",
        ]
        .iter()
        .any(|managed| key.eq_ignore_ascii_case(managed))
    };
    if argument.starts_with('-') {
        // Inspect the option key, never arbitrary expert-option values.
        return is_managed(argument);
    }
    // Native URL options may be supplied on their own or attached to a map URL.
    argument.split('?').any(is_managed)
}

fn ark_cluster_issue(field: &str, code: &str, message: &str) -> LaunchValidationIssue {
    LaunchValidationIssue {
        code: String::from(code),
        severity: String::from("error"),
        message: String::from(message),
        context: BTreeMap::from([(String::from("field"), String::from(field))]),
        path: None,
    }
}
