use super::*;

const MAX_FIELD_BYTES: usize = 256;
const MAX_METADATA_NODES: usize = 64;

pub(super) fn read_metadata(mod_root: &Path, name: &str) -> Result<JsonValue, String> {
    let lua = Lua::new();
    lua.set_memory_limit(MAX_LUA_MEMORY_BYTES)
        .map_err(|error| error.to_string())?;
    install_instruction_budget(&lua).map_err(|error| error.to_string())?;
    let context = Rc::new(RefCell::new(LuaImportContext {
        mod_root: mod_root.to_path_buf(),
        imported: HashSet::new(),
    }));
    install_safe_globals(&lua, context, "en", name).map_err(|error| error.to_string())?;
    let root = mod_root.to_path_buf();
    let imported = Rc::new(RefCell::new(HashSet::new()));
    let bytes_left = Rc::new(Cell::new(MAX_MODINFO_BYTES * 2));
    let shared_budget = bytes_left.clone();
    let globals = lua.globals();
    let strings = globals
        .get::<Table>("string")
        .map_err(|error| error.to_string())?;
    // Native pattern backtracking does not consume Lua instructions. This reader
    // reports a metadata gap instead of running an unbounded C pattern matcher.
    for pattern in ["match", "find", "gmatch", "gsub"] {
        strings
            .set(
                pattern,
                lua.create_function(|_, _: mlua::MultiValue| -> mlua::Result<Value> {
                    Err(LuaError::runtime(
                        "Lua pattern matching is disabled in bounded Mod evidence reads.",
                    ))
                })
                .map_err(|error| error.to_string())?,
            )
            .map_err(|error| error.to_string())?;
    }
    for quiet in ["print", "warn"] {
        globals
            .set(
                quiet,
                lua.create_function(|_, _: mlua::MultiValue| Ok(()))
                    .map_err(|error| error.to_string())?,
            )
            .map_err(|error| error.to_string())?;
    }
    globals
        .set(
            "modimport",
            lua.create_function(move |lua, relative: String| {
                let relative = relative.replace('\\', "/");
                if relative.len() > 256 || relative.split('/').any(|part| !safe_component(part)) {
                    return Err(LuaError::runtime(
                        "modimport requires a relative helper path without traversal.",
                    ));
                }
                let relative = if Path::new(&relative).extension().is_none() {
                    format!("{relative}.lua")
                } else {
                    relative
                };
                if !relative.ends_with(".lua")
                    || Path::new(&relative)
                        .file_name()
                        .and_then(|value| value.to_str())
                        .is_some_and(|value| value.eq_ignore_ascii_case("modmain.lua"))
                {
                    return Err(LuaError::runtime(
                        "modimport cannot read or execute modmain.lua or non-Lua files.",
                    ));
                }
                let mut seen = imported.borrow_mut();
                if seen.contains(&relative) {
                    return Ok(());
                }
                if seen.len() >= MAX_IMPORTED_FILES {
                    return Err(LuaError::runtime(
                        "modimport exceeded its helper file limit.",
                    ));
                }
                seen.insert(relative.clone());
                drop(seen);
                let source = read_evidence_source(&root.join(&relative), &shared_budget)
                    .map_err(LuaError::runtime)?;
                lua.load(&source)
                    .set_mode(mlua::chunk::ChunkMode::Text)
                    .set_name(format!("@{relative}"))
                    .exec()
            })
            .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
    let source = read_evidence_source(&mod_root.join("modinfo.lua"), &bytes_left)?;
    lua.load(&source)
        .set_mode(mlua::chunk::ChunkMode::Text)
        .set_name("@modinfo.lua")
        .exec()
        .map_err(|error| {
            configuration_error_message(
                &error,
                &mut ConfigurationOutputBudget::new(MAX_FIELD_BYTES),
            )
            .unwrap_or_else(std::convert::identity)
        })?;
    let mut metadata = JsonMap::new();
    let mut remaining = MAX_METADATA_NODES;
    for key in [
        "name",
        "version",
        "api_version",
        "api_version_dst",
        "priority",
        "mod_dependencies",
        "dst_compatible",
        "dont_starve_together_compatible",
        "client_only_mod",
        "server_only_mod",
        "all_clients_require_mod",
    ] {
        let value = globals
            .raw_get::<Value>(key)
            .map_err(|error| error.to_string())?;
        if !matches!(value, Value::Nil) {
            metadata.insert(String::from(key), metadata_value(value, 0, &mut remaining)?);
        }
    }
    Ok(JsonValue::Object(metadata))
}

fn metadata_value(value: Value, depth: usize, remaining: &mut usize) -> Result<JsonValue, String> {
    *remaining = remaining
        .checked_sub(1)
        .ok_or("Mod metadata exceeded the node limit.")?;
    if depth > 4 {
        return Err(String::from(
            "Mod metadata is cyclic or exceeds the nesting limit.",
        ));
    }
    match value {
        Value::Nil => Ok(JsonValue::Null),
        Value::Boolean(value) => Ok(json!(value)),
        Value::Integer(value) => Ok(json!(value)),
        Value::Number(value) if value.is_finite() => Ok(json!(value)),
        Value::String(value) => {
            if value.as_bytes().len() > MAX_FIELD_BYTES {
                return Err(String::from(
                    "Mod metadata field exceeded the text byte limit.",
                ));
            }
            Ok(json!(
                value
                    .to_str()
                    .map_err(|_| "Mod metadata text is not UTF-8.")?
                    .to_string()
            ))
        }
        Value::Table(table) => {
            let mut entries = Vec::new();
            for entry in table.pairs::<Value, Value>() {
                let (key, value) = entry.map_err(|_| "Mod metadata table could not be read.")?;
                entries.push((key, metadata_value(value, depth + 1, remaining)?));
            }
            if entries
                .iter()
                .all(|(key, _)| matches!(key, Value::Integer(value) if *value > 0))
            {
                entries.sort_by_key(|(key, _)| {
                    if let Value::Integer(value) = key {
                        *value
                    } else {
                        0
                    }
                });
                if entries.iter().enumerate().any(|(index, (key, _))| !matches!(key, Value::Integer(value) if *value == index as i64 + 1)) {
                    return Err(String::from("Mod metadata contains a sparse sequence."));
                }
                Ok(JsonValue::Array(
                    entries.into_iter().map(|(_, value)| value).collect(),
                ))
            } else {
                let mut object = JsonMap::new();
                for (key, value) in entries {
                    let Value::String(key) = key else {
                        return Err(String::from("Mod metadata mixes array and object keys."));
                    };
                    if key.as_bytes().len() > MAX_FIELD_BYTES {
                        return Err(String::from(
                            "Mod metadata key exceeded the text byte limit.",
                        ));
                    }
                    object.insert(
                        key.to_str()
                            .map_err(|_| "Mod metadata key is not UTF-8.")?
                            .to_string(),
                        value,
                    );
                }
                Ok(JsonValue::Object(object))
            }
        }
        _ => Err(String::from(
            "Mod metadata contains an unsupported value type.",
        )),
    }
}
