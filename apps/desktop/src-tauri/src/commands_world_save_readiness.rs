//! The same current-run gate serves UI reads, manual actions and scheduled saves.
//! Readiness never turns command delivery into proof that a save reached disk.
use super::*;
use app_core::InstanceWorldSaveReadiness;
use app_storage::StoragePaths;

fn tracked(
    state: &DesktopState,
    instance_id: &str,
) -> Result<Option<InstanceWorldSaveReadiness>, String> {
    Ok(state
        .runtime_supervisor
        .lock()
        .map_err(|_| String::from("runtime supervisor lock poisoned"))?
        .world_save_readiness(instance_id))
}

pub(in crate::commands) async fn snapshot(
    state: &DesktopState,
    paths: &StoragePaths,
    instance_id: &str,
) -> Result<Option<InstanceWorldSaveReadiness>, String> {
    let Some(mut readiness) = tracked(state, instance_id)? else {
        return Ok(None);
    };
    let (details, mut health) = Box::pin(app_storage::read_instance_details_and_runtime_health(
        paths,
        instance_id,
    ))
    .await
    .map_err(|error| error.to_string())?;
    crate::runtime_astroneer_health::apply(state, paths, &details, &mut health).await;
    crate::runtime_palworld_health::apply(state, paths, &details, &mut health).await;
    // A read can span a restart. Never combine one run's preparation with
    // another run's log or native session status.
    if tracked(state, instance_id)?.as_ref() != Some(&readiness)
        || !matches!(details.summary.status, InstanceStatus::Running)
        || !details.active_run.as_ref().is_some_and(|run| {
            run.run_id == readiness.run_id && run.session_id == readiness.session_id
        })
    {
        return Ok(None);
    }
    // Native interfaces have no extension-preparation prerequisite, including
    // when the manager reattaches to an already-owned run.
    if !super::super::commands_save_startup::requires_private_program(&details.summary.module_id) {
        readiness.prepared = Some(true);
    }
    readiness.world_ready = Some(health.status == "ready");
    Ok(Some(readiness))
}

pub(in crate::commands) async fn require_ready(
    state: &DesktopState,
    paths: &StoragePaths,
    expected: &InstanceDetails,
) -> Result<(), String> {
    let readiness = Box::pin(snapshot(state, paths, &expected.summary.id))
        .await?
        .ok_or(
            "Save readiness for this server run is unknown. Refresh the instance before saving.",
        )?;
    if !expected
        .active_run
        .as_ref()
        .is_some_and(|run| run.run_id == readiness.run_id && run.session_id == readiness.session_id)
    {
        return Err("The server run changed before saving. Refresh the instance and retry.".into());
    }
    match readiness.prepared {
        Some(true) => {}
        Some(false) => return Err("Manual saving is unavailable for this run. See the startup log.".into()),
        None => return Err("Save preparation for this run is unknown. Stop the server normally and start it again.".into()),
    }
    if readiness.world_ready != Some(true) {
        return Err(
            "The world is still loading or its readiness is unconfirmed. Wait before saving."
                .into(),
        );
    }
    Ok(())
}
