use super::*;

pub(super) struct RuntimeLogHealth {
    pub health_scan: LogTailSnapshot,
    pub log_tail: LogTailSnapshot,
    pub health: app_core::RuntimeHealth,
    pub query_restriction: Option<(&'static str, &'static str)>,
}

/// Read current-run health without the independent live-player query.
pub(super) async fn read(
    paths: &StoragePaths,
    data: &InstanceRuntimeData,
    ark_health_instance: Option<InstanceDetails>,
) -> Result<RuntimeLogHealth, StorageError> {
    let record = &data.record;
    let log_source_path =
        resolve_instance_log_source_path(record, data.active_run.as_ref(), &data.recent_runs)
            .await?;
    let module_id = record.summary.module_id.clone();
    let status = record.summary.status.clone();
    let health_run = data.active_run.clone();
    let health_ports = data.ports.clone();
    let config_file_path = record.config_dir.join("instance.json");
    let status_health_root = if module_id == "returntomoria" {
        Some(effective_instance_install_root(record)?)
    } else {
        None
    };
    let ark_health_instance = if app_core::ark_maps::is_ark(&module_id) {
        Some(match ark_health_instance {
            Some(details) => details,
            None => data.details(paths)?,
        })
    } else {
        None
    };
    tokio::task::spawn_blocking(move || {
        let query_restriction =
            if matches!(module_id.as_str(), "valheim" | "vrising" | "abioticfactor") {
                let settings = crate::instances::read_instance_settings_json(&config_file_path)?;
                steam_player_query_visibility_restriction(&module_id, &settings)
            } else {
                None
            };
        let health_scan =
            read_log_snapshot(log_source_path.clone(), RUNTIME_HEALTH_SCAN_LINE_LIMIT);
        let log_tail = read_log_snapshot(log_source_path, 32);
        let health = ark_health_instance
            .as_ref()
            .and_then(ark_readiness::analyze_ark_maps_runtime_health)
            .unwrap_or_else(|| {
                analyze_runtime_health_with_startup_evidence(
                    &module_id,
                    &status,
                    &health_scan,
                    health_run.as_ref(),
                    &health_ports,
                )
            });
        let health = if matches!(health.status.as_str(), "starting" | "ready") {
            status_health_root
                .as_ref()
                .and_then(|root| returntomoria_health::analyze(&status, root, health_run.as_ref()))
                .unwrap_or(health)
        } else {
            health
        };
        Ok(RuntimeLogHealth {
            health_scan,
            log_tail,
            health,
            query_restriction,
        })
    })
    .await
    .map_err(|error| StorageError::BlockingTaskFailed {
        operation: "read runtime health logs",
        message: error.to_string(),
    })?
}
