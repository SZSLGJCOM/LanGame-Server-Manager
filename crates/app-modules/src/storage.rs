use serde::Deserialize;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModuleProgramSharing {
    #[default]
    Independent,
    Shared,
}

#[derive(Debug, Clone, Default)]
pub struct ModuleStorageSpec {
    /// Whether an unchanged program version may serve multiple instances.
    /// This capability does not describe an instance's selected version or loader.
    pub program_sharing: ModuleProgramSharing,
    pub saves_path_template: Option<String>,
    /// Files or directories retained during uninstall, relative to the install root.
    /// These are literal paths, independent of instance-creation isolation.
    pub retained_paths: Vec<String>,
    /// Instance-owned native paths omitted when copying the shared package
    /// into a new private runtime.
    pub runtime_copy_exclusions: Vec<String>,
}

impl ModuleStorageSpec {
    /// Validate without filesystem access so discovery and lifecycle callers use
    /// the same rules. Filesystem containment and link checks belong to removal.
    pub fn validate_retained_paths(&self) -> Result<(), String> {
        validate_literal_paths(&self.retained_paths, "retained path")
    }

    pub fn validate_runtime_copy_exclusions(&self) -> Result<(), String> {
        validate_literal_paths(&self.runtime_copy_exclusions, "runtime copy exclusion")
    }
}

fn validate_literal_paths(paths: &[String], label: &str) -> Result<(), String> {
    for relative in paths {
        // Recognize Windows separators and prefixes on every host. Splitting
        // the original string also rejects dot components normalized by Path.
        if relative.is_empty()
            || relative.trim() != relative
            || relative.chars().any(|character| {
                character.is_control()
                    || matches!(
                        character,
                        ':' | '*' | '?' | '"' | '<' | '>' | '|' | '{' | '}'
                    )
            })
            || relative.split(['/', '\\']).any(|component| {
                component.is_empty()
                    || matches!(component, "." | "..")
                    || component.ends_with(['.', ' '])
            })
        {
            return Err(format!(
                "{label} {relative:?} must be a literal install-root-relative file or directory with normal path components"
            ));
        }
    }
    Ok(())
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct ModuleTomlStorage {
    #[serde(default)]
    program_sharing: ModuleProgramSharing,
    saves_path_template: Option<String>,
    #[serde(default)]
    retained_paths: Vec<String>,
    #[serde(default)]
    runtime_copy_exclusions: Vec<String>,
}

pub(super) fn storage_spec_from_toml(
    storage: Option<ModuleTomlStorage>,
) -> Result<ModuleStorageSpec, String> {
    let storage = storage.unwrap_or_default();
    let storage = ModuleStorageSpec {
        program_sharing: storage.program_sharing,
        saves_path_template: storage.saves_path_template,
        retained_paths: storage.retained_paths,
        runtime_copy_exclusions: storage.runtime_copy_exclusions,
    };
    storage.validate_retained_paths()?;
    storage.validate_runtime_copy_exclusions()?;
    Ok(storage)
}

pub(super) fn validate_program_sharing(
    storage: &ModuleStorageSpec,
    install: Option<&app_core::InstallSpec>,
    process: Option<&app_core::ProcessSpec>,
    mods: Option<&toml::Value>,
) -> Result<(), String> {
    if storage.program_sharing == ModuleProgramSharing::Independent {
        return Ok(());
    }
    if install.is_none() {
        return Err("shared programs require an install specification".into());
    }
    let process = process.ok_or("shared programs require a process specification")?;
    let executable = process
        .executable
        .strip_prefix("{{paths.install_root}}/")
        .or_else(|| process.executable.strip_prefix("{{paths.install_root}}\\"))
        .unwrap_or(&process.executable);
    validate_literal_paths(&[executable.to_owned()], "shared program executable")?;
    if !storage.retained_paths.is_empty() || !storage.runtime_copy_exclusions.is_empty() {
        return Err(
            "shared programs cannot declare install-root retained paths or runtime copy exclusions"
                .into(),
        );
    }
    let working_directory = process
        .working_directory_template
        .as_deref()
        .ok_or("shared programs require an instance-owned working directory")?;
    validate_instance_path_template(working_directory, "shared program working directory")?;
    let saves = storage
        .saves_path_template
        .as_deref()
        .ok_or("shared programs require an instance-owned saves path")?;
    validate_instance_path_template(saves, "shared program saves path")?;
    if let Some(mods) = mods {
        let mods = mods
            .as_table()
            .ok_or("shared program mods must be a table")?;
        if let Some(staging) = mods.get("manual_staging") {
            let target = staging
                .as_table()
                .and_then(|staging| staging.get("target_template"))
                .and_then(toml::Value::as_str)
                .ok_or("shared program mod staging requires a target template")?;
            validate_instance_path_template(target, "shared program mod staging target")?;
        }
    }
    Ok(())
}

fn validate_instance_path_template(template: &str, label: &str) -> Result<(), String> {
    let invalid = || format!("{label} must remain inside an instance-owned path");
    let suffix = [
        "{{paths.instance_root}}",
        "{{paths.config_dir}}",
        "{{paths.data_dir}}",
        "{{paths.logs_dir}}",
    ]
    .into_iter()
    .find_map(|root| template.strip_prefix(root))
    .ok_or_else(invalid)?;
    if suffix.is_empty() {
        return Ok(());
    }
    let suffix = suffix
        .strip_prefix('/')
        .or_else(|| suffix.strip_prefix('\\'))
        .ok_or_else(invalid)?;

    // Check literal traversal here; lifecycle callers must also validate the
    // resolved path because a settings value can contain path components.
    let mut literal = String::new();
    let mut remaining = suffix;
    while let Some(start) = remaining.find("{{") {
        literal.push_str(&remaining[..start]);
        let token_start = &remaining[start + 2..];
        let end = token_start.find("}}").ok_or_else(invalid)?;
        let token = &token_start[..end];
        let setting_key = token.strip_prefix("settings.");
        if token != "instance.id"
            && !setting_key.is_some_and(|key| {
                !key.is_empty()
                    && key
                        .chars()
                        .all(|character| character.is_ascii_alphanumeric() || character == '_')
            })
        {
            return Err(invalid());
        }
        literal.push_str("instance-value");
        remaining = &token_start[end + 2..];
    }
    literal.push_str(remaining);
    validate_literal_paths(&[literal], label)
}

#[cfg(test)]
#[path = "storage_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "program_sharing_tests.rs"]
mod program_sharing_tests;
