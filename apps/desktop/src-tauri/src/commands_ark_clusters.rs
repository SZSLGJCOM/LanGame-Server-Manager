use super::*;
use app_storage::{ArkClusterIdentity, ArkClusterReport};

#[derive(Debug, Deserialize)]
pub struct ReadArkClusterInput {
    pub instance_id: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArkClusterAction {
    Start,
    Stop,
}

#[derive(Debug, Deserialize)]
pub struct OperateArkClusterInput {
    pub instance_id: String,
    pub expected_identity: ArkClusterIdentity,
    pub action: ArkClusterAction,
}

#[derive(Debug, Serialize)]
pub struct ArkClusterMemberOperation {
    pub instance_id: String,
    pub instance_name: String,
    pub outcome: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct ArkClusterOperationResult {
    pub action: ArkClusterAction,
    pub members: Vec<ArkClusterMemberOperation>,
}

#[tauri::command]
pub async fn read_ark_cluster(
    state: tauri::State<'_, DesktopState>,
    input: ReadArkClusterInput,
) -> Result<ArkClusterReport, String> {
    let _operation = state.begin_storage_context_operation("ARK cluster inspection")?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    reconcile_runtime_state(&state).await?;
    app_storage::read_ark_cluster_report(&storage.paths, &input.instance_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn operate_ark_cluster(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, DesktopState>,
    input: OperateArkClusterInput,
) -> Result<ArkClusterOperationResult, String> {
    let _operation = state.begin_storage_context_operation("ARK cluster operation")?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    reconcile_runtime_state(&state).await?;
    let report = app_storage::read_ark_cluster_report(&storage.paths, &input.instance_id)
        .await
        .map_err(|error| error.to_string())?;
    validate_membership(&report, &input.expected_identity, input.action)?;
    let mut outcomes = Vec::with_capacity(report.members.len());
    for member in report.members {
        // The visible member list is a precondition, not authority to operate a
        // newly regrouped server. Recheck before each normal lifecycle command.
        let result: Result<(&str, String), String> = async {
            ensure_storage_context_snapshot_current(&state, &storage, "ARK cluster operation")?;
            // Read before regrouping, then let the normal start worker compare
            // this snapshot under its mutation lock. A later settings change
            // must not launch this member into a different cluster.
            let expected_instance = if matches!(input.action, ArkClusterAction::Start) {
                Some(
                    read_instance_details(&storage.paths, &member.summary.id)
                        .await
                        .map_err(|error| error.to_string())?,
                )
            } else {
                None
            };
            let current = app_storage::read_ark_cluster_report(&storage.paths, &input.instance_id)
                .await
                .map_err(|error| error.to_string())?;
            validate_membership(&current, &input.expected_identity, input.action)?;
            let current_member = current
                .members
                .iter()
                .find(|entry| entry.summary.id == member.summary.id)
                .ok_or_else(|| {
                    String::from(
                        "ARK cluster membership changed. Refresh the cluster before retrying.",
                    )
                })?;
            match input.action {
                ArkClusterAction::Start
                    if matches!(current_member.summary.status, InstanceStatus::Running) =>
                {
                    Ok(("skipped", "Already running".to_owned()))
                }
                ArkClusterAction::Stop
                    if current_member.summary.active_process_count == 0
                        && !matches!(
                            current_member.summary.status,
                            InstanceStatus::Starting | InstanceStatus::Stopping
                        ) =>
                {
                    Ok(("skipped", "Already stopped".to_owned()))
                }
                ArkClusterAction::Start => {
                    commands_runtime_lifecycle::start_instance_process_with_preconditions(
                        Some(&app_handle),
                        &state,
                        &storage,
                        member.summary.id.clone(),
                        "manual",
                        commands_runtime_lifecycle::RuntimeStartPreconditions {
                            instance: expected_instance,
                            ..Default::default()
                        },
                    )
                    .await?;
                    Ok((
                        "succeeded",
                        "Started through the managed instance lifecycle".to_owned(),
                    ))
                }
                ArkClusterAction::Stop => {
                    commands_runtime_lifecycle::stop_instance_process(
                        state.clone(),
                        member.summary.id.clone(),
                    )
                    .await?;
                    Ok((
                        "succeeded",
                        "Stopped through the managed instance lifecycle".to_owned(),
                    ))
                }
            }
        }
        .await;
        let (outcome, message) = match result {
            Ok(value) => value,
            Err(error) => ("failed", error),
        };
        outcomes.push(ArkClusterMemberOperation {
            instance_id: member.summary.id,
            instance_name: member.summary.name,
            outcome: outcome.to_owned(),
            message,
        });
    }
    Ok(ArkClusterOperationResult {
        action: input.action,
        members: outcomes,
    })
}

fn validate_membership(
    report: &ArkClusterReport,
    expected: &ArkClusterIdentity,
    action: ArkClusterAction,
) -> Result<(), String> {
    if report.identity.as_ref() != Some(expected) || expected.member_ids.is_empty() {
        return Err(String::from(
            "ARK cluster membership changed. Refresh the cluster before retrying.",
        ));
    }
    if matches!(action, ArkClusterAction::Start) && report.start_blocked {
        return Err(String::from(
            "Resolve the reported ARK cluster configuration conflicts before starting the cluster.",
        ));
    }
    Ok(())
}
