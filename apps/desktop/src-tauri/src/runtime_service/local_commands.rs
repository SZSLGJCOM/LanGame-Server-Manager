//! Commands authorized only through the current-user named pipe. Keeping
//! destructive maintenance out of the HTTP dispatcher preserves the LAN boundary.
use crate::commands::commands_ark_cluster_backups as backups;
use crate::commands::commands_storage_management as storage;
use crate::state::DesktopState;
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use tauri::Manager;

type Command<'a> = Pin<Box<dyn Future<Output = Result<Value, String>> + 'a>>;

pub(super) fn dispatch<'a>(
    app: &'a tauri::AppHandle,
    command: &str,
    args: Value,
) -> Option<Command<'a>> {
    // Box each construction separately, as in the shared dispatcher. Large
    // native command futures must not accumulate on a connection worker's stack.
    match command {
        "read_ark_tools_status" => Some(Box::pin(async move {
            to_json(
                crate::commands::commands_ark_tools::read_ark_tools_status(
                    app.state::<DesktopState>(),
                    input(args)?,
                )
                .await?,
            )
        })),
        "prepare_ark_tools" => Some(Box::pin(async move {
            to_json(
                crate::commands::commands_ark_tools::prepare_ark_tools(
                    app.state::<DesktopState>(),
                    input(args)?,
                )
                .await?,
            )
        })),
        "spawn_ark_creature" => Some(Box::pin(async move {
            to_json(
                crate::commands::commands_ark_tools::spawn_ark_creature(
                    app.state::<DesktopState>(),
                    input(args)?,
                )
                .await?,
            )
        })),
        "read_knowledge_status" => Some(Box::pin(async move {
            to_json(
                crate::commands_knowledge::read_knowledge_status(app.state::<DesktopState>())
                    .await?,
            )
        })),
        "update_knowledge_settings" => Some(Box::pin(async move {
            to_json(
                crate::commands_knowledge::update_knowledge_settings(
                    app.state::<DesktopState>(),
                    input(args)?,
                )
                .await?,
            )
        })),
        "start_knowledge_sync" => Some(Box::pin(async move {
            to_json(
                crate::commands_knowledge::start_knowledge_sync(
                    app.state::<DesktopState>(),
                    input(args)?,
                )
                .await?,
            )
        })),
        "cancel_knowledge_sync" => Some(Box::pin(async move {
            to_json(crate::commands_knowledge::cancel_knowledge_sync(
                app.state::<DesktopState>(),
                input(args)?,
            )?)
        })),
        "scan_storage_usage" => Some(Box::pin(async move {
            to_json(storage::scan_storage_usage(app.state::<DesktopState>(), input(args)?).await?)
        })),
        "cancel_storage_usage_scan" => Some(Box::pin(async move {
            to_json(storage::cancel_storage_usage_scan(
                app.state::<DesktopState>(),
                input(args)?,
            )?)
        })),
        "archive_instance_record" => Some(Box::pin(async move {
            let id = args
                .get("instanceId")
                .or_else(|| args.get("instance_id"))
                .and_then(Value::as_str)
                .ok_or("Missing instance ID")?
                .to_owned();
            to_json(crate::commands::archive_instance_record(app.clone(), id).await?)
        })),
        "list_instance_archives" => Some(Box::pin(async move {
            to_json(storage::list_instance_archives(app.state::<DesktopState>()).await?)
        })),
        "read_instance_archive_details" => Some(Box::pin(async move {
            to_json(
                storage::read_instance_archive_details(app.state::<DesktopState>(), input(args)?)
                    .await?,
            )
        })),
        "restore_instance_archive" => Some(Box::pin(async move {
            to_json(storage::restore_instance_archive(app.clone(), input(args)?).await?)
        })),
        "purge_instance_archive" => Some(Box::pin(async move {
            to_json(
                storage::purge_instance_archive(app.state::<DesktopState>(), input(args)?).await?,
            )
        })),
        "list_ark_cluster_backups" => Some(Box::pin(async move {
            to_json(
                backups::list_ark_cluster_backups(app.state::<DesktopState>(), input(args)?)
                    .await?,
            )
        })),
        "create_ark_cluster_backup" => Some(Box::pin(async move {
            to_json(
                backups::create_ark_cluster_backup(app.state::<DesktopState>(), input(args)?)
                    .await?,
            )
        })),
        "restore_ark_cluster_backup" => Some(Box::pin(async move {
            to_json(
                backups::restore_ark_cluster_backup(app.state::<DesktopState>(), input(args)?)
                    .await?,
            )
        })),
        "read_pending_ark_cluster_restore" => Some(Box::pin(async move {
            to_json(
                backups::read_pending_ark_cluster_restore(
                    app.state::<DesktopState>(),
                    input(args)?,
                )
                .await?,
            )
        })),
        "recover_ark_cluster_restore" => Some(Box::pin(async move {
            to_json(
                backups::recover_ark_cluster_restore(app.state::<DesktopState>(), input(args)?)
                    .await?,
            )
        })),
        _ => None,
    }
}

fn input<T: serde::de::DeserializeOwned>(mut args: Value) -> Result<T, String> {
    let value = args
        .get_mut("input")
        .ok_or("Missing operation input")?
        .take();
    serde_json::from_value(value).map_err(|error| format!("Invalid operation input: {error}"))
}

fn to_json<T: serde::Serialize>(value: T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|error| error.to_string())
}
