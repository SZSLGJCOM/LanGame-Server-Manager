//! Per-Mod INI fragments. Paths are fixed; downloaded Mod content is never executed.
use super::*;
use serde::Deserialize;
use std::collections::BTreeMap;

const MAX_FRAGMENT_BYTES: usize = 64 * 1024;
const MAX_TOTAL_BYTES: usize = 1024 * 1024;

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ModConfiguration {
    #[serde(default)]
    game_ini: String,
    #[serde(default)]
    game_user_settings_ini: String,
}

fn read_asa_mod_configurations(
    settings: &Map<String, Value>,
) -> Result<BTreeMap<String, ModConfiguration>, String> {
    const FIELD: &str = "asa_mod_configurations";
    let Some(value) = settings.get(FIELD) else {
        return Ok(BTreeMap::new());
    };
    let configurations: BTreeMap<String, ModConfiguration> =
        serde_json::from_value(value.clone()).map_err(|error| format!("{FIELD}: {error}"))?;
    if configurations.len() > 256 {
        return Err("At most 256 Mod configurations are supported".into());
    }
    let mut total = 0;
    for (id, config) in &configurations {
        if id.starts_with('0')
            || id.parse::<u64>().is_err()
            || !id.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(format!("Invalid CurseForge Mod ID: {id}"));
        }
        for (file, raw) in [
            (ARK_GAME_INI_FILE, &config.game_ini),
            (ARK_GAME_USER_SETTINGS_FILE, &config.game_user_settings_ini),
        ] {
            total += raw.len();
            if raw.len() > MAX_FRAGMENT_BYTES || total > MAX_TOTAL_BYTES {
                return Err(format!(
                    "Mod {id}: INI content exceeds the size limit (64 KiB per file, 1 MiB total)"
                ));
            }
            if raw
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
            {
                return Err(format!(
                    "Mod {id} / {file}: control characters are not allowed"
                ));
            }
            Document::parse(raw).map_err(|e| format!("Mod {id} / {file}: {e}"))?;
        }
    }
    Ok(configurations)
}

fn enabled_ids(settings: &Map<String, Value>) -> HashSet<String> {
    ["mod_ids_csv", "passive_mod_ids_csv"]
        .into_iter()
        .filter_map(|field| settings.get(field).and_then(Value::as_str))
        .flat_map(app_core::ark_mods::parse_mod_ids)
        .filter_map(|id| {
            id.parse::<u64>()
                .ok()
                .filter(|id| *id > 0)
                .map(|id| id.to_string())
        })
        .collect()
}

// These native arrays have stable, author-supplied identities. Other unknown
// repeated keys stay ordered within one Mod, but cannot silently share owners.
fn rule_identity(section: &str, key: &str, line: &str) -> Result<Option<String>, String> {
    if section != GAME_MODE {
        return Ok(None);
    }
    let identity = match key {
        "configoverrideitemcraftingcosts" | "configoverrideitemmaxquantity" => "ItemClassString",
        "overridenamedengramentries" => "EngramClassName",
        _ => return Ok(None),
    };
    crate::settings_validation::ark_mod_rule_identity(line, identity).map(Some)
}

type Claim = (String, String, Option<String>);

fn claim_document(
    document: &Document,
    owner: &str,
    claims: &mut HashMap<Claim, String>,
    managed: &HashSet<NativeKey>,
    is_mod: bool,
) -> Result<(), String> {
    for section in &document.sections {
        for line in &section.lines {
            let Some(key) = assignment(line) else {
                continue;
            };
            let identity = rule_identity(&section.name, &key, line)?;
            if is_mod
                && identity.is_none()
                && managed.contains(&(section.name.clone(), key_family(&key)))
            {
                return Err(format!(
                    "{owner}: [{}] {key} is managed by the server configuration page",
                    section.name
                ));
            }
            let claim = (
                section.name.clone(),
                key.trim_start_matches(['+', '-', '.', '!']).to_owned(),
                identity,
            );
            if let Some(previous) = claims.get(&claim) {
                if previous != owner || (is_mod && claim.2.is_some()) {
                    return Err(format!(
                        "{owner}: [{}] {} conflicts with {previous}{}",
                        claim.0,
                        claim.1,
                        claim
                            .2
                            .as_ref()
                            .map(|id| format!(" (class {id})"))
                            .unwrap_or_default()
                    ));
                }
            } else {
                claims.insert(claim, owner.into());
            }
        }
    }
    Ok(())
}

/// Validate both destinations even if only one is being written, before mutation.
pub(crate) fn validate(settings: &Map<String, Value>) -> Result<(), String> {
    if read_asa_mod_configurations(settings)?.is_empty() {
        return Ok(());
    }
    for game_ini in [true, false] {
        let mut base = raw_extra_document(settings, game_ini)?;
        let raw_keys = base.keys();
        // Include native repeated rules in collision checks before rendering.
        if game_ini {
            let mut rules = String::from("[/script/shootergame.shootergamemode]\n");
            for (field, key) in [
                (
                    "config_override_item_crafting_costs",
                    "ConfigOverrideItemCraftingCosts",
                ),
                (
                    "config_override_item_max_quantity",
                    "ConfigOverrideItemMaxQuantity",
                ),
                (
                    "override_named_engram_entries",
                    "OverrideNamedEngramEntries",
                ),
            ] {
                // Raw INI replaces the entire exact-key sequence at render time.
                // Superseded typed rows must not retain conflict ownership.
                if raw_keys.contains(&(GAME_MODE.into(), key.to_ascii_lowercase())) {
                    continue;
                }
                for line in settings
                    .get(field)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                {
                    let line = line.trim();
                    rules.push_str(&if line.starts_with(key) {
                        line.into()
                    } else {
                        format!("{key}={line}")
                    });
                    rules.push('\n');
                }
            }
            base.sections.extend(Document::parse(&rules)?.sections);
        }
        append(settings, game_ini, &mut base)?;
    }
    Ok(())
}

pub(super) fn append(
    settings: &Map<String, Value>,
    game_ini: bool,
    base: &mut Document,
) -> Result<(), String> {
    let configurations = read_asa_mod_configurations(settings)?;
    if configurations.is_empty() {
        return Ok(());
    }
    let enabled = enabled_ids(settings);
    let managed = owned_keys("arksurvivalascended", game_ini);
    let mut claims = HashMap::new();
    claim_document(
        base,
        "existing server configuration / raw INI",
        &mut claims,
        &managed,
        false,
    )?;
    let file = if game_ini {
        ARK_GAME_INI_FILE
    } else {
        ARK_GAME_USER_SETTINGS_FILE
    };
    for (id, config) in configurations {
        if !enabled.contains(&id) {
            continue;
        }
        let raw = if game_ini {
            config.game_ini
        } else {
            config.game_user_settings_ini
        };
        let document = Document::parse(&raw)?;
        claim_document(
            &document,
            &format!("Mod {id} / {file}"),
            &mut claims,
            &managed,
            true,
        )?;
        append_metadata(&mut base.preamble, document.preamble);
        base.sections.extend(document.sections);
    }
    Ok(())
}
