#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssistantTaskGoal {
    Inspect,
    #[default]
    ApplyChange,
    PrepareService,
    RestoreService,
    LaunchService,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssistantTaskRequest {
    pub goal: AssistantTaskGoal,
    pub preserve_existing_mods: bool,
}

impl Default for AssistantTaskRequest {
    fn default() -> Self {
        Self {
            goal: AssistantTaskGoal::ApplyChange,
            preserve_existing_mods: true,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssistantTaskStatus {
    Proposed,
    Completed,
    Failed,
    Inconclusive,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssistantTaskCheckStatus {
    Satisfied,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantTaskCheck {
    pub name: String,
    pub status: AssistantTaskCheckStatus,
    pub summary: String,
    pub evidence: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantTaskReceipt {
    pub id: String,
    pub goal: AssistantTaskGoal,
    pub preserve_existing_mods: bool,
    pub instance_id: Option<String>,
    pub module_id: Option<String>,
    pub status: AssistantTaskStatus,
    pub operation_limit: usize,
    pub requirements: Vec<AssistantTaskRequirementView>,
    pub checks: Vec<AssistantTaskCheck>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AssistantRequiredMod {
    shard: String,
    folder_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AssistantConfigurationStage {
    Unconfigured,
    Configuring,
    Protected,
}

// The application owns the baseline independently of operation snapshots.
// Existing instances are protected immediately; new ones at their first start.
#[derive(Debug, Clone)]
pub(super) struct AssistantTaskContract {
    session: Option<std::sync::Arc<crate::assistant_sessions::AssistantSession>>,
    run: std::sync::Arc<AssistantTaskRun>,
    investigation_draft: std::sync::Arc<StdMutex<Option<AssistantRequirementsDraft>>>,
    id: String,
    request: AssistantTaskRequest,
    original_request: String,
    requirements: Option<AssistantTaskRequirements>,
    requirements_schema: Option<std::sync::Arc<str>>,
    instance_id: Option<String>,
    module_id: Option<String>,
    initial_settings: Value,
    configuration_stage: AssistantConfigurationStage,
    required_mods: Vec<AssistantRequiredMod>,
    mod_evidence_known: bool,
    file_changes: Vec<app_storage::InstanceFilePatchResult>,
}

impl AssistantTaskContract {
    pub(super) fn capture(
        input: &AssistantExecuteOperationInput,
        instance: Option<&InstanceDetails>,
    ) -> Result<Self, String> {
        if input.task.goal == AssistantTaskGoal::RestoreService && instance.is_none() {
            return Err(String::from(
                "Select an existing server for a restore-service task.",
            ));
        }
        if matches!(
            input.task.goal,
            AssistantTaskGoal::PrepareService | AssistantTaskGoal::LaunchService
        ) && instance.is_none()
            && assistant_normalized_optional_id(input.selected_module_id.as_deref()).is_none()
        {
            return Err(String::from(
                "Select a game module before preparing a new server.",
            ));
        }
        let settings: Value = instance
            .map(|value| serde_json::from_str(&value.settings_json))
            .transpose()
            .map_err(|error| format!("Cannot capture task configuration: {error}"))?
            .unwrap_or_else(|| json!({}));
        let mut contract = Self {
            session: None,
            run: std::sync::Arc::new(AssistantTaskRun::default()),
            investigation_draft: std::sync::Arc::new(StdMutex::new(None)),
            id: uuid::Uuid::new_v4().simple().to_string(),
            request: input.task.clone(),
            original_request: input.prompt.clone(),
            requirements: None,
            requirements_schema: None,
            instance_id: instance.map(|value| value.summary.id.clone()),
            module_id: instance
                .map(|value| value.summary.module_id.clone())
                .or_else(|| input.selected_module_id.clone()),
            initial_settings: settings,
            configuration_stage: if instance.is_some() {
                AssistantConfigurationStage::Protected
            } else {
                AssistantConfigurationStage::Unconfigured
            },
            required_mods: Vec::new(),
            mod_evidence_known: true,
            file_changes: Vec::new(),
        };
        if contract.request.preserve_existing_mods && instance.is_some() {
            contract.capture_mod_requirements()?;
        }
        Ok(contract)
    }

    fn capture_mod_requirements(&mut self) -> Result<(), String> {
        if self.module_id.as_deref() != Some("dontstarve") {
            self.mod_evidence_known = !self.initial_settings.as_object().is_some_and(|settings| {
                settings.iter().any(|(key, value)| {
                    assistant_setting_describes_mods(key)
                        && !value.is_null()
                        && value != ""
                        && value != &json!([])
                })
            });
            return Ok(());
        }
        let projection = app_storage::inspect_dst_mod_enablement(&self.initial_settings)?;
        let Some(shards) = projection["shards"].as_array() else {
            self.mod_evidence_known = false;
            return Ok(());
        };
        for shard in shards.iter().filter(|shard| shard["active"] == true) {
            if shard["analysisStatus"] != "known"
                || shard["declaredUnspecifiedModNames"]
                    .as_array()
                    .is_none_or(|names| !names.is_empty())
            {
                self.mod_evidence_known = false;
            }
            if let Some(names) = shard["declaredEnabledModNames"].as_array() {
                for name in names.iter().filter_map(Value::as_str) {
                    if self.required_mods.len() == 64
                        || crate::dst_mods::validate_dst_mod_directory_name(name).is_err()
                    {
                        self.mod_evidence_known = false;
                        continue;
                    }
                    self.required_mods.push(AssistantRequiredMod {
                        shard: shard["shard"].as_str().unwrap_or_default().to_owned(),
                        folder_name: name.to_owned(),
                    });
                }
            }
        }
        Ok(())
    }

    fn validate_settings(&self, settings: &Value) -> Result<(), String> {
        if !self.request.preserve_existing_mods
            || self.configuration_stage != AssistantConfigurationStage::Protected
        {
            return Ok(());
        }
        if self.module_id.as_deref() == Some("dontstarve") {
            return app_storage::validate_dst_mod_preservation(&self.initial_settings, settings);
        }
        let keys = self
            .initial_settings
            .as_object()
            .into_iter()
            .flat_map(|values| values.keys())
            .chain(
                settings
                    .as_object()
                    .into_iter()
                    .flat_map(|values| values.keys()),
            );
        if keys
            .filter(|key| assistant_setting_describes_mods(key))
            .any(|key| {
                if self.initial_settings[key] == settings[key] {
                    return false;
                }
                // Only this native field has a verified order-independent ID
                // preservation contract; other games and fields remain exact.
                if self.module_id.as_deref() == Some("projectzomboid") && key == "mods" {
                    return match (self.initial_settings[key].as_str(), settings[key].as_str()) {
                        (Some(before), Some(after)) => {
                            !app_storage::projectzomboid_mod_reorder_preserves_ids(before, after)
                        }
                        _ => true,
                    };
                }
                true
            })
        {
            return Err(String::from(
                "This task preserves existing mod configuration. Only a verified reordering of the same Project Zomboid Mod IDs is allowed; leave other protected fields unchanged.",
            ));
        }
        Ok(())
    }

    fn validate_plan(
        &self,
        plan: &AssistantOperationPlan,
        instance: Option<&InstanceDetails>,
    ) -> Result<(), String> {
        validate_assistant_text_patch_plan(plan)?;
        validate_assistant_workshop_plan(plan)?;
        if matches!(
            plan.action,
            AssistantOperationAction::PatchInstanceText
                | AssistantOperationAction::PatchInstanceFiles
        ) {
            ensure_assistant_patch_target_stopped(
                instance.ok_or("Select an existing instance before editing its Mod files.")?,
            )?;
        }
        if plan.action == AssistantOperationAction::None {
            return Ok(());
        }
        if self.request.goal == AssistantTaskGoal::Inspect {
            return Err(String::from(
                "This request is read-only. Explain the observed evidence without proposing a change or starting a server.",
            ));
        }
        validate_assistant_lifecycle_plan(plan, self.request.goal, instance)?;
        if let Some(requirements) = &self.requirements {
            requirements.validate_action(plan, instance, self.requirements_schema.as_deref())?;
        }
        if self.prepares_service() && self.instance_id.is_none() && plan.instance_id.is_some() {
            return Err(String::from(
                "A new-server task cannot select an existing instance.",
            ));
        }
        if plan.action == AssistantOperationAction::CreateServer
            && (self.instance_id.is_some() || plan.instance_id.is_some())
        {
            return Err(String::from(
                "A create-server operation cannot replace an existing task target.",
            ));
        }
        if plan.action == AssistantOperationAction::StartServer && self.instance_id.is_none() {
            return Err(String::from(
                "Create and configure an instance before requesting start_server.",
            ));
        }
        if plan.action == AssistantOperationAction::StartServer
            && self.request.goal == AssistantTaskGoal::PrepareService
        {
            return Err(String::from(
                "This task prepares the server without starting it. Startup requires a separate user request.",
            ));
        }
        if plan.action == AssistantOperationAction::StartServer
            && self.request.goal == AssistantTaskGoal::LaunchService
            && self.configuration_stage == AssistantConfigurationStage::Unconfigured
        {
            return Err(String::from(
                "Confirm the requested initial settings with customize_config or apply_beginner_config before starting the newly created instance. Defaults also require a configuration preview.",
            ));
        }
        if plan
            .instance_id
            .as_ref()
            .zip(self.instance_id.as_ref())
            .is_some_and(|(planned, bound)| planned != bound)
            || plan
                .module_id
                .as_ref()
                .zip(self.module_id.as_ref())
                .is_some_and(|(planned, bound)| planned != bound)
        {
            return Err(String::from(
                "The proposed target conflicts with the bound task target.",
            ));
        }
        if self.request.goal == AssistantTaskGoal::RestoreService
            && !matches!(
                plan.action,
                AssistantOperationAction::StartServer
                    | AssistantOperationAction::CustomizeConfig
                    | AssistantOperationAction::PatchInstanceText
                    | AssistantOperationAction::PatchInstanceFiles
                    | AssistantOperationAction::ApplyBeginnerConfig
                    | AssistantOperationAction::RepairPorts
                    | AssistantOperationAction::ValidateServer
            )
        {
            return Err(String::from(
                "A restore-service task only permits configuration, private Mod text repair, port repair, file validation and a separately confirmed start.",
            ));
        }
        if self.prepares_service() {
            let allowed = if self.instance_id.is_none() {
                matches!(
                    plan.action,
                    AssistantOperationAction::InstallServer
                        | AssistantOperationAction::ValidateServer
                        | AssistantOperationAction::CreateServer
                )
            } else {
                matches!(
                    plan.action,
                    AssistantOperationAction::InstallServer
                        | AssistantOperationAction::ValidateServer
                        | AssistantOperationAction::CustomizeConfig
                        | AssistantOperationAction::ApplyBeginnerConfig
                        | AssistantOperationAction::RepairPorts
                        | AssistantOperationAction::StartServer
                )
            };
            if !allowed {
                return Err(String::from(
                    "This task may prepare files, create its instance, and configure it. Only a launch task may request startup.",
                ));
            }
        }
        if let Some(instance) = instance {
            if self.instance_id.is_none() {
                return Err(String::from(
                    "The task must capture the selected server baseline before previewing a change.",
                ));
            }
            if self
                .instance_id
                .as_deref()
                .is_some_and(|id| id != instance.summary.id)
                || self
                    .module_id
                    .as_deref()
                    .is_some_and(|id| id != instance.summary.module_id)
            {
                return Err(String::from("The task target changed."));
            }
            let current: Value =
                serde_json::from_str(&instance.settings_json).map_err(|error| error.to_string())?;
            self.validate_settings(&current)?;
            if matches!(
                plan.action,
                AssistantOperationAction::CustomizeConfig
                    | AssistantOperationAction::ApplyBeginnerConfig
            ) {
                let patch = plan
                    .settings_patch
                    .as_ref()
                    .ok_or("AI did not provide a settingsPatch to apply.")?;
                let merged = merge_assistant_settings_patch(&current, patch)?;
                if self.requires_requirements() && !merged.rejected_keys.is_empty() {
                    return Err(String::from(
                        "The settings patch contains unknown keys. Use search_settings or read_settings to find the actual setting keys, then propose the complete corrected patch. No partial configuration can be confirmed for this task.",
                    ));
                }
                self.validate_settings(&merged.settings)?;
            }
        }
        Ok(())
    }

    fn summary(&self) -> String {
        let goal = match self.request.goal {
            AssistantTaskGoal::Inspect => {
                "inspect evidence and answer without changing server state"
            }
            AssistantTaskGoal::ApplyChange => "apply the confirmed operation",
            AssistantTaskGoal::PrepareService => {
                "prepare server files, create an instance if needed, and verify the requested saved configuration without starting a server"
            }
            AssistantTaskGoal::RestoreService => "restore service and verify the new run",
            AssistantTaskGoal::LaunchService => {
                "prepare server files, create an instance if needed, apply the requested configuration, then start and verify service"
            }
        };
        let preservation = if self.request.preserve_existing_mods
            && self.configuration_stage != AssistantConfigurationStage::Protected
        {
            "The new instance's defaults may be replaced across confirmed initial configuration steps. Its requested settings must be confirmed and verified before preparation is complete. A launch task additionally requires a separately confirmed start, which protects the final enabled mods, active shards and mod options for subsequent repairs."
        } else if self.request.preserve_existing_mods
            && self.module_id.as_deref() == Some("projectzomboid")
        {
            "Keep the exact existing enabled Mod IDs and other Mod settings. The mods field may be reordered when evidence or an explicit requested order supports it; do not add, remove or rename IDs. A saved order does not prove dependency compatibility or runtime loading."
        } else if self.request.preserve_existing_mods {
            "Keep existing enabled mods, active shards and mod configuration."
        } else {
            "Mod configuration may change as shown in the confirmed operation."
        };
        format!(
            "Task goal: {goal}. {preservation} The goal and preservation requirements remain fixed throughout follow-up steps.{}",
            self.requirements_summary()
        )
    }

    fn receipt(
        &self,
        status: AssistantTaskStatus,
        checks: Vec<AssistantTaskCheck>,
    ) -> AssistantTaskReceipt {
        AssistantTaskReceipt {
            id: self.id.clone(),
            goal: self.request.goal,
            preserve_existing_mods: self.request.preserve_existing_mods,
            instance_id: self.instance_id.clone(),
            module_id: self.module_id.clone(),
            status,
            operation_limit: self.operation_limit(),
            requirements: self
                .requirements
                .as_ref()
                .map_or_else(Vec::new, AssistantTaskRequirements::views),
            checks,
        }
    }

    fn operation_limit(&self) -> usize {
        assistant_operation_limit(self.request.goal)
    }

    fn requires_running_service(&self) -> bool {
        matches!(
            self.request.goal,
            AssistantTaskGoal::RestoreService | AssistantTaskGoal::LaunchService
        )
    }

    fn prepares_service(&self) -> bool {
        matches!(
            self.request.goal,
            AssistantTaskGoal::PrepareService | AssistantTaskGoal::LaunchService
        )
    }

    fn requires_requirements(&self) -> bool {
        self.prepares_service() || self.request.goal == AssistantTaskGoal::RestoreService
    }

    fn bind_created_instance(&self, details: &InstanceDetails) -> Result<Self, String> {
        if self.instance_id.is_some()
            || self.module_id.as_deref() != Some(details.summary.module_id.as_str())
        {
            return Err(String::from(
                "The created server does not match the task's selected game.",
            ));
        }
        Ok(Self {
            id: self.id.clone(),
            request: self.request.clone(),
            original_request: self.original_request.clone(),
            requirements: self.requirements.clone(),
            requirements_schema: self.requirements_schema.clone(),
            instance_id: Some(details.summary.id.clone()),
            module_id: self.module_id.clone(),
            initial_settings: serde_json::from_str(&details.settings_json)
                .map_err(|error| error.to_string())?,
            configuration_stage: AssistantConfigurationStage::Unconfigured,
            required_mods: Vec::new(),
            mod_evidence_known: true,
            file_changes: self.file_changes.clone(),
            session: self.session.clone(),
            run: self.run.clone(),
            investigation_draft: self.investigation_draft.clone(),
        })
    }

    fn record_initial_configuration(&self, details: &InstanceDetails) -> Result<Self, String> {
        if self.configuration_stage == AssistantConfigurationStage::Protected
            || self.instance_id.as_deref() != Some(details.summary.id.as_str())
            || self.module_id.as_deref() != Some(details.summary.module_id.as_str())
        {
            return Err(String::from(
                "The initial server configuration cannot be rebound.",
            ));
        }
        let initialized = Self {
            id: self.id.clone(),
            request: self.request.clone(),
            original_request: self.original_request.clone(),
            requirements: self.requirements.clone(),
            requirements_schema: self.requirements_schema.clone(),
            instance_id: self.instance_id.clone(),
            module_id: self.module_id.clone(),
            initial_settings: serde_json::from_str(&details.settings_json)
                .map_err(|error| error.to_string())?,
            configuration_stage: AssistantConfigurationStage::Configuring,
            required_mods: Vec::new(),
            mod_evidence_known: true,
            file_changes: self.file_changes.clone(),
            session: self.session.clone(),
            run: self.run.clone(),
            investigation_draft: self.investigation_draft.clone(),
        };
        Ok(initialized)
    }

    fn protect_created_configuration(&self, details: &InstanceDetails) -> Result<Self, String> {
        if self.configuration_stage != AssistantConfigurationStage::Configuring {
            return Err(String::from(
                "Confirm initial configuration before protecting its baseline.",
            ));
        }
        let mut protected = self.record_initial_configuration(details)?;
        protected.configuration_stage = AssistantConfigurationStage::Protected;
        if self.request.preserve_existing_mods {
            protected.capture_mod_requirements()?;
        }
        Ok(protected)
    }
}

pub(super) fn assistant_operation_limit(_goal: AssistantTaskGoal) -> usize {
    // Per-run accounting pauses earlier when a scheduling allowance is spent.
    // This lifetime boundary also protects old confirmation step counters.
    32
}

#[cfg(test)]
#[path = "task_tests.rs"]
mod task_tests;
