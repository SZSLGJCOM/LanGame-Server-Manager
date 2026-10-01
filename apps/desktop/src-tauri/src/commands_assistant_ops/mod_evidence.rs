const ASSISTANT_DST_EVIDENCE_GUIDANCE: &str = "For DST, modinfo priority controls loading (higher first); modoverrides table order does not. Inspect installed metadata, current enablement and the error log. Missing optional modworldgenmain.lua is normal. A MOD ERROR is a candidate to investigate, not a proven cause. mod_dependencies false names directories, true names display names, and workshop identifies a Workshop candidate; false does not mean disabled. Dedicated servers do not automatically enable dependencies. Confirm candidates are installed before proposing enablement.";

fn assistant_mod_declared_enablement(projection: &Value, name: &str) -> Value {
    let states = [
        ("declaredEnabledModNames", "enabled"),
        ("declaredDisabledModNames", "disabled"),
        ("declaredUnspecifiedModNames", "unspecified"),
    ];
    json!(
        projection["shards"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|shard| {
                let names = states
                    .iter()
                    .map(|(key, _)| shard[*key].as_array())
                    .collect::<Option<Vec<_>>>();
                let state = names
                    .filter(|lists| {
                        shard["analysisStatus"] == "known"
                            && lists.iter().all(|list| list.iter().all(Value::is_string))
                    })
                    .map_or("unknown", |lists| {
                        if let Some(index) = lists
                            .iter()
                            .position(|list| list.iter().any(|value| value == name))
                        {
                            return states[index].1;
                        }
                        // The engine may resolve a bare Workshop ID to its prefixed
                        // directory, depending on installed copies. Do not guess it.
                        if name.strip_prefix("workshop-").is_some_and(|id| {
                            lists
                                .iter()
                                .any(|list| list.iter().any(|value| value == id))
                        }) {
                            "unknown"
                        } else {
                            "not_declared"
                        }
                    });
                json!({"shard": shard["shard"], "active": shard["active"],
            "matching": "exact_directory", "declaredState": state})
            })
            .collect::<Vec<_>>()
    )
}

// This field is produced only by the selected DST runtime reader, never parsed
// from a model request. A log marker triggers evidence collection, not a mutation.
fn assistant_mod_error_follow_up(result: &Value) -> Option<Vec<String>> {
    let names = result.get("data")?.get("modErrorNames")?.as_array()?;
    if result.get("ok")?.as_bool() != Some(true) || names.is_empty() || names.len() > 5 {
        return None;
    }
    names
        .iter()
        .map(|name| name.as_str().map(String::from))
        .collect()
}

#[derive(Default)]
struct AssistantModEvidenceFollowUp {
    error_inspected: bool,
    state_read: bool,
    dependencies_inspected: bool,
    inspected_names: std::collections::BTreeSet<String>,
}

impl AssistantModEvidenceFollowUp {
    fn observe(
        &mut self,
        request: &AssistantReadTool,
        result: &Value,
        reads: usize,
        pending: &mut std::collections::VecDeque<AssistantReadTool>,
    ) {
        self.state_read |= matches!(request, AssistantReadTool::ReadModState {});
        let available = ASSISTANT_INVESTIGATION_STEPS.saturating_sub(reads + pending.len());
        if matches!(request, AssistantReadTool::ReadRuntime { .. })
            && !self.error_inspected
            && available > 0
            && let Some(names) = assistant_mod_error_follow_up(result)
        {
            self.error_inspected = true;
            if !self.state_read && available >= 2 {
                pending.push_back(AssistantReadTool::ReadModState {});
            }
            pending.push_back(AssistantReadTool::InspectInstalledMods { names, offset: 0 });
        }
        let AssistantReadTool::InspectInstalledMods { names, .. } = request else {
            return;
        };
        self.inspected_names.extend(names.iter().cloned());
        if result.get("ok").and_then(Value::as_bool) != Some(true) {
            return;
        }
        let Some(entries) = result.pointer("/data/entries").and_then(Value::as_array) else {
            return;
        };
        // Inventory pages can already contain a declared dependency. Failed reads
        // remain evidence gaps; automatic collection must not silently retry them.
        self.inspected_names
            .extend(entries.iter().filter_map(|entry| {
                entry
                    .get("folderName")
                    .and_then(Value::as_str)
                    .map(String::from)
            }));
        if self.dependencies_inspected || available == 0 {
            return;
        }
        let queued = pending
            .iter()
            .filter_map(|request| match request {
                AssistantReadTool::InspectInstalledMods { names, .. } => Some(names),
                _ => None,
            })
            .flatten()
            .collect::<std::collections::BTreeSet<_>>();
        let mut names = Vec::new();
        for entry in entries.iter().filter(|entry| entry["status"] == "read") {
            for name in crate::dst_mods::dst_mod_dependency_names(&entry["metadata"]) {
                if !self.inspected_names.contains(&name)
                    && !queued.contains(&name)
                    && !names.contains(&name)
                {
                    names.push(name);
                    if names.len() == 5 {
                        break;
                    }
                }
            }
            if names.len() == 5 {
                break;
            }
        }
        if !names.is_empty() {
            // One direct expansion only: transitive/cyclic graphs stay bounded,
            // and the model can request more evidence using the remaining budget.
            self.dependencies_inspected = true;
            pending.push_back(AssistantReadTool::InspectInstalledMods { names, offset: 0 });
        }
    }
}

async fn read_assistant_mod_state(
    storage: &StorageBootstrap,
    instance: &InstanceDetails,
    module: Option<&ModuleDetails>,
) -> Result<Value, String> {
    let current = read_instance_details(&storage.paths, &instance.summary.id)
        .await
        .map_err(|error| error.to_string())?;
    let settings: Value =
        serde_json::from_str(&current.settings_json).map_err(|error| error.to_string())?;
    let keys = settings
        .as_object()
        .into_iter()
        .flat_map(|values| values.keys())
        .filter(|key| assistant_setting_describes_mods(key))
        .cloned()
        .collect::<Vec<_>>();
    let mut result = json!({"support": module.and_then(|value| value.mods.as_ref()),
        "workshop": module.and_then(|value| value.workshop.as_ref()),
        "configuration": if keys.is_empty() { json!({"entries": []}) } else {
            assistant_settings_evidence(&current.settings_json, module.and_then(|value| value.schema_json.as_deref()), &keys[..keys.len().min(40)], 0)?
        },
        "remainingKeys": keys.iter().skip(40).collect::<Vec<_>>(),
        "limitation": "Configured order is not proof that files are installed or dependencies compatible. Read the native config and runtime errors before changing it."});
    if instance.summary.module_id == "dontstarve" {
        result["declaredEnablement"] = app_storage::inspect_dst_mod_enablement(&settings)?;
    }
    Ok(result)
}

pub(super) async fn read_assistant_installed_mods(
    state: &tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    instance: &InstanceDetails,
    names: Vec<String>,
    offset: usize,
) -> Result<Value, String> {
    if instance.summary.module_id != "dontstarve" {
        return Err(String::from(
            "Installed mod metadata inspection currently supports DST only; no files were read.",
        ));
    }
    let private_root = super::commands_runtime_lifecycle::private_runtime_install_root(instance)?;
    let settings = storage.settings.clone();
    let module_id = instance.summary.module_id.clone();
    let modules_root = storage.paths.modules_root.clone();
    // Only this selected instance contributes a UGC root. Other instances' mod
    // copies can differ and must not become evidence for the selected server.
    let instance_data_root = Path::new(&instance.config_file_path)
        .parent()
        .and_then(Path::parent)
        .map(|root| root.join("data"))
        .ok_or_else(|| String::from("Selected instance has no configuration parent."))?;
    let roots = vec![storage.paths.steamcmd_root.clone(), instance_data_root];
    let mut result = run_assistant_evidence_read(state, move || {
        let descriptors = discover_modules(&modules_root).map_err(|error| error.to_string())?;
        let descriptor = find_descriptor(&descriptors, &module_id)?;
        let installation = probe_module_install_state_with_override(
            &settings,
            &module_id,
            descriptor.summary.steam_app_id,
            descriptor.install.as_ref(),
            descriptor.process.as_ref(),
            Some(private_root.as_str()),
        );
        let install_root = PathBuf::from(installation.install_root);
        crate::dst_mods::read_dst_installed_mod_evidence(&install_root, &roots, &names, offset)
            .and_then(|page| serde_json::to_value(page).map_err(|error| error.to_string()))
    })
    .await?;
    let current = read_instance_details(&storage.paths, &instance.summary.id)
        .await
        .map_err(|error| error.to_string())?;
    let settings: Value =
        serde_json::from_str(&current.settings_json).map_err(|error| error.to_string())?;
    let enablement = app_storage::inspect_dst_mod_enablement(&settings)?;
    if let Some(entries) = result["entries"].as_array_mut() {
        for entry in entries {
            if let Some(name) = entry["folderName"].as_str() {
                entry["configuredEnablementByShard"] =
                    assistant_mod_declared_enablement(&enablement, name);
            }
        }
    }
    result["sourceLabels"] = json!({
        "install": "Selected instance runtime installation",
        "extra-0": "Shared SteamCMD cache",
        "extra-1": "Selected instance data"
    });
    result["limitation"] = json!(
        "File presence, modinfo metadata and configuredEnablementByShard do not prove runtime enablement, which copy loaded, or compatibility. declaredState=not_declared means no exact directory entry in the generated configuration; unknown means the configuration or an alias could not be resolved safely. Inspect fresh runtime evidence after a confirmed change."
    );
    Ok(result)
}

#[cfg(test)]
#[path = "mod_enablement_tests.rs"]
mod mod_enablement_tests;
