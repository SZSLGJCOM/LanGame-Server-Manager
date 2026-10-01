use crate::{InstanceStatus, Map, Path, PathBuf, StorageError, Value};

pub(crate) const MORIA_PERMISSIONS_FILE: &str = "MoriaServerPermissions.txt";
const PERMISSIONS_KEY: &str = "permissions_lines";

/// The game adds visiting players to this file; instance.json is not its owner.
pub(crate) struct MoriaPermissionsSnapshot {
    path: PathBuf,
    content: Option<String>,
}

impl MoriaPermissionsSnapshot {
    pub(crate) fn read(module_id: &str, install_root: &Path) -> Result<Option<Self>, StorageError> {
        if module_id != "returntomoria" {
            return Ok(None);
        }
        let path = install_root.join(MORIA_PERMISSIONS_FILE);
        let content = read_permissions(&path)?;
        Ok(Some(Self { path, content }))
    }

    pub(crate) fn project(&self, settings: &mut Map<String, Value>) {
        if let Some(content) = &self.content {
            settings.insert(PERMISSIONS_KEY.to_owned(), Value::String(content.clone()));
        }
    }

    pub(crate) fn reconcile(
        &self,
        instance_id: &str,
        persisted: &Map<String, Value>,
        incoming: &mut Map<String, Value>,
        expected: Option<&str>,
    ) -> Result<(), StorageError> {
        let current = self.current_value(persisted);
        if let Some(expected) = expected {
            let expected: Value = serde_json::from_str(expected)?;
            let expected = expected
                .as_object()
                .ok_or(StorageError::InvalidSettingsRoot)?;
            if expected.get(PERMISSIONS_KEY) != current.as_ref() {
                return Err(precondition(instance_id));
            }
            if incoming.get(PERMISSIONS_KEY) == expected.get(PERMISSIONS_KEY) {
                self.project(incoming);
            }
        } else if incoming.get(PERMISSIONS_KEY) == persisted.get(PERMISSIONS_KEY) {
            self.project(incoming);
        } else if current.as_ref() != persisted.get(PERMISSIONS_KEY)
            && incoming.get(PERMISSIONS_KEY) != current.as_ref()
        {
            // A caller without a baseline cannot distinguish a deletion from a
            // stale copy of a roster the game has since extended.
            return Err(precondition(instance_id));
        }
        Ok(())
    }

    fn current_value(&self, persisted: &Map<String, Value>) -> Option<Value> {
        self.content
            .as_ref()
            .map(|content| Value::String(content.clone()))
            .or_else(|| persisted.get(PERMISSIONS_KEY).cloned())
    }

    pub(crate) fn verify_unchanged(&self, instance_id: &str) -> Result<(), StorageError> {
        if read_permissions(&self.path)? != self.content {
            return Err(precondition(instance_id));
        }
        Ok(())
    }

    pub(crate) fn into_write_plan(
        self,
        settings: &Map<String, Value>,
    ) -> crate::templates::ManagedConfigMergePlan {
        crate::templates::ManagedConfigMergePlan {
            destination_path: self.path,
            original: self.content.map(String::into_bytes),
            replacement: string_setting(settings, PERMISSIONS_KEY)
                .as_bytes()
                .to_vec(),
        }
    }
}

fn precondition(instance_id: &str) -> StorageError {
    StorageError::InstanceSettingsPreconditionFailed {
        id: instance_id.to_owned(),
    }
}

fn read_permissions(path: &Path) -> Result<Option<String>, StorageError> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(StorageError::ReadPath {
                path: path.to_owned(),
                source,
            });
        }
        Ok(_) => {}
    }
    // The existing reader rejects links/reparse points and caps reads at 256 KiB.
    let bytes = crate::instance_file_patch::io::read_bytes(path)?;
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| StorageError::ModuleSupportMaterialization {
            module_id: "returntomoria".to_owned(),
            path: path.to_owned(),
            message: "The native permissions file must contain UTF-8 text".to_owned(),
        })
}

pub(crate) fn reject_active_sensitive_changes(
    module_id: &str,
    status: &InstanceStatus,
    has_active_run: bool,
    active_process_count: usize,
    persisted: &Map<String, Value>,
    incoming: &Map<String, Value>,
    permissions: Option<&MoriaPermissionsSnapshot>,
) -> Result<(), StorageError> {
    if !has_active_run && active_process_count == 0 && matches!(status, InstanceStatus::Stopped) {
        return Ok(());
    }
    let changed = match module_id {
        "returntomoria" => {
            let current = permissions
                .and_then(|snapshot| snapshot.current_value(persisted))
                .or_else(|| persisted.get(PERMISSIONS_KEY).cloned());
            if incoming.get(PERMISSIONS_KEY) != current.as_ref() {
                Some(PERMISSIONS_KEY)
            } else if string_setting(persisted, "upgrade_optional_dlc_array")
                != string_setting(incoming, "upgrade_optional_dlc_array")
            {
                Some("upgrade_optional_dlc_array")
            } else {
                None
            }
        }
        "scum" => ["partial_wipe", "gold_wipe", "full_wipe"]
            .into_iter()
            .find(|key| wipe_setting(persisted, key) != wipe_setting(incoming, key)),
        _ => None,
    };
    if let Some(key) = changed {
        return Err(StorageError::InvalidModuleSetting {
            module_id: module_id.to_owned(),
            field: if module_id == "scum" {
                format!("server_general.{key}")
            } else {
                key.to_owned()
            },
            message: "Stop the server before changing this setting".to_owned(),
        });
    }
    Ok(())
}

fn string_setting<'a>(settings: &'a Map<String, Value>, key: &str) -> &'a str {
    settings.get(key).and_then(Value::as_str).unwrap_or("")
}

fn wipe_setting(settings: &Map<String, Value>, key: &str) -> bool {
    settings
        .get("server_general")
        .and_then(|general| general.get(key))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}
