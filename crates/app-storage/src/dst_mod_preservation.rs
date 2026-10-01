use std::collections::BTreeMap;
use std::io::{self, Write};

use serde_json::{Map, Value};

use super::super::templates_render_dst::render_dst_modoverrides;
use super::lua;

const MAX_INPUT_BYTES: usize = 128 * 1024;
const MAX_DECLARED_NAMES: usize = 100;
const MAX_DECLARED_NAME_BYTES: usize = 4096;

/// Check whether the rendered DST Mod configuration preserves existing features.
/// This checks saved declarations, not whether the game actually loaded a Mod.
pub fn validate_dst_mod_preservation(before: &Value, after: &Value) -> Result<(), String> {
    let before = bounded_settings(before)?;
    let after = bounded_settings(after)?;
    let before_shards = app_core::dst_shards::dst_shards(&Value::Object(before.clone()))?;
    let after_shards = app_core::dst_shards::dst_shards(&Value::Object(after.clone()))?;
    if before_shards
        .iter()
        .any(|shard| !after_shards.contains(shard))
    {
        return Err(String::from(
            "Mod preservation requires every initially active shard to remain enabled.",
        ));
    }
    let mut before_budget = NameBudget::default();
    let mut after_budget = NameBudget::default();
    for spec in before_shards {
        let shard = spec.process_key;
        let original = render_dst_modoverrides(before, shard);
        let proposed = render_dst_modoverrides(after, shard);
        // Opaque Lua stays user-owned. Identical effective source also permits
        // ordinary server edits without pretending that it was statically read.
        if original == proposed {
            continue;
        }
        let unknown = || {
            format!(
                "Cannot verify Mod preservation for {shard}: changed declarations are outside the supported static analysis limits."
            )
        };
        let original = declarations(&original, &mut before_budget).ok_or_else(unknown)?;
        let proposed = declarations(&proposed, &mut after_budget).ok_or_else(unknown)?;
        validate_shard(shard, &original, &proposed)?;
    }
    Ok(())
}

struct ModDeclaration {
    enabled: Option<bool>,
    options: Map<String, Value>,
}

struct ShardDeclarations {
    mods: BTreeMap<String, ModDeclaration>,
    client_policy: Option<Value>,
}

#[derive(Default)]
struct NameBudget {
    names: usize,
    bytes: usize,
}

fn declarations(source: &str, budget: &mut NameBudget) -> Option<ShardDeclarations> {
    // The shared parser bounds source bytes, nesting depth and total entries.
    let table = lua::parse(source, false)?;
    let mut mods = BTreeMap::new();
    let mut client_policy = None;
    for entry in table.entries {
        let name = entry.key?;
        if name == "client_mods_disabled" {
            let value = entry.node.scalar()?;
            if !matches!(value, Value::Bool(_) | Value::Null) {
                return None;
            }
            client_policy = Some(value.clone());
            continue;
        }
        budget.names += 1;
        budget.bytes += name.len();
        if budget.names > MAX_DECLARED_NAMES || budget.bytes > MAX_DECLARED_NAME_BYTES {
            return None;
        }
        let mut options = static_fields(entry.node.table()?)?;
        let enabled = match options.remove("enabled") {
            Some(Value::Bool(value)) => Some(value),
            None | Some(Value::Null) => None,
            _ => return None,
        };
        mods.insert(name, ModDeclaration { enabled, options });
    }
    Some(ShardDeclarations {
        mods,
        client_policy,
    })
}

fn static_fields(table: &lua::Table) -> Option<Map<String, Value>> {
    table
        .entries
        .iter()
        .map(|entry| {
            // Entry does not retain numeric key identities. Never mistake two
            // different indexed tables for equal option values.
            let key = entry.key.clone()?;
            let value = match &entry.node.value {
                lua::Literal::Scalar(value) => value.clone(),
                lua::Literal::Table(table) => Value::Object(static_fields(table)?),
            };
            Some((key, value))
        })
        .collect()
}

fn validate_shard(
    shard: &str,
    before: &ShardDeclarations,
    after: &ShardDeclarations,
) -> Result<(), String> {
    if before.client_policy != after.client_policy {
        return Err(format!(
            "Mod preservation requires the {shard} client-Mod policy to remain unchanged."
        ));
    }
    for (name, original) in &before.mods {
        let proposed = after.mods.get(name);
        let enablement_preserved = match original.enabled {
            Some(true) => proposed.is_some_and(|value| value.enabled == Some(true)),
            // Missing/nil is unspecified, not evidence that the Mod was disabled.
            None => proposed.is_some_and(|value| value.enabled != Some(false)),
            Some(false) => true,
        };
        if !enablement_preserved {
            return Err(format!(
                "Mod preservation forbids removing or disabling an initially enabled or unspecified {shard} Mod."
            ));
        }
        let options_preserved = match proposed {
            Some(proposed) => original.options == proposed.options,
            None => original.options.is_empty(),
        };
        if !options_preserved {
            return Err(format!(
                "Mod preservation requires existing {shard} configuration options and other non-enabled declarations to remain unchanged."
            ));
        }
    }
    Ok(())
}

fn bounded_settings(value: &Value) -> Result<&Map<String, Value>, String> {
    let settings = value
        .as_object()
        .ok_or_else(|| String::from("DST settings must be a JSON object."))?;
    serde_json::to_writer(
        JsonByteBudget {
            remaining: MAX_INPUT_BYTES,
        },
        value,
    )
    .map_err(|_| String::from("DST Mod preservation settings exceed the input byte limit."))?;
    Ok(settings)
}

struct JsonByteBudget {
    remaining: usize,
}

impl Write for JsonByteBudget {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.remaining {
            return Err(io::Error::other("JSON byte budget exceeded"));
        }
        self.remaining -= bytes.len();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
#[path = "dst_mod_preservation_tests.rs"]
mod tests;
