use super::*;

#[path = "templates_render_ark_asa.rs"]
mod asa;
pub(super) use asa::*;
#[path = "templates_render_ark_ase.rs"]
mod ase;
pub(super) use ase::*;

#[cfg(test)]
#[path = "templates_render_ark_asa_advanced_tests.rs"]
mod asa_advanced_tests;

pub(crate) type ArkIniSetting = (&'static str, &'static str, bool, bool, bool);

pub(super) fn render_ark_native_ini_lines(
    settings: &Map<String, Value>,
    schema_defaults: &Map<String, Value>,
    definitions: &[ArkIniSetting],
) -> String {
    let mut rendered = String::new();
    for &(native_key, setting_key, repeated, indexed, quoted) in definitions {
        let value = settings
            .get(setting_key)
            .or_else(|| schema_defaults.get(setting_key));
        let Some(value) = value else {
            continue;
        };

        if repeated {
            let merged = Map::from_iter([(setting_key.to_owned(), value.clone())]);
            if indexed {
                rendered.push_str(&render_ark_indexed_lines(&merged, setting_key, native_key));
            } else {
                rendered.push_str(&render_ark_prefixed_lines(
                    &merged,
                    setting_key,
                    &format!("{native_key}="),
                ));
            }
            continue;
        }

        let text = match value {
            Value::Null => continue,
            Value::Bool(boolean) => boolean.to_string(),
            Value::Number(number) => number.to_string(),
            Value::String(text) => text.trim().to_owned(),
            other => other.to_string(),
        };
        rendered.push_str(native_key);
        rendered.push('=');
        if quoted {
            rendered.push('"');
        }
        rendered.push_str(&text);
        if quoted {
            rendered.push('"');
        }
        rendered.push('\n');
    }
    rendered
}

fn render_ark_indexed_lines(
    settings: &Map<String, Value>,
    setting_key: &str,
    native_key: &str,
) -> String {
    let Some(raw) = settings.get(setting_key).and_then(Value::as_str) else {
        return String::new();
    };
    let lines = raw
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| {
            if line.starts_with(native_key) {
                line.to_owned()
            } else if line.starts_with('[') || line.starts_with('_') {
                format!("{native_key}{line}")
            } else if let Some((index, value)) = line.split_once('=') {
                format!("{native_key}[{}]={}", index.trim(), value.trim())
            } else {
                format!("{native_key}{line}")
            }
        })
        .collect::<Vec<_>>();
    if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    }
}

pub(super) fn render_ark_mod_installer_section(
    settings: &Map<String, Value>,
    schema_defaults: &Map<String, Value>,
) -> String {
    let raw = settings
        .get("auto_managed_mod_ids")
        .or_else(|| schema_defaults.get("auto_managed_mod_ids"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if raw.lines().all(|line| line.trim().is_empty()) {
        return String::new();
    }

    let mut merged = Map::new();
    merged.insert(
        String::from("auto_managed_mod_ids"),
        Value::String(raw.to_owned()),
    );
    format!(
        "\n[ModInstaller]\n{}",
        render_ark_prefixed_lines(&merged, "auto_managed_mod_ids", "ModIDS=")
    )
}

pub(super) fn render_ark_multihome_ini_line(bind_ip: &str) -> String {
    let bind_ip = bind_ip.trim();
    if bind_ip.is_empty() || bind_ip == "0.0.0.0" {
        String::new()
    } else {
        format!("MultiHome={bind_ip}\n")
    }
}

pub(super) fn render_ark_active_mods_ini_line(
    settings: &Map<String, Value>,
    schema_defaults: &Map<String, Value>,
    setting_key: &str,
) -> String {
    let raw = settings
        .get(setting_key)
        .or_else(|| schema_defaults.get(setting_key))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let mut seen = HashSet::new();
    let ids = raw
        .split(|character: char| {
            character.is_whitespace() || matches!(character, ',' | ';' | '\u{3001}' | '\u{ff0c}')
        })
        .filter_map(normalize_workshop_id_line)
        .filter(|id| seen.insert(id.clone()))
        .collect::<Vec<_>>();
    if ids.is_empty() {
        String::new()
    } else {
        format!("ActiveMods={}\n", ids.join(","))
    }
}
