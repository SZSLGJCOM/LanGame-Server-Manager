use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::*;
use crate::atomic_file::{compare_and_swap_optional_file_atomically, create_file_atomically};
use crate::instance_file_patch::io::is_link;

pub(super) const FILE: &str = "dragonwilds-world-settings.pending.json";
pub(super) const MAX_DOCUMENT: usize = 256 * 1024;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Pending {
    pub version: u32,
    pub instance_id: String,
    pub world_mode: DragonwildsWorldMode,
    pub overrides: BTreeMap<String, f32>,
}

pub(super) struct StoredPending {
    pub value: Pending,
    pub bytes: Vec<u8>,
}

impl Pending {
    pub fn native(&self, world_name: &str) -> NativeWorldSettings {
        NativeWorldSettings {
            world_name: world_name.to_owned(),
            world_mode: self.world_mode.native_value(),
            hardcore_state: 0,
            overrides: self.overrides.clone(),
        }
    }
}

pub(super) fn read_document(path: &Path) -> Result<Option<Vec<u8>>, StorageError> {
    let parent = path
        .parent()
        .ok_or_else(|| invalid(path, "Missing document directory."))?;
    let _guards = guard_directories(parent)?;
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(invalid(path, error.to_string())),
    };
    if !metadata.is_file() || is_link(&metadata) || metadata.len() > MAX_DOCUMENT as u64 {
        return Err(invalid(
            path,
            "World initialization documents must be ordinary files below 256 KiB.",
        ));
    }
    let mut options = File::options();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x00200000); // FILE_FLAG_OPEN_REPARSE_POINT
    }
    let file = options
        .open(path)
        .map_err(|error| invalid(path, error.to_string()))?;
    let metadata = file
        .metadata()
        .map_err(|error| invalid(path, error.to_string()))?;
    if !metadata.is_file() || is_link(&metadata) || metadata.len() > MAX_DOCUMENT as u64 {
        return Err(invalid(
            path,
            "World initialization document changed while opening it.",
        ));
    }
    let mut bytes = Vec::new();
    file.take((MAX_DOCUMENT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| invalid(path, error.to_string()))?;
    if bytes.len() > MAX_DOCUMENT {
        return Err(invalid(
            path,
            "World initialization document exceeds 256 KiB.",
        ));
    }
    Ok(Some(bytes))
}

pub(super) fn validate(
    value: &Pending,
    instance_id: &str,
    path: &Path,
) -> Result<(), StorageError> {
    if value.version != 1 || value.instance_id != instance_id {
        return Err(invalid(
            path,
            "World draft does not belong to this instance or uses an unsupported version.",
        ));
    }
    let definitions = definitions()?;
    if value.overrides.len() > definitions.len() {
        return Err(invalid(path, "Too many saved world rules."));
    }
    for (tag, value) in &value.overrides {
        let definition = definitions
            .iter()
            .find(|definition| &definition.tag == tag)
            .ok_or_else(|| invalid(path, "Saved world draft contains an unknown rule."))?;
        // Locked values enter a draft only through a native preset conversion.
        // One native internal default is outside its exposed slider range.
        if !definition.editable_in(DragonwildsWorldMode::Custom) {
            if definition
                .preset_defaults
                .values()
                .any(|preset| *preset as f32 == *value)
            {
                continue;
            }
            return Err(invalid(
                path,
                format!("Saved locked world rule {tag} is not a native preset value."),
            ));
        }
        let step = 10_f64.powi(-(definition.decimal_places as i32));
        let rounded = ((f64::from(*value) / step).round() * step) as f32;
        if !value.is_finite()
            || *value < definition.minimum as f32
            || *value > definition.maximum as f32
            || rounded != *value
            || (definition.kind == "boolean" && *value != 0.0 && *value != 1.0)
        {
            return Err(invalid(
                path,
                format!("Saved world rule {tag} violates its native range or precision."),
            ));
        }
    }
    Ok(())
}

pub(super) fn load(
    context: &WorldContext,
    instance_id: &str,
) -> Result<Option<StoredPending>, StorageError> {
    let path = context.config.join(FILE);
    let Some(bytes) = read_document(&path)? else {
        return Ok(None);
    };
    let value: Pending = serde_json::from_slice(&bytes)
        .map_err(|error| invalid(&path, format!("World draft is malformed: {error}")))?;
    validate(&value, instance_id, &path)?;
    Ok(Some(StoredPending { value, bytes }))
}

fn empty(instance_id: &str) -> Pending {
    Pending {
        version: 1,
        instance_id: instance_id.to_owned(),
        world_mode: DragonwildsWorldMode::Normal,
        overrides: BTreeMap::new(),
    }
}

fn revision(instance_id: &str, bytes: Option<&[u8]>) -> Result<String, StorageError> {
    // A new-world draft belongs to the instance. Basic configuration may rename
    // its world before startup without invalidating a simultaneous rule save.
    Ok(sha256(&serde_json::to_vec(&(
        "dragonwilds-draft-v1",
        instance_id,
        bytes,
    ))?))
}

pub(super) fn snapshot(
    instance_id: &str,
    context: &WorldContext,
    stored: Option<StoredPending>,
    definitions: Vec<DragonwildsWorldSettingDefinition>,
) -> Result<DragonwildsWorldSettingsSnapshot, StorageError> {
    let journal = read_document(&context.config.join(bootstrap::JOURNAL))?;
    if journal.is_some() && stored.is_none() {
        return Err(invalid(
            &context.config,
            "World initialization recovery is pending. Start the instance to finish recovery.",
        ));
    }
    if journal.is_none() && !io::world_files(&context.saves)?.is_empty() {
        return Err(invalid(
            &context.saves,
            "An existing world was imported after these rules were staged. The imported save and draft were preserved; they cannot be initialized as a new world.",
        ));
    }
    let default = empty(instance_id);
    let draft = stored.as_ref().map_or(&default, |stored| &stored.value);
    let values = effective_values(
        &draft.native(&context.default_world_name),
        draft.world_mode,
        &definitions,
    )?;
    Ok(DragonwildsWorldSettingsSnapshot {
        instance_id: instance_id.to_owned(),
        status: "new".into(),
        world_file: None,
        world_name: Some(context.default_world_name.clone()),
        world_mode: Some(draft.world_mode),
        revision: Some(revision(instance_id, stored.as_ref().map(|stored| stored.bytes.as_slice()))?),
        values,
        overrides: draft.overrides.iter().map(|(tag, value)| (tag.clone(), f64::from(*value))).collect(),
        definitions,
        writable: context.writable && journal.is_none(),
        message: journal.is_some().then(|| "World initialization is awaiting completion. Start the instance to resume; its saved rules and generated world are preserved.".into()),
        backup_id: None,
    })
}

pub(super) fn write(
    context: &WorldContext,
    input: &WriteDragonwildsWorldSettingsInput,
) -> Result<DragonwildsWorldSettingsSnapshot, StorageError> {
    if read_document(&context.config.join(bootstrap::JOURNAL))?.is_some() {
        return Err(invalid(
            &context.config,
            "World initialization has already started. Resume it before changing the staged rules.",
        ));
    }
    if !io::world_files(&context.saves)?.is_empty() {
        return Err(invalid(
            &context.saves,
            "A generated or imported world already exists. Reload world settings before saving.",
        ));
    }
    let stored = load(context, &input.instance_id)?;
    let original = stored.as_ref().map(|stored| stored.bytes.as_slice());
    if input.expected_revision != revision(&input.instance_id, original)? {
        return Err(invalid(
            &context.config,
            "World draft changed outside this editor. Your draft is retained; reload before saving.",
        ));
    }
    let default = empty(&input.instance_id);
    let draft = stored.as_ref().map_or(&default, |stored| &stored.value);
    let path = context.config.join(FILE);
    let next = Pending {
        version: 1,
        instance_id: input.instance_id.clone(),
        world_mode: input.world_mode,
        overrides: changed_overrides(
            &path,
            &draft.native(&context.default_world_name),
            input.world_mode,
            &input.values,
        )?,
    };
    let bytes = serde_json::to_vec_pretty(&next)?;
    let _guards = guard_directories(&context.config)?;
    if original.is_none() {
        create_file_atomically(&path, &bytes).map_err(|error| invalid(&path, error.to_string()))?;
    } else if !compare_and_swap_optional_file_atomically(&path, original, Some(&bytes))
        .map_err(|error| invalid(&path, error.to_string()))?
    {
        return Err(invalid(
            &path,
            "World draft changed while saving. Reload before retrying.",
        ));
    }
    snapshot(
        &input.instance_id,
        context,
        Some(StoredPending { value: next, bytes }),
        definitions()?,
    )
}
