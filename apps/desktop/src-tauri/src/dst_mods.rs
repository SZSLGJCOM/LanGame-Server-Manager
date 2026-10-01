use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::text_decode::decode_utf8_or_gb18030_text;
use mlua::{Error as LuaError, HookTriggers, Lua, Table, Value, VmState};
use serde::Serialize;

const MAX_MODINFO_BYTES: usize = 768 * 1024;
const MAX_LUA_MEMORY_BYTES: usize = 32 * 1024 * 1024;
const MAX_CONFIGURATION_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
const MAX_CONFIGURATION_TEXT_BYTES: usize = 64 * 1024;
const MAX_CONFIGURATION_OPTIONS: usize = 512;
const MAX_CHOICES_PER_OPTION: usize = 1024;
const MAX_CONFIGURATION_CHOICES: usize = 8192;
const MAX_IMPORTED_FILES: usize = 24;
const INSTRUCTION_LIMIT: u32 = 120_000;
const DST_STEAM_APP_ID: u32 = 322330;

#[path = "dst_mods/evidence.rs"]
mod evidence;
pub use evidence::{
    dst_mod_dependency_names, dst_mod_error_names, read_dst_installed_mod_evidence,
    validate_directory_name as validate_dst_mod_directory_name,
};

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum DstModPrimitiveValue {
    String(String),
    Number(f64),
    Boolean(bool),
    Default,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DstModConfigChoice {
    pub label: String,
    pub hover: Option<String>,
    pub value: DstModPrimitiveValue,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DstModConfigOptionSpec {
    pub name: String,
    pub label: String,
    pub hover: Option<String>,
    pub default_value: Option<DstModPrimitiveValue>,
    pub options: Vec<DstModConfigChoice>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DstModConfigurationSpec {
    pub mod_id: String,
    pub mod_dir: String,
    pub modinfo_path: String,
    pub mod_name: Option<String>,
    pub description: Option<String>,
    pub client_only: bool,
    pub status: String,
    pub message: Option<String>,
    pub options: Vec<DstModConfigOptionSpec>,
}

#[derive(Debug)]
struct LuaImportContext {
    mod_root: PathBuf,
    imported: HashSet<PathBuf>,
}

#[cfg(test)]
pub fn read_dst_mod_configuration_specs(
    install_root: &Path,
    ids: &[String],
    locale: &str,
) -> Vec<DstModConfigurationSpec> {
    read_dst_mod_configuration_specs_with_roots(install_root, &[], ids, locale)
}

pub fn read_dst_mod_configuration_specs_with_roots(
    install_root: &Path,
    extra_roots: &[PathBuf],
    ids: &[String],
    locale: &str,
) -> Vec<DstModConfigurationSpec> {
    let normalized_locale = normalize_locale(locale);
    let mut seen = HashSet::new();
    let mut specs = Vec::new();

    for raw_id in ids {
        let Some(mod_id) = normalize_workshop_id(raw_id) else {
            continue;
        };
        if !seen.insert(mod_id.clone()) {
            continue;
        }

        let mod_dir_candidates = build_dst_mod_dir_candidates(install_root, extra_roots, &mod_id);
        let mod_dir = select_mod_dir_candidate(&mod_dir_candidates);
        let modinfo_path = mod_dir.join("modinfo.lua");
        let mut spec = DstModConfigurationSpec {
            mod_id,
            mod_dir: mod_dir.to_string_lossy().into_owned(),
            modinfo_path: modinfo_path.to_string_lossy().into_owned(),
            mod_name: None,
            description: None,
            client_only: false,
            status: String::from("loaded"),
            message: None,
            options: Vec::new(),
        };

        if !mod_dir.exists() {
            spec.status = String::from("missing_mod");
            spec.message = Some(String::from(
                "Workshop mod folder was not found under the installed DST mods directory, UGC storage, or Workshop cache.",
            ));
            specs.push(spec);
            continue;
        }

        if !modinfo_path.exists() {
            spec.status = String::from("missing_modinfo");
            spec.message = Some(String::from(
                "modinfo.lua was not found under the installed DST mod folder, UGC storage, or Workshop cache.",
            ));
            specs.push(spec);
            continue;
        }

        match parse_mod_configuration_spec(
            &mod_dir,
            &modinfo_path,
            &normalized_locale,
            &format!("workshop-{}", spec.mod_id),
        ) {
            Ok(parsed) => specs.push(DstModConfigurationSpec {
                mod_id: spec.mod_id,
                mod_dir: spec.mod_dir,
                modinfo_path: spec.modinfo_path,
                ..parsed
            }),
            Err(message) => {
                spec.status = String::from("parse_error");
                spec.message = Some(message);
                specs.push(spec);
            }
        }
    }

    specs
}

fn build_dst_mod_dir_candidates(
    install_root: &Path,
    extra_roots: &[PathBuf],
    mod_id: &str,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    // The selected instance owns its deployed UGC. A shared download cache may
    // contain an older modinfo.lua for the same Workshop ID.
    for root in extra_roots {
        for spec in app_core::dst_shards::DST_SHARDS {
            let shard = spec.directory;
            candidates.push(
                root.join("ugc")
                    .join(shard)
                    .join("content")
                    .join(DST_STEAM_APP_ID.to_string())
                    .join(mod_id),
            );
        }
    }
    candidates.push(install_root.join("mods").join(format!("workshop-{mod_id}")));
    candidates.extend(build_dst_ugc_mod_dir_candidates(
        install_root,
        DST_STEAM_APP_ID,
        mod_id,
    ));
    let roots = std::iter::once(install_root.to_path_buf()).chain(extra_roots.iter().cloned());
    for root in roots {
        for workshop_root in build_workshop_root_candidates(&root, DST_STEAM_APP_ID) {
            candidates.push(workshop_root.join(mod_id));
        }
    }
    dedupe_paths(candidates)
}

fn build_dst_ugc_mod_dir_candidates(
    install_root: &Path,
    app_id: u32,
    mod_id: &str,
) -> Vec<PathBuf> {
    let ugc_root = install_root.join("ugc_mods");
    let mut cluster_roots = vec![ugc_root.join("main")];

    if let Ok(entries) = fs::read_dir(&ugc_root) {
        let mut discovered = entries
            .flatten()
            .filter_map(|entry| {
                entry
                    .file_type()
                    .ok()
                    .filter(|file_type| file_type.is_dir())
                    .map(|_| entry.path())
            })
            .collect::<Vec<_>>();
        discovered.sort_by_key(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default()
        });
        cluster_roots.extend(discovered);
    }

    let mut candidates = Vec::new();
    for cluster_root in dedupe_paths(cluster_roots) {
        for spec in app_core::dst_shards::DST_SHARDS {
            let shard = spec.directory;
            candidates.push(
                cluster_root
                    .join(shard)
                    .join("content")
                    .join(app_id.to_string())
                    .join(mod_id),
            );
        }
    }
    candidates
}

fn select_mod_dir_candidate(candidates: &[PathBuf]) -> PathBuf {
    if let Some(with_modinfo) = candidates
        .iter()
        .find(|candidate| candidate.join("modinfo.lua").exists())
    {
        return with_modinfo.clone();
    }

    candidates
        .iter()
        .find(|candidate| candidate.exists())
        .cloned()
        .unwrap_or_else(|| {
            candidates
                .first()
                .cloned()
                .unwrap_or_else(|| PathBuf::from("mods").join("workshop-unknown"))
        })
}

fn build_workshop_root_candidates(base_root: &Path, app_id: u32) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    for root in [
        Some(base_root.to_path_buf()),
        base_root.parent().map(Path::to_path_buf),
        base_root
            .parent()
            .and_then(Path::parent)
            .map(Path::to_path_buf),
    ]
    .into_iter()
    .flatten()
    {
        candidates.push(
            root.join("steamapps")
                .join("workshop")
                .join("content")
                .join(app_id.to_string()),
        );
        candidates.push(
            root.join("steamapps")
                .join("workshop")
                .join(app_id.to_string()),
        );
        candidates.push(
            root.join("workshop")
                .join("content")
                .join(app_id.to_string()),
        );
        candidates.push(root.join("workshop").join(app_id.to_string()));
    }
    dedupe_paths(candidates)
}

fn dedupe_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    for path in paths {
        let key = path
            .to_string_lossy()
            .replace('\\', "/")
            .to_ascii_lowercase();
        if seen.insert(key) {
            deduped.push(path);
        }
    }
    deduped
}

fn parse_mod_configuration_spec(
    mod_dir: &Path,
    modinfo_path: &Path,
    locale: &str,
    folder_name: &str,
) -> Result<DstModConfigurationSpec, String> {
    let lua = Lua::new();
    // Native Lua functions can allocate without consuming the instruction budget.
    // Imported helpers share this state and therefore the same memory ceiling.
    lua.set_memory_limit(MAX_LUA_MEMORY_BYTES)
        .map_err(|error| {
            format!(
                "failed to bound Lua memory while reading {}: {error}",
                modinfo_path.display()
            )
        })?;

    install_instruction_budget(&lua).map_err(|error| error.to_string())?;

    let mod_root = fs::canonicalize(mod_dir).unwrap_or_else(|_| mod_dir.to_path_buf());
    let context = Rc::new(RefCell::new(LuaImportContext {
        mod_root,
        imported: HashSet::new(),
    }));

    install_safe_globals(&lua, context, locale, folder_name).map_err(|error| error.to_string())?;

    let exec_result = exec_mod_file(&lua, modinfo_path);
    let globals = lua.globals();
    let mut output_budget = ConfigurationOutputBudget::new(MAX_CONFIGURATION_OUTPUT_BYTES);
    let options = globals
        .get::<Option<Table>>("configuration_options")
        .map_err(|error| {
            configuration_error_message(&error, &mut output_budget)
                .unwrap_or_else(std::convert::identity)
        })?
        .map(|table| parse_configuration_options(table, &mut output_budget))
        .transpose()
        .map_err(|error| {
            configuration_error_message(&error, &mut output_budget)
                .unwrap_or_else(std::convert::identity)
        })?
        .unwrap_or_default();

    let mod_name =
        read_configuration_text(&globals, "name", &mut output_budget).map_err(|error| {
            configuration_error_message(&error, &mut output_budget)
                .unwrap_or_else(std::convert::identity)
        })?;
    let description = read_configuration_text(&globals, "description", &mut output_budget)
        .map_err(|error| {
            configuration_error_message(&error, &mut output_budget)
                .unwrap_or_else(std::convert::identity)
        })?;
    let client_only = matches!(
        globals
            .raw_get::<Value>("client_only_mod")
            .map_err(|error| {
                configuration_error_message(&error, &mut output_budget)
                    .unwrap_or_else(std::convert::identity)
            })?,
        Value::Boolean(true)
    );

    let mut status = String::from("loaded");
    let mut message = None;

    match exec_result {
        Ok(()) if options.is_empty() => {
            status = String::from("no_options");
            let explanation = "This mod did not expose configuration_options in modinfo.lua.";
            output_budget
                .charge(explanation.len())
                .map_err(|error| error.to_string())?;
            message = Some(String::from(explanation));
        }
        Ok(()) => {}
        Err(error) if !options.is_empty() => {
            status = String::from("loaded_with_warnings");
            message = Some(configuration_error_message(&error, &mut output_budget)?);
        }
        Err(error) => return Err(configuration_error_message(&error, &mut output_budget)?),
    }

    Ok(DstModConfigurationSpec {
        mod_id: String::new(),
        mod_dir: String::new(),
        modinfo_path: String::new(),
        mod_name,
        description,
        client_only,
        status,
        message,
        options,
    })
}

fn install_instruction_budget(lua: &Lua) -> mlua::Result<()> {
    let remaining = Cell::new(INSTRUCTION_LIMIT);
    // A global hook covers coroutines. Checking every instruction also prevents
    // pcall/xpcall from resuming script execution after the shared budget expires.
    lua.set_global_hook(HookTriggers::new().every_nth_instruction(1), move |_, _| {
        let Some(next) = remaining.get().checked_sub(1) else {
            return Err(LuaError::runtime(
                "modinfo.lua exceeded the instruction limit",
            ));
        };
        remaining.set(next);
        Ok(VmState::Continue)
    })
}

fn install_safe_globals(
    lua: &Lua,
    context: Rc<RefCell<LuaImportContext>>,
    locale: &str,
    folder_name: &str,
) -> mlua::Result<()> {
    let globals = lua.globals();
    let locale_value = String::from(locale);
    globals.set("locale", locale)?;
    globals.set("folder_name", folder_name)?;
    globals.set("modname", folder_name)?;
    globals.set("io", Value::Nil)?;
    globals.set("os", Value::Nil)?;
    globals.set("package", Value::Nil)?;

    for blocked in ["dofile", "load", "loadfile"] {
        let name = String::from(blocked);
        globals.set(
            blocked,
            lua.create_function(move |_lua, ()| -> mlua::Result<()> {
                Err(LuaError::runtime(format!(
                    "{name} is disabled while reading modinfo.lua"
                )))
            })?,
        )?;
    }

    globals.set(
        "require",
        lua.create_function(|_lua, ()| -> mlua::Result<()> {
            Err(LuaError::runtime(
                "require is disabled while reading modinfo.lua",
            ))
        })?,
    )?;

    globals.set("collectgarbage", lua.create_function(|_lua, ()| Ok(0_u32))?)?;

    globals.set(
        "modimport",
        lua.create_function(move |lua, relative_path: String| {
            let import_path = resolve_import_path(&context.borrow().mod_root, &relative_path)
                .map_err(LuaError::runtime)?;
            {
                let mut borrowed = context.borrow_mut();
                if borrowed.imported.len() >= MAX_IMPORTED_FILES {
                    return Err(LuaError::runtime(
                        "modinfo.lua imported too many helper files while reading configuration.",
                    ));
                }
                if !borrowed.imported.insert(import_path.clone()) {
                    return Ok(());
                }
            }
            exec_mod_file(lua, &import_path)
        })?,
    )?;

    globals.set(
        "ChooseTranslationTable",
        lua.create_function(move |_lua, table: Table| {
            choose_translation_value(&table, &locale_value)
        })?,
    )?;

    Ok(())
}

fn exec_mod_file(lua: &Lua, path: &Path) -> mlua::Result<()> {
    let source = read_lua_source(path).map_err(LuaError::external)?;
    let chunk_name = path.to_string_lossy().into_owned();
    lua.load(&source).set_name(&chunk_name).exec()
}

fn read_lua_source(path: &Path) -> Result<String, String> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .and_then(|file| {
            file.take((MAX_MODINFO_BYTES + 1) as u64)
                .read_to_end(&mut bytes)
        })
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    if bytes.len() > MAX_MODINFO_BYTES {
        return Err(format!(
            "{} exceeds the {}-byte limit for Lua configuration files.",
            path.display(),
            MAX_MODINFO_BYTES
        ));
    }

    Ok(decode_utf8_or_gb18030_text(&bytes)
        .trim_start_matches('\u{feff}')
        .to_owned())
}

fn resolve_import_path(mod_root: &Path, relative_path: &str) -> Result<PathBuf, String> {
    let trimmed = relative_path.trim();
    if trimmed.is_empty() {
        return Err(String::from("modimport received an empty relative path"));
    }

    let candidate = if Path::new(trimmed).extension().is_some() {
        mod_root.join(trimmed)
    } else {
        mod_root.join(format!("{trimmed}.lua"))
    };

    let canonical = fs::canonicalize(&candidate).map_err(|error| {
        format!(
            "failed to resolve modimport path {}: {error}",
            candidate.display()
        )
    })?;
    let canonical_root = fs::canonicalize(mod_root)
        .map_err(|error| format!("failed to resolve mod root {}: {error}", mod_root.display()))?;

    if !canonical.starts_with(&canonical_root) {
        return Err(format!(
            "refused to load helper outside the mod folder: {}",
            canonical.display()
        ));
    }

    Ok(canonical)
}

fn choose_translation_value(table: &Table, locale: &str) -> mlua::Result<Value> {
    for key in translation_candidates(locale) {
        if let Some(value) = table.get::<Option<Value>>(key)? {
            return Ok(value);
        }
    }

    if let Some(value) = table.get::<Option<Value>>("default")? {
        return Ok(value);
    }

    if let Some(pair) = table.pairs::<Value, Value>().next() {
        let (_, value) = pair?;
        return Ok(value);
    }

    Ok(Value::Nil)
}

fn translation_candidates(locale: &str) -> Vec<&str> {
    match locale {
        "zh" => vec![
            "zh", "zhr", "chs", "sc", "schinese", "cn", "chi", "chinese", "en", "english",
        ],
        _ => vec!["en", "english", "default"],
    }
}

struct ConfigurationOutputBudget {
    remaining_bytes: usize,
    remaining_choices: usize,
}

impl ConfigurationOutputBudget {
    fn new(bytes: usize) -> Self {
        Self {
            remaining_bytes: bytes,
            remaining_choices: MAX_CONFIGURATION_CHOICES,
        }
    }

    fn charge(&mut self, bytes: usize) -> mlua::Result<()> {
        self.remaining_bytes = self.remaining_bytes.checked_sub(bytes).ok_or_else(|| {
            LuaError::runtime("modinfo.lua exceeded the configuration output byte limit")
        })?;
        Ok(())
    }

    fn copy_text(&mut self, value: mlua::LuaString) -> mlua::Result<String> {
        if value.as_bytes().len() > MAX_CONFIGURATION_TEXT_BYTES {
            return Err(LuaError::runtime(
                "modinfo.lua configuration text exceeds the 64 KiB per-value limit",
            ));
        }
        let value = value.to_str()?;
        let value = value.trim();
        // Shared Lua strings are charged on every Rust copy, not once per identity.
        self.charge(value.len())?;
        Ok(value.to_owned())
    }
}

fn configuration_error_message(
    error: &LuaError,
    budget: &mut ConfigurationOutputBudget,
) -> Result<String, String> {
    struct ErrorMessage {
        text: String,
        remaining: usize,
    }

    impl std::fmt::Write for ErrorMessage {
        fn write_str(&mut self, text: &str) -> std::fmt::Result {
            self.remaining = self
                .remaining
                .checked_sub(text.len())
                .ok_or(std::fmt::Error)?;
            self.text.push_str(text);
            Ok(())
        }
    }

    let mut output = ErrorMessage {
        text: String::new(),
        remaining: budget.remaining_bytes.min(MAX_CONFIGURATION_TEXT_BYTES),
    };
    std::fmt::write(&mut output, format_args!("{error}")).map_err(|_| {
        String::from("modinfo.lua error message exceeded the configuration output limit")
    })?;
    budget
        .charge(output.text.len())
        .map_err(|error| error.to_string())?;
    Ok(output.text)
}

fn read_configuration_text(
    table: &Table,
    key: &str,
    budget: &mut ConfigurationOutputBudget,
) -> mlua::Result<Option<String>> {
    table
        .get::<Option<mlua::LuaString>>(key)?
        .map(|value| budget.copy_text(value))
        .transpose()
        .map(|value| value.filter(|value| !value.is_empty()))
}

fn parse_configuration_options(
    table: Table,
    budget: &mut ConfigurationOutputBudget,
) -> mlua::Result<Vec<DstModConfigOptionSpec>> {
    let mut specs = Vec::new();

    for (index, entry) in table.sequence_values::<Table>().enumerate() {
        if index >= MAX_CONFIGURATION_OPTIONS {
            return Err(LuaError::runtime(
                "modinfo.lua has too many configuration options",
            ));
        }
        let entry = entry?;
        let Some(name) = read_configuration_text(&entry, "name", budget)? else {
            continue;
        };

        let label = match read_configuration_text(&entry, "label", budget)? {
            Some(label) => label,
            None => humanize_key(&name, budget)?,
        };
        let hover = read_configuration_text(&entry, "hover", budget)?;
        let default_value = entry
            .get::<Option<Value>>("default")?
            .map(|value| parse_primitive_value(value, budget))
            .transpose()?
            .flatten();
        let options = match entry.get::<Option<Table>>("options")? {
            Some(options) => parse_option_choices(options, budget)?,
            None => Vec::new(),
        };

        specs.push(DstModConfigOptionSpec {
            name,
            label,
            hover,
            default_value,
            options,
        });
    }

    Ok(specs)
}

fn parse_option_choices(
    table: Table,
    budget: &mut ConfigurationOutputBudget,
) -> mlua::Result<Vec<DstModConfigChoice>> {
    let mut choices = Vec::new();

    for (index, choice) in table.sequence_values::<Table>().enumerate() {
        if index >= MAX_CHOICES_PER_OPTION {
            return Err(LuaError::runtime(
                "modinfo.lua has too many choices for one option",
            ));
        }
        budget.remaining_choices = budget.remaining_choices.checked_sub(1).ok_or_else(|| {
            LuaError::runtime("modinfo.lua exceeded the total configuration choice limit")
        })?;
        let choice = choice?;
        let label = match read_configuration_text(&choice, "description", budget)? {
            Some(label) => label,
            None => {
                budget.charge("Option".len())?;
                String::from("Option")
            }
        };
        let hover = read_configuration_text(&choice, "hover", budget)?;
        let value = match choice.get::<Option<Value>>("data")? {
            Some(value) => match parse_primitive_value(value, budget)? {
                Some(value) => value,
                None => continue,
            },
            None => DstModPrimitiveValue::Default,
        };

        choices.push(DstModConfigChoice {
            label,
            hover,
            value,
        });
    }

    Ok(choices)
}

fn parse_primitive_value(
    value: Value,
    budget: &mut ConfigurationOutputBudget,
) -> mlua::Result<Option<DstModPrimitiveValue>> {
    match value {
        Value::Nil => Ok(Some(DstModPrimitiveValue::Default)),
        Value::Boolean(value) => Ok(Some(DstModPrimitiveValue::Boolean(value))),
        Value::Integer(value) => Ok(Some(DstModPrimitiveValue::Number(value as f64))),
        Value::Number(value) => Ok(Some(DstModPrimitiveValue::Number(value))),
        Value::String(value) => Ok(Some(DstModPrimitiveValue::String(budget.copy_text(value)?))),
        _ => Ok(None),
    }
}

fn normalize_workshop_id(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let normalized = trimmed.strip_prefix("workshop-").unwrap_or(trimmed);
    let digits = normalized
        .chars()
        .filter(|character| character.is_ascii_digit())
        .collect::<String>();

    (digits.len() >= 6).then_some(digits)
}

fn normalize_locale(locale: &str) -> String {
    let normalized = locale.trim().to_ascii_lowercase();
    if normalized.starts_with("zh") {
        String::from("zh")
    } else {
        String::from("en")
    }
}

fn humanize_key(key: &str, budget: &mut ConfigurationOutputBudget) -> mlua::Result<String> {
    let mut output = String::new();
    let mut previous_was_separator = true;

    for character in key.chars() {
        if character == '_' || character == '-' {
            if !output.is_empty() && !output.ends_with(' ') {
                budget.charge(1)?;
                output.push(' ');
            }
            previous_was_separator = true;
            continue;
        }

        if character.is_uppercase() && !previous_was_separator && !output.ends_with(' ') {
            budget.charge(1)?;
            output.push(' ');
        }

        if previous_was_separator {
            for uppercase in character.to_uppercase() {
                budget.charge(uppercase.len_utf8())?;
                output.push(uppercase);
            }
        } else {
            budget.charge(character.len_utf8())?;
            output.push(character);
        }
        previous_was_separator = false;
    }

    output.truncate(output.trim_end().len());
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_temp_root(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "langame-dst-mods-{label}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&path).expect("create temp root");
        path
    }

    #[test]
    fn rejects_shared_configuration_text_amplification() {
        let lua = Lua::new();
        for field in ["label", "hover", "default"] {
            let options: Table = lua
                .load(
                    r#"
local field = ...
local option = { name = "x", label = "X" }
option[field] = string.rep("a", 64)
return { option, option, option, option }
"#,
                )
                .call(field)
                .expect("create shared options");
            let error =
                parse_configuration_options(options, &mut ConfigurationOutputBudget::new(100))
                    .expect_err("reject repeated copies of a shared Lua string");
            assert!(
                error.to_string().contains("output byte limit"),
                "{field}: {error}"
            );
        }

        for field in ["description", "hover", "data"] {
            let choices: Table = lua
                .load(
                    r#"
local field = ...
local choice = { description = "X", data = true }
choice[field] = string.rep("a", 64)
return { choice, choice, choice, choice }
"#,
                )
                .call(field)
                .expect("create shared choices");
            let error = parse_option_choices(choices, &mut ConfigurationOutputBudget::new(100))
                .expect_err("reject repeated copies of shared choice text");
            assert!(
                error.to_string().contains("output byte limit"),
                "{field}: {error}"
            );
        }
    }

    #[test]
    fn configuration_output_budget_includes_top_level_metadata() {
        let lua = Lua::new();
        let options: Table = lua
            .load("return {{name='x',label='X',hover='H',default='D',options={{description='C',hover='O',data='V'}}}}")
            .eval()
            .expect("create complete option");
        let mut budget = ConfigurationOutputBudget::new(9);
        assert_eq!(
            parse_configuration_options(options, &mut budget)
                .expect("parse option")
                .len(),
            1
        );
        lua.globals().set("name", "N").expect("set name");
        lua.globals()
            .set("description", "S")
            .expect("set description");
        assert_eq!(
            read_configuration_text(&lua.globals(), "name", &mut budget).expect("read name"),
            Some(String::from("N"))
        );
        assert_eq!(
            read_configuration_text(&lua.globals(), "description", &mut budget)
                .expect("read description"),
            Some(String::from("S"))
        );
        assert!(read_configuration_text(&lua.globals(), "name", &mut budget).is_err());
    }

    #[test]
    fn oversized_lua_errors_fail_with_a_bounded_explanation() {
        let error = LuaError::runtime("repeated text ".repeat(20));
        let message = configuration_error_message(&error, &mut ConfigurationOutputBudget::new(32))
            .expect_err("reject an error larger than the remaining output budget");
        assert_eq!(
            message,
            "modinfo.lua error message exceeded the configuration output limit"
        );
    }

    #[test]
    fn metadata_lookup_errors_use_the_same_output_limit() {
        let root = make_temp_root("metadata-error-limit");
        let path = root.join("modinfo.lua");
        fs::write(
            &path,
            "setmetatable(_G, {__index=function() error(string.rep('x', 128 * 1024)) end})",
        )
        .expect("write metadata error fixture");
        let error = parse_mod_configuration_spec(&root, &path, "en", "workshop-test")
            .expect_err("reject oversized metadata lookup error");
        assert_eq!(
            error,
            "modinfo.lua error message exceeded the configuration output limit"
        );
        fs::remove_dir_all(root).expect("remove metadata error fixture");
    }

    #[test]
    fn rejects_oversized_metadata_before_copying_and_does_not_expand_tables() {
        let lua = Lua::new();
        let text = lua
            .create_string(vec![b'x'; MAX_CONFIGURATION_TEXT_BYTES + 1])
            .expect("create text");
        for field in ["name", "description"] {
            lua.globals()
                .set(field, text.clone())
                .expect("set metadata");
            let error = read_configuration_text(
                &lua.globals(),
                field,
                &mut ConfigurationOutputBudget::new(MAX_CONFIGURATION_OUTPUT_BYTES),
            )
            .expect_err("reject oversized metadata");
            assert!(error.to_string().contains("per-value limit"));
        }
        let cyclic: Table = lua
            .load("local value = {}; value.self = value; return value")
            .eval()
            .expect("create cyclic table");
        assert!(
            parse_primitive_value(Value::Table(cyclic), &mut ConfigurationOutputBudget::new(0))
                .expect("ignore non-scalar value")
                .is_none()
        );
    }

    #[test]
    fn rejects_excessive_configuration_rows_without_truncation() {
        let lua = Lua::new();
        let table: Table = lua.load("local row={name='x',label='X'}; local rows={}; for i=1,... do rows[i]=row end; return rows").call(MAX_CONFIGURATION_OPTIONS + 1).expect("create options");
        assert!(
            parse_configuration_options(
                table,
                &mut ConfigurationOutputBudget::new(MAX_CONFIGURATION_OUTPUT_BYTES)
            )
            .expect_err("reject option count")
            .to_string()
            .contains("too many configuration options")
        );

        let table: Table = lua.load("local row={description='X',data=true}; local rows={}; for i=1,... do rows[i]=row end; return rows").call(MAX_CHOICES_PER_OPTION + 1).expect("create choices");
        assert!(
            parse_option_choices(
                table,
                &mut ConfigurationOutputBudget::new(MAX_CONFIGURATION_OUTPUT_BYTES)
            )
            .expect_err("reject choices per option")
            .to_string()
            .contains("too many choices for one option")
        );

        let table: Table = lua.load("local choices={}; for i=1,... do choices[i]={description='X',data=true} end; return choices").call(MAX_CHOICES_PER_OPTION).expect("create shared choices");
        let mut budget = ConfigurationOutputBudget::new(MAX_CONFIGURATION_OUTPUT_BYTES);
        for _ in 0..MAX_CONFIGURATION_CHOICES / MAX_CHOICES_PER_OPTION {
            parse_option_choices(table.clone(), &mut budget)
                .expect("accept choices within aggregate limit");
        }
        assert!(
            parse_option_choices(table, &mut budget)
                .expect_err("reject aggregate choice count")
                .to_string()
                .contains("total configuration choice limit")
        );
    }

    #[test]
    fn instruction_budget_cannot_be_reset_by_coroutines_or_protected_calls() {
        for script in [
            "for i=1,100 do coroutine.wrap(function() local n=0; for j=1,1000 do n=n+j end end)() end; completed=true",
            "for i=1,10 do pcall(function() local n=0; for j=1,1000000 do n=n+j end end) end; completed=true",
            "for i=1,10 do xpcall(function() local n=0; for j=1,1000000 do n=n+j end end, function(error) return error end) end; completed=true",
        ] {
            let lua = Lua::new();
            install_instruction_budget(&lua).expect("install instruction budget");
            lua.load(script)
                .exec()
                .expect_err("reject exhausted shared budget");
            assert_eq!(
                lua.globals()
                    .raw_get::<Option<bool>>("completed")
                    .expect("inspect completion"),
                None
            );
        }
    }

    #[test]
    fn preserves_protected_calls_and_coroutines_within_instruction_budget() {
        let lua = Lua::new();
        install_instruction_budget(&lua).expect("install instruction budget");
        lua.load(
            r#"
local ok, result = pcall(function() return 42 end)
assert(ok and result == 42)
local ok, result = xpcall(function() error("expected") end, function() return "handled" end)
assert(not ok and result == "handled")
assert(coroutine.wrap(function() return 7 end)() == 7)
"#,
        )
        .exec()
        .expect("preserve ordinary Lua control flow");
    }

    #[test]
    fn rejects_single_native_allocation_above_modinfo_memory_budget() {
        let root = make_temp_root("native-allocation-limit");
        let mod_root = root.join("mods").join("workshop-1909182187");
        fs::create_dir_all(&mod_root).expect("create mod root");
        fs::write(
            mod_root.join("modinfo.lua"),
            "name = string.rep('x', 64 * 1024 * 1024)",
        )
        .expect("write modinfo");

        let specs = read_dst_mod_configuration_specs(&root, &[String::from("1909182187")], "en");
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].status, "parse_error");
        assert!(
            specs[0]
                .message
                .as_deref()
                .unwrap_or_default()
                .contains("memory")
        );
        fs::remove_dir_all(root).expect("remove modinfo fixture");
    }

    #[test]
    fn imported_helpers_share_the_modinfo_memory_budget() {
        let root = make_temp_root("import-memory-limit");
        let mod_root = root.join("mods").join("workshop-1909182187");
        fs::create_dir_all(&mod_root).expect("create mod root");
        fs::write(
            mod_root.join("modinfo.lua"),
            "retained = string.rep('x', 12 * 1024 * 1024)\nmodimport('helper')",
        )
        .expect("write modinfo");
        fs::write(
            mod_root.join("helper.lua"),
            r#"
configuration_options = {
    { name = "enabled", options = { { description = "Yes", data = true } }, default = true }
}
additional = string.rep('y', 12 * 1024 * 1024)
"#,
        )
        .expect("write helper");

        let specs = read_dst_mod_configuration_specs(&root, &[String::from("1909182187")], "en");
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].status, "loaded_with_warnings");
        assert_eq!(specs[0].options.len(), 1);
        assert!(
            specs[0]
                .message
                .as_deref()
                .unwrap_or_default()
                .contains("memory")
        );
        fs::remove_dir_all(root).expect("remove import fixture");
    }

    #[test]
    fn accepts_source_at_byte_limit_and_rejects_larger_files() {
        let root = make_temp_root("source-byte-limit");
        let path = root.join("modinfo.lua");
        let mut source = vec![b' '; MAX_MODINFO_BYTES];
        fs::write(&path, &source).expect("write source at limit");
        assert_eq!(
            read_lua_source(&path).expect("read bounded source").len(),
            MAX_MODINFO_BYTES
        );

        source.push(b' ');
        fs::write(&path, &source).expect("write oversized source");
        let error = read_lua_source(&path).expect_err("reject oversized source");
        assert!(error.contains("exceeds the"));
        assert!(error.contains("modinfo.lua"));
        fs::remove_dir_all(root).expect("remove source fixture");
    }

    #[test]
    fn parses_modinfo_configuration_options_with_translation_and_imports() {
        let root = make_temp_root("translation");
        let mod_root = root.join("mods").join("workshop-2039181790");
        fs::create_dir_all(&mod_root).expect("create mod root");
        fs::write(
            mod_root.join("strings.lua"),
            r#"
strings = {
  label = "语言",
  option_zh = "中文",
  option_default = "默认",
}
"#,
        )
        .expect("write strings");
        fs::write(
            mod_root.join("modinfo.lua"),
            r#"
local text = ChooseTranslationTable({
  en = { hover = "Choose the label language" },
  zh = { hover = "选择标签语言" },
})
modimport("strings.lua")
name = "Insight"
description = "Server-side overlay"
configuration_options = {
  {
    name = "language",
    label = strings.label,
    hover = text.hover,
    options = {
      { description = strings.option_zh, data = "zh" },
      { description = strings.option_default, data = nil },
    },
    default = "en",
  },
  {
    name = "range_ring",
    label = "Range Ring",
    options = {
      { description = "On", data = true },
      { description = "Off", data = false },
    },
    default = true,
  },
}
"#,
        )
        .expect("write modinfo");

        let specs = read_dst_mod_configuration_specs(&root, &[String::from("2039181790")], "zh-CN");
        assert_eq!(specs.len(), 1);
        let spec = &specs[0];
        assert_eq!(spec.status, "loaded");
        assert_eq!(spec.mod_name.as_deref(), Some("Insight"));
        assert_eq!(spec.description.as_deref(), Some("Server-side overlay"));
        assert_eq!(spec.options.len(), 2);
        assert_eq!(spec.options[0].label, "语言");
        assert_eq!(spec.options[0].hover.as_deref(), Some("选择标签语言"));
        assert_eq!(
            spec.options[0].default_value,
            Some(DstModPrimitiveValue::String(String::from("en")))
        );
        assert_eq!(
            spec.options[0].options[1].value,
            DstModPrimitiveValue::Default
        );
        assert_eq!(
            spec.options[1].default_value,
            Some(DstModPrimitiveValue::Boolean(true))
        );

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn decodes_gbk_modinfo_configuration_options() {
        let root = make_temp_root("gbk");
        let mod_root = root.join("mods").join("workshop-2039181790");
        fs::create_dir_all(&mod_root).expect("create mod root");
        fs::write(
            mod_root.join("modinfo.lua"),
            [
                0x6E, 0x61, 0x6D, 0x65, 0x20, 0x3D, 0x20, 0x22, 0xD6, 0xD0, 0xCE, 0xC4, 0x20, 0x4D,
                0x6F, 0x64, 0x22, 0x0A, 0x64, 0x65, 0x73, 0x63, 0x72, 0x69, 0x70, 0x74, 0x69, 0x6F,
                0x6E, 0x20, 0x3D, 0x20, 0x22, 0xC5, 0xE4, 0xD6, 0xC3, 0xCB, 0xB5, 0xC3, 0xF7, 0x22,
                0x0A, 0x63, 0x6F, 0x6E, 0x66, 0x69, 0x67, 0x75, 0x72, 0x61, 0x74, 0x69, 0x6F, 0x6E,
                0x5F, 0x6F, 0x70, 0x74, 0x69, 0x6F, 0x6E, 0x73, 0x20, 0x3D, 0x20, 0x7B, 0x0A, 0x20,
                0x20, 0x7B, 0x0A, 0x20, 0x20, 0x20, 0x20, 0x6E, 0x61, 0x6D, 0x65, 0x20, 0x3D, 0x20,
                0x22, 0x6C, 0x61, 0x6E, 0x67, 0x75, 0x61, 0x67, 0x65, 0x22, 0x2C, 0x0A, 0x20, 0x20,
                0x20, 0x20, 0x6C, 0x61, 0x62, 0x65, 0x6C, 0x20, 0x3D, 0x20, 0x22, 0xD3, 0xEF, 0xD1,
                0xD4, 0x22, 0x2C, 0x0A, 0x20, 0x20, 0x20, 0x20, 0x6F, 0x70, 0x74, 0x69, 0x6F, 0x6E,
                0x73, 0x20, 0x3D, 0x20, 0x7B, 0x0A, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x7B, 0x20,
                0x64, 0x65, 0x73, 0x63, 0x72, 0x69, 0x70, 0x74, 0x69, 0x6F, 0x6E, 0x20, 0x3D, 0x20,
                0x22, 0xD6, 0xD0, 0xCE, 0xC4, 0x22, 0x2C, 0x20, 0x64, 0x61, 0x74, 0x61, 0x20, 0x3D,
                0x20, 0x22, 0x7A, 0x68, 0x22, 0x20, 0x7D, 0x2C, 0x0A, 0x20, 0x20, 0x20, 0x20, 0x7D,
                0x2C, 0x0A, 0x20, 0x20, 0x20, 0x20, 0x64, 0x65, 0x66, 0x61, 0x75, 0x6C, 0x74, 0x20,
                0x3D, 0x20, 0x22, 0x7A, 0x68, 0x22, 0x2C, 0x0A, 0x20, 0x20, 0x7D, 0x2C, 0x0A, 0x7D,
                0x0A,
            ],
        )
        .expect("write gbk modinfo");

        let specs = read_dst_mod_configuration_specs(&root, &[String::from("2039181790")], "zh-CN");
        assert_eq!(specs.len(), 1);
        let spec = &specs[0];
        assert_eq!(spec.status, "loaded");
        assert_eq!(spec.mod_name.as_deref(), Some("\u{4e2d}\u{6587} Mod"));
        assert_eq!(
            spec.description.as_deref(),
            Some("\u{914d}\u{7f6e}\u{8bf4}\u{660e}")
        );
        assert_eq!(spec.options[0].label, "\u{8bed}\u{8a00}");
        assert_eq!(spec.options[0].options[0].label, "\u{4e2d}\u{6587}");

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn keeps_options_when_modinfo_errors_after_assignment() {
        let root = make_temp_root("partial-error");
        let mod_root = root.join("mods").join("workshop-1909182187");
        fs::create_dir_all(&mod_root).expect("create mod root");
        fs::write(
            mod_root.join("modinfo.lua"),
            r#"
configuration_options = {
  {
    name = "marker_scale",
    label = "Marker Scale",
    options = {
      { description = "1.0x", data = 1.0 },
      { description = "1.5x", data = 1.5 },
    },
    default = 1.0,
  },
}
error("boom")
"#,
        )
        .expect("write modinfo");

        let specs = read_dst_mod_configuration_specs(&root, &[String::from("1909182187")], "en");
        assert_eq!(specs.len(), 1);
        let spec = &specs[0];
        assert_eq!(spec.status, "loaded_with_warnings");
        assert!(spec.message.as_deref().unwrap_or_default().contains("boom"));
        assert_eq!(spec.options.len(), 1);

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn reads_configuration_options_from_downloaded_workshop_cache() {
        let root = make_temp_root("workshop-cache");
        let mod_root = root
            .join("steamapps")
            .join("workshop")
            .join("content")
            .join("322330")
            .join("1909182187");
        fs::create_dir_all(&mod_root).expect("create workshop cache mod root");
        fs::write(
            mod_root.join("modinfo.lua"),
            r#"
name = "Cache Only Mod"
configuration_options = {
  {
    name = "marker_scale",
    label = "Marker Scale",
    options = {
      { description = "1.0x", data = 1.0 },
      { description = "1.5x", data = 1.5 },
    },
    default = 1.0,
  },
}
"#,
        )
        .expect("write cached modinfo");

        let specs = read_dst_mod_configuration_specs(&root, &[String::from("1909182187")], "en");
        assert_eq!(specs.len(), 1);
        let spec = &specs[0];
        assert_eq!(spec.status, "loaded");
        assert_eq!(spec.mod_name.as_deref(), Some("Cache Only Mod"));
        assert_eq!(spec.mod_dir, mod_root.to_string_lossy());
        assert_eq!(spec.options.len(), 1);
        assert_eq!(spec.options[0].name, "marker_scale");

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn dst_mod_pipeline_workshop_folder_name_is_logical_in_every_cache_layout() {
        let root = make_temp_root("logical-folder-name");
        for relative in [
            "mods/workshop-1909182187",
            "steamapps/workshop/content/322330/1909182187",
            "ugc_mods/main/Master/content/322330/1909182187",
        ] {
            let mod_root = root.join(relative);
            fs::create_dir_all(&mod_root).expect("create cache layout");
            fs::write(mod_root.join("modinfo.lua"), r#"
name = folder_name
configuration_options = {{name="official", options={{description="Yes",data=true},{description="No",data=false}}, default=folder_name == "workshop-1909182187"}}
"#).expect("write synthetic modinfo");
            let specs =
                read_dst_mod_configuration_specs(&root, &[String::from("1909182187")], "en");
            assert_eq!(specs[0].status, "loaded", "{relative}");
            assert_eq!(
                specs[0].mod_name.as_deref(),
                Some("workshop-1909182187"),
                "{relative}"
            );
            assert_eq!(
                specs[0].options[0].default_value,
                Some(DstModPrimitiveValue::Boolean(true)),
                "{relative}"
            );
            fs::remove_dir_all(&mod_root).expect("remove fixture cache");
        }
        fs::remove_dir_all(root).expect("remove fixture root");
    }

    #[test]
    fn reads_configuration_options_from_dst_ugc_storage() {
        let root = make_temp_root("ugc-storage");
        let mod_root = root
            .join("ugc_mods")
            .join("custom-cluster")
            .join("Master")
            .join("content")
            .join("322330")
            .join("1909182187");
        fs::create_dir_all(&mod_root).expect("create UGC mod root");
        fs::write(
            mod_root.join("modinfo.lua"),
            r#"
name = "UGC Mod"
configuration_options = {
  {
    name = "respawn_delay",
    label = "Respawn Delay",
    options = {
      { description = "Short", data = 5 },
      { description = "Long", data = 15 },
    },
    default = 5,
  },
}
"#,
        )
        .expect("write UGC modinfo");

        let specs = read_dst_mod_configuration_specs(&root, &[String::from("1909182187")], "en");
        assert_eq!(specs.len(), 1);
        let spec = &specs[0];
        assert_eq!(spec.status, "loaded");
        assert_eq!(spec.mod_name.as_deref(), Some("UGC Mod"));
        assert_eq!(spec.mod_dir, mod_root.to_string_lossy());
        assert_eq!(spec.options[0].name, "respawn_delay");

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn instance_ugc_configuration_precedes_shared_mod_with_same_workshop_id() {
        let root = make_temp_root("instance-ugc-precedence");
        let install_root = root.join("server");
        let instance_data_root = root.join("instance-data");
        let shared_mod = install_root.join("mods/workshop-1909182187");
        let instance_mod = instance_data_root.join("ugc/Master/content/322330/1909182187");
        fs::create_dir_all(&shared_mod).expect("create shared mod");
        fs::create_dir_all(&instance_mod).expect("create instance mod");
        fs::write(
            shared_mod.join("modinfo.lua"),
            "name = \"Shared version\"\n",
        )
        .expect("write shared modinfo");
        fs::write(
            instance_mod.join("modinfo.lua"),
            "name = \"Instance version\"\n",
        )
        .expect("write instance modinfo");

        let specs = read_dst_mod_configuration_specs_with_roots(
            &install_root,
            std::slice::from_ref(&instance_data_root),
            &[String::from("1909182187")],
            "en",
        );
        assert_eq!(specs[0].mod_name.as_deref(), Some("Instance version"));
        assert_eq!(Path::new(&specs[0].mod_dir), instance_mod);

        fs::remove_dir_all(root).ok();
    }
    #[test]
    fn reads_configuration_options_from_instance_isolated_dst_ugc_storage() {
        let root = make_temp_root("instance-ugc-storage");
        let install_root = root.join("server");
        let instance_data_root = root.join("instance-data");
        let mod_root = instance_data_root
            .join("ugc")
            .join("Master")
            .join("content")
            .join("322330")
            .join("1909182187");
        fs::create_dir_all(&mod_root).expect("create isolated UGC mod root");
        fs::write(
            mod_root.join("modinfo.lua"),
            r#"name = "Isolated UGC Mod"
configuration_options = {
  {
    name = "enabled",
    label = "Enabled",
    options = {
      { description = "Yes", data = true },
      { description = "No", data = false },
    },
    default = true,
  },
}
"#,
        )
        .expect("write modinfo");

        let specs = read_dst_mod_configuration_specs_with_roots(
            &install_root,
            std::slice::from_ref(&instance_data_root),
            &[String::from("1909182187")],
            "en",
        );
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].status, "loaded");
        assert_eq!(Path::new(&specs[0].mod_dir), mod_root);
        assert_eq!(specs[0].options[0].name, "enabled");

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn reads_configuration_options_from_extra_steamcmd_workshop_root() {
        let root = make_temp_root("steamcmd-cache");
        let install_root = root.join("server");
        let steamcmd_root = root.join("steamcmd");
        let mod_root = steamcmd_root
            .join("steamapps")
            .join("workshop")
            .join("content")
            .join("322330")
            .join("2039181790");
        fs::create_dir_all(&mod_root).expect("create steamcmd workshop mod root");
        fs::write(
            mod_root.join("modinfo.lua"),
            r#"
name = "SteamCMD Cache Mod"
configuration_options = {
  {
    name = "language",
    label = "Language",
    options = {
      { description = "English", data = "en" },
      { description = "Chinese", data = "zh" },
    },
    default = "en",
  },
}
"#,
        )
        .expect("write steamcmd cached modinfo");

        let specs = read_dst_mod_configuration_specs_with_roots(
            &install_root,
            &[steamcmd_root],
            &[String::from("2039181790")],
            "en",
        );
        assert_eq!(specs.len(), 1);
        let spec = &specs[0];
        assert_eq!(spec.status, "loaded");
        assert_eq!(spec.mod_name.as_deref(), Some("SteamCMD Cache Mod"));
        assert_eq!(spec.mod_dir, mod_root.to_string_lossy());
        assert_eq!(spec.options[0].name, "language");

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn reads_client_only_flag_without_hiding_configuration_options() {
        let root = make_temp_root("client-only");
        let mod_root = root.join("mods").join("workshop-1234567890");
        fs::create_dir_all(&mod_root).expect("create client-only fixture");
        let configuration = r#"
configuration_options = {{
    name = "messages", label = "Messages", default = true,
    options = {
        {description = "On", data = true},
        {description = "Off", data = false},
    },
}}
"#;
        for (declaration, expected) in [
            ("local client = true; client_only_mod = client", true),
            ("client_only_mod = false", false),
            ("", false),
        ] {
            fs::write(
                mod_root.join("modinfo.lua"),
                format!("{declaration}\n{configuration}"),
            )
            .expect("write client-only configuration fixture");
            let specs =
                read_dst_mod_configuration_specs(&root, &[String::from("1234567890")], "en");
            assert_eq!(specs[0].client_only, expected, "{declaration}");
            assert_eq!(specs[0].status, "loaded");
            assert_eq!(specs[0].options.len(), 1);
            assert_eq!(specs[0].options[0].name, "messages");
            assert_eq!(specs[0].options[0].options.len(), 2);
            assert_eq!(
                specs[0].options[0].default_value,
                Some(DstModPrimitiveValue::Boolean(true))
            );
            assert_eq!(
                serde_json::to_value(&specs[0]).expect("serialize spec")["client_only"],
                serde_json::Value::Bool(expected)
            );
        }

        fs::write(mod_root.join("modinfo.lua"), "client_only_mod = true")
            .expect("write client-only fixture without options");
        let specs = read_dst_mod_configuration_specs(&root, &[String::from("1234567890")], "en");
        assert!(specs[0].client_only);
        assert_eq!(specs[0].status, "no_options");
        assert!(specs[0].options.is_empty());
        fs::remove_dir_all(root).expect("remove client-only fixture");
    }

    #[test]
    fn reports_missing_mod_folders() {
        let root = make_temp_root("missing");
        let specs = read_dst_mod_configuration_specs(&root, &[String::from("2039181790")], "en");
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].status, "missing_mod");
        assert!(!specs[0].client_only);
        fs::create_dir_all(root.join("mods").join("workshop-2039181790"))
            .expect("create folder without modinfo");
        let specs = read_dst_mod_configuration_specs(&root, &[String::from("2039181790")], "en");
        assert_eq!(specs[0].status, "missing_modinfo");
        assert!(!specs[0].client_only);
        fs::remove_dir_all(root).ok();
    }

    #[test]
    #[ignore = "Requires LGSM_DST_MODINFO_PATH pointing to an operator-selected downloaded modinfo.lua"]
    fn parses_external_modinfo_in_bounded_sandbox() {
        let path = std::env::var_os("LGSM_DST_MODINFO_PATH")
            .map(PathBuf::from)
            .expect("set LGSM_DST_MODINFO_PATH to an existing modinfo.lua");
        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some("modinfo.lua")
        );
        let mod_root = path.parent().expect("modinfo has a parent directory");
        let folder = mod_root
            .file_name()
            .and_then(|name| name.to_str())
            .expect("mod folder name");
        let logical_name = normalize_workshop_id(folder)
            .map(|id| format!("workshop-{id}"))
            .unwrap_or_else(|| folder.to_owned());
        let spec = parse_mod_configuration_spec(mod_root, &path, "zh", &logical_name)
            .expect("read external modinfo through the production bounded sandbox");
        assert_eq!(spec.status, "loaded", "{:?}", spec.message);
        assert!(
            !spec.options.is_empty(),
            "selected modinfo must declare editable options"
        );
        eprintln!(
            "external modinfo sandbox: status={}, options={}",
            spec.status,
            spec.options.len()
        );
    }
}
