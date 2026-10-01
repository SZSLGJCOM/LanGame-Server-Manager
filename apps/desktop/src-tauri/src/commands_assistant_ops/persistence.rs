#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AssistantTaskCheckpoint {
    version: u32,
    revision: u64,
    id: String,
    request: AssistantTaskRequest,
    original_request: String,
    requirements: Option<AssistantTaskRequirements>,
    requirements_schema: Option<String>,
    instance_id: Option<String>,
    module_id: Option<String>,
    target: String,
    initial_settings: Value,
    configuration_stage: String,
    required_mods: Vec<AssistantArchivedRequiredMod>,
    mod_evidence_known: bool,
    file_changes: Vec<AssistantArchivedFileChange>,
    budget: AssistantRunCheckpoint,
    requires_restatement: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AssistantArchivedRequiredMod {
    shard: String,
    folder_name: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AssistantArchivedFileChange {
    file: String,
    source_sha256: String,
    result_sha256: String,
    backup_id: String,
    read_back_verified: bool,
}

async fn assistant_restore_sessions(
    state: &DesktopState,
    settings: &AssistantProviderSettings,
) -> Result<(), String> {
    let binding = assistant_session_binding(state, settings)?;
    state.assistant_sessions.restore(&binding).await?;
    if assistant_session_binding(state, settings)? != binding {
        return Err("Assistant storage paths changed while opening its archive; retry in the current context.".into());
    }
    Ok(())
}

fn assistant_checkpoint_target(
    task: &AssistantTaskContract,
    mode: &AssistantOperationMode,
) -> Result<(), String> {
    let Some(session) = &task.session else {
        return Ok(());
    };
    let target = match mode {
        AssistantOperationMode::ResolvedPreview { target, .. } => *target,
        AssistantOperationMode::Confirmed(pending)
            if pending.plan.action == AssistantOperationAction::CreateServer =>
        {
            AssistantIntentTarget::NewInstance
        }
        AssistantOperationMode::Confirmed(_) | AssistantOperationMode::FollowUp { .. }
            if task.instance_id.is_some() =>
        {
            AssistantIntentTarget::ExistingInstance
        }
        AssistantOperationMode::Confirmed(_) | AssistantOperationMode::FollowUp { .. }
            if task.prepares_service() =>
        {
            AssistantIntentTarget::NewInstance
        }
        AssistantOperationMode::Confirmed(_) | AssistantOperationMode::FollowUp { .. } => {
            AssistantIntentTarget::Module
        }
        #[cfg(test)]
        AssistantOperationMode::Preview | AssistantOperationMode::ExecuteImmediately => {
            return Ok(());
        }
    };
    let mut checkpoint = session.checkpoint()?.unwrap_or_else(|| json!({}));
    checkpoint["target"] = json!(match target {
        AssistantIntentTarget::ExistingInstance => "existing_instance",
        AssistantIntentTarget::NewInstance => "new_instance",
        AssistantIntentTarget::Module => "module",
        AssistantIntentTarget::None =>
            return Err("A server task checkpoint requires a resolved target.".into()),
    });
    session.set_checkpoint(Some(checkpoint))
}

/// Save the task after binding/changing its contract, and after reserving work
/// but before starting it. No prepared operation, provider key, old confirmation
/// token, or instruction to replay a mutation belongs in this checkpoint.
async fn assistant_checkpoint_task(task: &AssistantTaskContract) -> Result<(), String> {
    let Some(session) = &task.session else {
        return Ok(());
    };
    if session.check_active().is_err() {
        return session.flush().await;
    }
    let original_request = redact_assistant_provider_text(&task.original_request);
    let requirements =
        serde_json::to_value(&task.requirements).map_err(|error| error.to_string())?;
    let redacted_requirements: Value =
        serde_json::from_str(&redact_assistant_provider_text(&requirements.to_string()))
            .map_err(|_| "Task constraints could not be safely archived.")?;
    let initial_settings: Value = serde_json::from_str(&redact_assistant_provider_text(
        &task.initial_settings.to_string(),
    ))
    .map_err(|_| "Task baseline could not be safely archived.")?;
    let requirements_schema = task
        .requirements_schema
        .as_deref()
        .map(redact_assistant_provider_text);
    let target = if task.instance_id.is_some() {
        "existing_instance".to_owned()
    } else {
        session
            .checkpoint()?
            .and_then(|value| value["target"].as_str().map(str::to_owned))
            .unwrap_or_else(|| {
                if task.prepares_service() {
                    "new_instance"
                } else {
                    "module"
                }
                .into()
            })
    };
    let checkpoint = AssistantTaskCheckpoint {
        version: 1,
        revision: session.revision(),
        id: task.id.clone(),
        request: task.request.clone(),
        requires_restatement: original_request != task.original_request
            || redacted_requirements != requirements
            || initial_settings != task.initial_settings
            || requirements_schema.as_deref() != task.requirements_schema.as_deref(),
        original_request,
        requirements: serde_json::from_value(redacted_requirements)
            .map_err(|error| error.to_string())?,
        requirements_schema,
        instance_id: task.instance_id.clone(),
        module_id: task.module_id.clone(),
        target,
        initial_settings,
        configuration_stage: match task.configuration_stage {
            AssistantConfigurationStage::Unconfigured => "unconfigured",
            AssistantConfigurationStage::Configuring => "configuring",
            AssistantConfigurationStage::Protected => "protected",
        }
        .into(),
        required_mods: task
            .required_mods
            .iter()
            .map(|item| AssistantArchivedRequiredMod {
                shard: item.shard.clone(),
                folder_name: item.folder_name.clone(),
            })
            .collect(),
        mod_evidence_known: task.mod_evidence_known,
        file_changes: task
            .file_changes
            .iter()
            .map(|item| AssistantArchivedFileChange {
                file: item.file.clone(),
                source_sha256: item.source_sha256.clone(),
                result_sha256: item.result_sha256.clone(),
                backup_id: item.backup_id.clone(),
                read_back_verified: item.read_back_verified,
            })
            .collect(),
        budget: task.run.checkpoint()?,
    };
    session.set_checkpoint(Some(
        serde_json::to_value(checkpoint).map_err(|error| error.to_string())?,
    ))?;
    session.flush().await
}

fn assistant_restore_task_checkpoint(
    session: &std::sync::Arc<AssistantSession>,
    settings: &AssistantProviderSettings,
) -> Result<(), String> {
    if !session.needs_recovery() || session.check_active().is_err() {
        return Ok(());
    }
    let mut continuations = assistant_continuations()
        .lock()
        .map_err(|_| "Assistant checkpoint store is unavailable.")?;
    if continuations
        .get(session.id())
        .is_some_and(AssistantTaskContinuation::is_current)
    {
        return Ok(());
    }
    let Some(value) = session.checkpoint()? else {
        return Ok(());
    };
    let saved: AssistantTaskCheckpoint = serde_json::from_value(value)
        .map_err(|_| "Assistant task checkpoint is damaged; no operation was restored.")?;
    if saved.version != 1
        || saved.revision != session.revision()
        || saved.id.len() > 64
        || saved.required_mods.len() > 64
        || saved.file_changes.len() > ASSISTANT_RUN_MAX_OPERATIONS * 8
    {
        return Err(
            "Assistant task checkpoint is stale or unsupported; no operation was restored.".into(),
        );
    }
    let stage = match saved.configuration_stage.as_str() {
        "unconfigured" => AssistantConfigurationStage::Unconfigured,
        "configuring" => AssistantConfigurationStage::Configuring,
        "protected" => AssistantConfigurationStage::Protected,
        _ => return Err("Assistant task configuration checkpoint is invalid.".into()),
    };
    let target = match saved.target.as_str() {
        "existing_instance" if saved.instance_id.is_some() => {
            AssistantIntentTarget::ExistingInstance
        }
        "new_instance" if saved.instance_id.is_none() => AssistantIntentTarget::NewInstance,
        "module" if saved.instance_id.is_none() => AssistantIntentTarget::Module,
        _ => {
            return Err(
                "Assistant checkpoint target does not match its recorded instance scope.".into(),
            );
        }
    };
    let task = std::sync::Arc::new(AssistantTaskContract {
        session: Some(session.clone()),
        run: std::sync::Arc::new(AssistantTaskRun::from_checkpoint(
            saved.budget,
            saved.requires_restatement,
        )?),
        investigation_draft: std::sync::Arc::new(StdMutex::new(None)),
        id: saved.id,
        request: saved.request,
        original_request: saved.original_request,
        requirements: saved.requirements,
        requirements_schema: saved.requirements_schema.map(std::sync::Arc::from),
        instance_id: saved.instance_id,
        module_id: saved.module_id,
        initial_settings: saved.initial_settings,
        configuration_stage: stage,
        mod_evidence_known: saved.mod_evidence_known,
        required_mods: saved
            .required_mods
            .into_iter()
            .map(|item| AssistantRequiredMod {
                shard: item.shard,
                folder_name: item.folder_name,
            })
            .collect(),
        file_changes: saved
            .file_changes
            .into_iter()
            .map(|item| app_storage::InstanceFilePatchResult {
                file: item.file,
                source_sha256: item.source_sha256,
                result_sha256: item.result_sha256,
                backup_id: item.backup_id,
                read_back_verified: item.read_back_verified,
            })
            .collect(),
    });
    let mut settings = settings.clone();
    settings.api_key.clear();
    let input = AssistantExecuteOperationInput {
        settings, prompt: task.original_request.clone(), context: Some("Recovered task: earlier evidence is historical. Inspect the current instance and files before preparing a new confirmation. The last interrupted operation may have completed; do not replay it based on missing output.".into()),
        task: task.request.clone(), selected_instance_id: task.instance_id.clone(), selected_module_id: task.module_id.clone(),
    };
    continuations.retain(|_, saved| saved.is_current());
    if continuations.len() >= 16 {
        return Err("Assistant checkpoint capacity is full.".into());
    }
    continuations.insert(
        session.id().to_owned(),
        AssistantTaskContinuation {
            input,
            mode: AssistantOperationMode::ResolvedPreview {
                task: task.clone(),
                target,
                conversation_reference: None,
            },
            task,
            revision: session.revision(),
            expires_at: Instant::now() + Duration::from_secs(30 * 24 * 60 * 60),
        },
    );
    session.recovered();
    Ok(())
}

#[cfg(test)]
#[path = "persistence_tests.rs"]
mod persistence_tests;
