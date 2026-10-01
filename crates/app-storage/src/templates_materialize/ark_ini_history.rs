use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub(super) const OWNERSHIP_FILE: &str = "ark-native-ownership.json";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Ownership {
    config: BTreeMap<String, Vec<NativeKey>>,
    live: BTreeMap<String, Vec<NativeKey>>,
}

pub(super) enum Target {
    Config,
    Live,
}

pub(super) struct Snapshot {
    path: PathBuf,
    original: Option<Vec<u8>>,
    ownership: Ownership,
}

impl Snapshot {
    pub(super) fn load(
        config_dir: &Path,
        config_path: &Path,
        module_id: &str,
    ) -> Result<Self, StorageError> {
        let path = config_dir.join(OWNERSHIP_FILE);
        let original = read_optional_bytes(&path)?;
        let ownership: Ownership = if let Some(bytes) = &original {
            serde_json::from_slice(bytes).map_err(|error| {
                materialization_error(
                    module_id,
                    &path,
                    format!("invalid ARK native ownership: {error}"),
                )
            })?
        } else {
            let previous_json = crate::instances::read_instance_settings_json(config_path)?;
            let settings: Map<String, Value> =
                serde_json::from_str(&previous_json).map_err(|error| {
                    materialization_error(
                        module_id,
                        config_path,
                        format!("invalid prior ARK settings: {error}"),
                    )
                })?;
            let mut keys = BTreeMap::new();
            for (filename, game_ini) in [
                (ARK_GAME_INI_FILE, true),
                (ARK_GAME_USER_SETTINGS_FILE, false),
            ] {
                let mut entries = extra_document(&settings, game_ini)
                    .map_err(|error| materialization_error(module_id, config_path, error))?
                    .keys()
                    .into_iter()
                    .collect::<Vec<_>>();
                entries.sort();
                keys.insert(filename.to_owned(), entries);
            }
            Ownership {
                config: keys.clone(),
                live: keys,
            }
        };
        for target in [&ownership.config, &ownership.live] {
            if target.len() != 2
                || !target.contains_key(ARK_GAME_INI_FILE)
                || !target.contains_key(ARK_GAME_USER_SETTINGS_FILE)
                || target.values().flatten().any(|(section, key)| {
                    section.is_empty()
                        || key.is_empty()
                        || section.chars().any(char::is_control)
                        || key.chars().any(char::is_control)
                })
            {
                return Err(materialization_error(
                    module_id,
                    &path,
                    String::from("ARK native ownership must contain two INI key inventories"),
                ));
            }
        }
        Ok(Self {
            path,
            original,
            ownership,
        })
    }

    pub(super) fn keys(&self, filename: &str, target: Target) -> HashSet<NativeKey> {
        let source = match target {
            Target::Config => &self.ownership.config,
            Target::Live => &self.ownership.live,
        };
        source
            .get(filename)
            .into_iter()
            .flatten()
            .cloned()
            .collect()
    }

    pub(super) fn record(&mut self, filename: &str, target: Target, keys: HashSet<NativeKey>) {
        let source = match target {
            Target::Config => &mut self.ownership.config,
            Target::Live => &mut self.ownership.live,
        };
        let mut keys = keys.into_iter().collect::<Vec<_>>();
        keys.sort();
        source.insert(filename.to_owned(), keys);
    }

    pub(super) fn plan(self) -> Result<ManagedConfigMergePlan, StorageError> {
        Ok(ManagedConfigMergePlan {
            destination_path: self.path,
            replacement: serde_json::to_vec_pretty(&self.ownership)?,
            original: self.original,
        })
    }
}

// Running saves must capture the last materialized ownership before instance.json
// changes. The initial inventory comes from raw extra; subsequent snapshots track
// actual template output. Unknown preserved entries are never added to ownership.
// Config and live snapshots advance separately, only with their file writes.
pub(in crate::templates::templates_materialize) fn capture_previous_ownership(
    context: &ModuleSupportMaterializationContext<'_>,
    config_path: &Path,
    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    if !matches!(
        context.module_id,
        "arksurvivalascended" | "arksurvivalevolved"
    ) {
        return Ok(());
    }
    let snapshot = Snapshot::load(context.config_dir, config_path, context.module_id)?;
    if snapshot.original.is_none() {
        files.apply(vec![snapshot.plan()?])?;
    }
    Ok(())
}
