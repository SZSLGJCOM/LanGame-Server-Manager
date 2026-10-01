use super::*;

#[derive(Debug, Serialize)]
pub struct InstanceConnectionInfo {
    pub instance_id: String,
    pub bind_ip: String,
    pub ports: Vec<PortBinding>,
    pub settings_json: String,
}

#[tauri::command]
pub async fn read_instance_connection_info_from_storage(
    state: tauri::State<'_, DesktopState>,
    instance_ids: Vec<String>,
) -> Result<Vec<InstanceConnectionInfo>, String> {
    if instance_ids.len() > app_storage::MAX_INSTANCE_PORT_PROJECTION_INSTANCES {
        return Err(
            app_storage::StorageError::InstancePortProjectionLimitExceeded {
                max: app_storage::MAX_INSTANCE_PORT_PROJECTION_INSTANCES,
                actual: instance_ids.len(),
            }
            .to_string(),
        );
    }
    if instance_ids.is_empty() {
        return Ok(Vec::new());
    }
    let _storage_context_operation =
        state.begin_storage_context_operation("instance connection inspection")?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    let projections = app_storage::read_instance_port_projections(&storage.paths, &instance_ids)
        .await
        .map_err(|error| error.to_string())?;
    let mut connections = Vec::with_capacity(projections.len());
    for projection in projections {
        let mut connection = InstanceConnectionInfo {
            instance_id: projection.instance_id,
            bind_ip: projection.bind_ip,
            ports: projection.ports,
            settings_json: String::from("{}"),
        };
        if projection.module_id == "corekeeper" {
            let details = read_instance_details(&storage.paths, &connection.instance_id)
                .await
                .map_err(|error| error.to_string())?;
            connection.settings_json = corekeeper_connection_settings(&details.settings_json)?;
            connection.bind_ip = details.summary.bind_ip;
            connection.ports = details.ports;
        }
        connections.push(connection);
    }
    Ok(connections)
}

fn corekeeper_connection_settings(settings_json: &str) -> Result<String, String> {
    let settings: Value = serde_json::from_str(settings_json).map_err(|error| error.to_string())?;
    let settings = settings
        .as_object()
        .ok_or("instance settings must be an object")?;
    let mut connection = serde_json::Map::new();
    for (key, is_valid) in [
        ("game_id", Value::is_string as fn(&Value) -> bool),
        ("direct_connection_enabled", Value::is_boolean),
    ] {
        if let Some(value) = settings.get(key) {
            if !is_valid(value) {
                return Err(format!("invalid Core Keeper connection setting `{key}`"));
            }
            connection.insert(key.to_owned(), value.clone());
        }
    }
    serde_json::to_string(&connection).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corekeeper_connection_info_only_exposes_typed_join_settings() {
        let settings = corekeeper_connection_settings(
            r#"{
            "game_id":"testjoin123", "direct_connection_enabled":false,
            "server_password":"private", "rcon_password":"private", "other":"private"
        }"#,
        )
        .unwrap();
        let value: Value = serde_json::from_str(&settings).unwrap();
        assert_eq!(
            value,
            json!({"game_id":"testjoin123", "direct_connection_enabled":false})
        );
        assert_eq!(corekeeper_connection_settings("{}").unwrap(), "{}");
        assert!(corekeeper_connection_settings(r#"{"game_id":42}"#).is_err());
        assert!(
            corekeeper_connection_settings(r#"{"direct_connection_enabled":"false"}"#).is_err()
        );
        assert!(corekeeper_connection_settings("[]").is_err());
    }

    #[tokio::test]
    async fn connection_info_command_handles_empty_and_oversized_batches_before_storage_access() {
        let app = tauri::test::mock_builder()
            .manage(DesktopState::default())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        assert!(
            read_instance_connection_info_from_storage(app.state::<DesktopState>(), Vec::new())
                .await
                .unwrap()
                .is_empty()
        );
        let error = read_instance_connection_info_from_storage(
            app.state::<DesktopState>(),
            vec![String::from("missing"); app_storage::MAX_INSTANCE_PORT_PROJECTION_INSTANCES + 1],
        )
        .await
        .unwrap_err();
        assert!(error.contains("128"), "{error}");
        assert!(error.contains("129"), "{error}");
    }
}
