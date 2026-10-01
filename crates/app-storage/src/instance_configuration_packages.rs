use super::*;

/// The instance settings lease protects configuration writers during package
/// preparation. Runtime and program metadata have other writers: check them again
/// in the short publication transaction instead of retaining the database writer
/// while copying payloads.
pub(super) async fn revalidate(
    tx: &mut Transaction<'_, Sqlite>,
    expected: &StoredInstanceRecord,
    ports: &[PortBinding],
    running: bool,
) -> Result<(), StorageError> {
    let current = fetch_instance_record(&mut **tx, &expected.summary.id).await?;
    let current_ports = load_instance_ports(&mut **tx, &expected.summary.id).await?;
    let current_running = matches!(
        current.summary.status,
        InstanceStatus::Starting | InstanceStatus::Running | InstanceStatus::Stopping
    ) || load_active_instance_run(&mut **tx, &expected.summary.id)
        .await?
        .is_some()
        || current.summary.active_process_count > 0;
    if current.config_dir != expected.config_dir
        || current.saves_dir != expected.saves_dir
        || current.runtime_mode != expected.runtime_mode
        || current.program_install_root != expected.program_install_root
        || current.summary.name != expected.summary.name
        || current.summary.module_id != expected.summary.module_id
        || current.summary.bind_ip != expected.summary.bind_ip
        || current.summary.autostart != expected.summary.autostart
        || current.auto_backup_on_stop != expected.auto_backup_on_stop
        || current.backup_retention_count != expected.backup_retention_count
        || current_running != running
        || canonical_port_bindings(&current_ports) != canonical_port_bindings(ports)
    {
        return Err(StorageError::ModuleSupportMaterialization {
            module_id: expected.summary.module_id.clone(),
            path: expected.config_dir.clone(),
            message: "Instance ownership or runtime changed during package preparation; retry the operation".to_owned(),
        });
    }
    Ok(())
}
