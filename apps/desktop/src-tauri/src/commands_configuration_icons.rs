use std::collections::BTreeMap;

use base64::Engine;

use super::*;

// Decoding stays on a blocking worker. The permit follows that worker even when
// the requesting webview disappears, so cancelled requests cannot pile up decoders.
static ICON_READ_GATE: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);
static ICON_REQUEST_GATE: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(4);

#[tauri::command]
pub async fn read_module_configuration_icons(
    state: tauri::State<'_, DesktopState>,
    module_id: String,
) -> Result<BTreeMap<String, String>, String> {
    if module_id != "dontstarve" {
        return Ok(BTreeMap::new());
    }
    let lease = state.begin_storage_context_operation("configuration icon loading")?;
    let request_permit = ICON_REQUEST_GATE
        .try_acquire()
        .map_err(|_| String::from("too many configuration icon requests"))?;
    let permit = tokio::time::timeout(std::time::Duration::from_secs(10), ICON_READ_GATE.acquire())
        .await
        .map_err(|_| String::from("configuration icon loading timed out while waiting"))?
        .map_err(|_| String::from("configuration icon loading is unavailable"))?;
    let (storage, descriptor, permit, request_permit) =
        spawn_blocking_storage_context_task(&lease, move || {
            let storage = bootstrap_storage().map_err(|error| error.to_string())?;
            let descriptors =
                discover_modules(&storage.paths.modules_root).map_err(|error| error.to_string())?;
            let descriptor = find_descriptor(&descriptors, "dontstarve")?.clone();
            Ok::<_, String>((storage, descriptor, permit, request_permit))
        })
        .await
        .map_err(|error| format!("configuration icon setup failed: {error}"))??;
    let install_root_override = resolve_module_install_root(&storage.paths, "dontstarve")
        .await
        .map_err(|error| format!("configuration icon installation lookup failed: {error}"))?;

    spawn_blocking_storage_context_task(&lease, move || {
        let _permit = permit;
        let _request_permit = request_permit;
        let probe = probe_module_install_state_with_override(
            &storage.settings,
            "dontstarve",
            descriptor.summary.steam_app_id,
            descriptor.install.as_ref(),
            descriptor.process.as_ref(),
            install_root_override.as_deref(),
        );
        let schema = descriptor
            .schema_json
            .as_deref()
            .ok_or_else(|| String::from("Don't Starve Together configuration schema is missing"))?;
        let icons = app_modules::load_dst_configuration_icons(
            Path::new(&probe.install_root),
            &storage.paths.app_data_root,
            schema,
        )
        .map_err(|error| {
            let message = format!("Don't Starve Together configuration icons: {error}");
            append_desktop_app_log(
                &storage,
                "warn",
                "module.configuration_icons.failed",
                &message,
                json!({ "module_id": "dontstarve" }),
            );
            message
        })?;
        for warning in icons.warnings {
            append_desktop_app_log(
                &storage,
                "warn",
                "module.configuration_icons.cache_warning",
                &warning,
                json!({ "module_id": "dontstarve" }),
            );
        }
        Ok(icons
            .icons
            .into_iter()
            .map(|(key, png)| {
                let encoded = base64::engine::general_purpose::STANDARD.encode(png);
                (key, format!("data:image/png;base64,{encoded}"))
            })
            .collect())
    })
    .await
    .map_err(|error| format!("configuration icon decoding task failed: {error}"))?
}
