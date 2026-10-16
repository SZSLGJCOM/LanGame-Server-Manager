use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::StorageError;
use crate::templates::{ManagedConfigMergePlan, ManagedConfigMutation};

#[path = "enshrouded_bans_entries.rs"]
mod entries;
#[path = "enshrouded_bans_roster.rs"]
mod roster;

const CONFIG_FILE: &str = "enshrouded_server.json";
const NATIVE_KEY: &str = "bannedAccounts";
const RETIRED_KEY: &str = "bans";

pub(crate) fn parse_enshrouded_ban_records(
    document: &Map<String, Value>,
    path: &Path,
) -> Result<std::collections::BTreeMap<u64, Value>, StorageError> {
    entries::parse(document, path)
}

struct DocumentSnapshot {
    path: PathBuf,
    content: Option<Vec<u8>>,
    document: Map<String, Value>,
}

/// Both native and previously rendered JSON can contain irreplaceable ban
/// metadata. A settings string is an editor projection, never their replacement.
pub(crate) struct EnshroudedBansSnapshot {
    native: DocumentSnapshot,
    managed: DocumentSnapshot,
    records: roster::Records,
}

impl EnshroudedBansSnapshot {
    pub(crate) fn read(
        module_id: &str,
        install_root: &Path,
        config_dir: &Path,
    ) -> Result<Option<Self>, StorageError> {
        if module_id != "enshrouded" {
            return Ok(None);
        }
        let native = DocumentSnapshot::read(install_root.join(CONFIG_FILE))?;
        let managed = DocumentSnapshot::read(config_dir.join(CONFIG_FILE))?;
        let mut records = entries::parse(&native.document, &native.path)?;
        for (id, record) in entries::parse(&managed.document, &managed.path)? {
            if let Some(current) = records.get_mut(&id) {
                roster::merge_record(current, record)
                    .map_err(|message| invalid(&managed.path, message))?;
            } else {
                records.insert(id, record);
            }
        }
        Ok(Some(Self {
            native,
            managed,
            records,
        }))
    }

    pub(crate) fn project(&self, settings: &mut Map<String, Value>) -> Result<(), StorageError> {
        let mut ids = roster::settings_ids(settings)?;
        ids.extend(self.records.keys().copied());
        roster::set_ids(settings, &ids);
        Ok(())
    }

    pub(crate) fn reconcile(
        &self,
        instance_id: &str,
        persisted: &Map<String, Value>,
        incoming: &mut Map<String, Value>,
        expected: Option<&str>,
        removed: Option<u64>,
    ) -> Result<(), StorageError> {
        roster::reconcile_ids(
            instance_id,
            self.records.keys().copied(),
            persisted,
            incoming,
            expected,
            removed,
        )
    }

    pub(crate) fn verify_unchanged(&self, instance_id: &str) -> Result<(), StorageError> {
        for original in [&self.native, &self.managed] {
            if read_optional(&original.path)? != original.content {
                return Err(StorageError::InstanceSettingsPreconditionFailed {
                    id: instance_id.to_owned(),
                });
            }
        }
        Ok(())
    }

    /// Called after rendering the managed JSON, with the validated complete ID
    /// set. CAS still compares the native file against its pre-render snapshot.
    pub(crate) fn materialize(
        &self,
        rendered_path: &Path,
        settings: &Map<String, Value>,
        files: &mut ManagedConfigMutation,
    ) -> Result<(), StorageError> {
        let rendered = DocumentSnapshot::read(rendered_path.to_owned())?;
        let rendered_bytes = rendered
            .content
            .as_deref()
            .ok_or_else(|| invalid(rendered_path, "Rendered native configuration is missing"))?;
        files.verify_published_source(
            rendered_path,
            rendered_bytes,
            self.managed.content.as_deref(),
        )?;
        let generated = entries::parse(&rendered.document, rendered_path)?;
        let selected = roster::settings_ids(settings)?;
        let mut records = Vec::with_capacity(selected.len());
        for id in &selected {
            let record = self
                .records
                .get(id)
                .or_else(|| generated.get(id))
                .ok_or_else(|| {
                    invalid(
                        rendered_path,
                        "A selected native ban has no serialized record",
                    )
                })?;
            records.push(record.clone());
        }
        let mut merged = self.native.document.clone();
        let mut rendered_configuration = rendered.document;
        rendered_configuration.remove(NATIVE_KEY);
        rendered_configuration.remove(RETIRED_KEY);
        merge_configuration(&mut merged, rendered_configuration);
        merged.remove(RETIRED_KEY);
        merged.insert(NATIVE_KEY.to_owned(), Value::Array(records));
        let mut replacement = serde_json::to_vec_pretty(&Value::Object(merged.clone()))?;
        replacement.push(b'\n');
        if replacement.len() > crate::instance_file_patch::MAX_FILE_BYTES {
            return Err(invalid(
                &self.native.path,
                "Merged native configuration exceeds the safe 256 KiB read/write limit",
            ));
        }
        let needs_recovery = self.native.document.contains_key(RETIRED_KEY)
            || self.managed.document.contains_key(RETIRED_KEY)
            || self.native.document.get(NATIVE_KEY) != merged.get(NATIVE_KEY);
        if read_optional(&self.native.path)? != self.native.content {
            return Err(invalid(
                &self.native.path,
                "Native configuration changed while this save was pending; reload before retrying",
            ));
        }
        let mut plans = if needs_recovery {
            self.recovery_plans()?
        } else {
            Vec::new()
        };
        // Native typed serialization drops unknown record members. Keep the
        // complete canonical records in the existing managed JSON as well, so
        // a later native rewrite cannot erase their only current copy. The
        // source CAS uses the already validated rendered bytes, including when
        // rendering was a no-op and created no mutation history entry.
        plans.push(ManagedConfigMergePlan {
            destination_path: rendered_path.to_owned(),
            original: Some(rendered_bytes.to_vec()),
            replacement: replacement.clone(),
        });
        plans.push(ManagedConfigMergePlan {
            destination_path: self.native.path.clone(),
            original: self.native.content.clone(),
            replacement,
        });
        files.apply(plans)
    }

    fn recovery_plans(&self) -> Result<Vec<ManagedConfigMergePlan>, StorageError> {
        if self.native.content.is_none() && self.managed.content.is_none() {
            return Ok(Vec::new());
        }
        let mut digest = Sha256::new();
        for document in [&self.native, &self.managed] {
            let content = document.content.as_deref().unwrap_or_default();
            digest.update((content.len() as u64).to_le_bytes());
            digest.update(content);
        }
        let parent =
            self.managed.path.parent().ok_or_else(|| {
                invalid(&self.managed.path, "Managed configuration has no parent")
            })?;
        let recovery = parent.join("native-ban-recovery").join(
            digest
                .finalize()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>(),
        );
        let _parent_guards = crate::instance_file_patch::io::guard_directories(parent)?;
        for directory in [parent.join("native-ban-recovery"), recovery.clone()] {
            match std::fs::create_dir(&directory) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(source) => {
                    return Err(StorageError::CreatePath {
                        path: directory,
                        source,
                    });
                }
            }
            crate::instance_file_patch::io::guard_directories(&directory)?;
        }
        let _guards = crate::instance_file_patch::io::guard_directories(&recovery)?;
        let mut plans = Vec::new();
        for (name, document) in [
            ("native-before.json", &self.native),
            ("managed-before.json", &self.managed),
        ] {
            let Some(content) = &document.content else {
                continue;
            };
            let destination = recovery.join(name);
            match read_optional(&destination)? {
                Some(existing) if existing == *content => {}
                Some(_) => {
                    return Err(invalid(
                        &destination,
                        "Recovery copy differs from its source; refusing to overwrite it",
                    ));
                }
                None => plans.push(ManagedConfigMergePlan {
                    destination_path: destination,
                    original: None,
                    replacement: content.clone(),
                }),
            }
        }
        Ok(plans)
    }

    pub(crate) fn selected_ids(
        settings: &Map<String, Value>,
    ) -> Result<BTreeSet<u64>, StorageError> {
        roster::settings_ids(settings)
    }
}

impl DocumentSnapshot {
    fn read(path: PathBuf) -> Result<Self, StorageError> {
        let content = read_optional(&path)?;
        let document = match content.as_deref() {
            Some(bytes) => serde_json::from_slice::<Value>(bytes)
                .map_err(|_| invalid(&path, "Native configuration must contain valid JSON"))?
                .as_object()
                .cloned()
                .ok_or_else(|| invalid(&path, "Native configuration must contain a JSON object"))?,
            None => Map::new(),
        };
        Ok(Self {
            path,
            content,
            document,
        })
    }
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, StorageError> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => crate::instance_file_patch::io::read_bytes(path).map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(StorageError::ReadPath {
            path: path.to_owned(),
            source,
        }),
    }
}

fn merge_configuration(existing: &mut Map<String, Value>, rendered: Map<String, Value>) {
    for (key, value) in rendered {
        if let (Some(Value::Object(current)), Value::Object(incoming)) =
            (existing.get_mut(&key), &value)
        {
            merge_configuration(current, incoming.clone());
        } else {
            existing.insert(key, value);
        }
    }
}

fn invalid(path: &Path, message: impl Into<String>) -> StorageError {
    StorageError::ModuleSupportMaterialization {
        module_id: "enshrouded".to_owned(),
        path: path.to_owned(),
        message: message.into(),
    }
}

#[cfg(test)]
#[path = "enshrouded_bans_tests.rs"]
mod tests;
