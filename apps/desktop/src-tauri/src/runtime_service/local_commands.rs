//! Commands authorized only through the current-user named pipe. Keeping
//! destructive maintenance out of the HTTP dispatcher preserves the LAN boundary.
use crate::commands::commands_ark_cluster_backups as backups;
use crate::commands::commands_nexus_mods as nexus;
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
        "nexus_account_status" => Some(Box::pin(async move {
            to_json(nexus::nexus_account_status().await?)
        })),
        "nexus_store_api_key" => Some(Box::pin(async move {
            to_json(nexus::nexus_store_api_key(string_argument(&args, "apiKey")?).await?)
        })),
        "nexus_clear_api_key" => Some(Box::pin(async move {
            to_json(nexus::nexus_clear_api_key().await?)
        })),
        "review_nexus_mod_references" => Some(Box::pin(async move {
            let request: NexusReviewInput = serde_json::from_value(args)
                .map_err(|_| String::from("Invalid Nexus Mod review input."))?;
            to_json(
                nexus::review_nexus_mod_references(
                    app.state::<DesktopState>(),
                    request.instance_id,
                    request.references,
                )
                .await?,
            )
        })),
        "install_nexus_mod_files" => Some(Box::pin(async move {
            let request: NexusInstallInput = serde_json::from_value(args)
                .map_err(|_| String::from("Invalid Nexus Mod installation input."))?;
            to_json(
                nexus::install_nexus_mod_files(
                    app.state::<DesktopState>(),
                    request.instance_id,
                    request.selections,
                )
                .await?,
            )
        })),
        "start_steam_workshop_account_login" => Some(Box::pin(async move {
            to_json(crate::commands::commands_steam_workshop_account::start_steam_workshop_account_login(
                app.state::<DesktopState>(), steam_login_target_argument(&args)?, string_argument(&args, "sessionId")?,
            ).await?)
        })),
        "read_steam_workshop_account_login" => Some(Box::pin(async move {
            to_json(crate::commands::commands_steam_workshop_account::read_steam_workshop_account_login(
                app.state::<DesktopState>(), steam_login_target_argument(&args)?, string_argument(&args, "sessionId")?,
            )?)
        })),
        "send_steam_workshop_account_input" => Some(Box::pin(async move {
            // Never include a rejected credential value in decoding errors.
            let revision = args
                .get("expectedRevision")
                .and_then(Value::as_u64)
                .ok_or("Missing or invalid expectedRevision")?;
            to_json(crate::commands::commands_steam_workshop_account::send_steam_workshop_account_input(
                app.state::<DesktopState>(), steam_login_target_argument(&args)?,
                string_argument(&args, "sessionId")?, revision, string_argument(&args, "input")?,
            )?)
        })),
        "cancel_steam_workshop_account_login" => Some(Box::pin(async move {
            to_json(crate::commands::commands_steam_workshop_account::cancel_steam_workshop_account_login(
                app.state::<DesktopState>(), steam_login_target_argument(&args)?, string_argument(&args, "sessionId")?,
            )?)
        })),
        "download_authenticated_steam_workshop_items" => Some(Box::pin(async move {
            let request: WorkshopAccountDownloadInput = serde_json::from_value(args)
                .map_err(|error| format!("Invalid Workshop account download input: {error}"))?;
            to_json(crate::commands::commands_steam_workshop_account::download_authenticated_steam_workshop_items(
                app.clone(), request.instance_id, request.ids, request.account_name,
                request.missing_only, request.locale,
            ).await?)
        })),
        "install_authenticated_module_game" => Some(Box::pin(async move {
            let request: SteamModuleAccountInstallInput = serde_json::from_value(args)
                .map_err(|_| String::from("Invalid authenticated server installation input."))?;
            to_json(
                crate::commands::commands_steam_app_account::install_authenticated_module_game(
                    app.clone(),
                    request.module_id,
                    request.validate,
                    request.account_name,
                    request.target,
                )
                .await?,
            )
        })),
        "clear_steam_server_account_authorization" => Some(Box::pin(async move {
            to_json(crate::commands::commands_steam_app_account::clear_steam_server_account_authorization(
                app.state::<DesktopState>(), steam_login_target_argument(&args)?,
            )?)
        })),
        "update_authenticated_instance_program" => {
            Some(Box::pin(async move {
                let request: SteamInstanceAccountUpdateInput = serde_json::from_value(args)
                    .map_err(|_| String::from("Invalid authenticated instance update input."))?;
                to_json(crate::commands::commands_steam_app_account::update_authenticated_instance_program(
                app.clone(), request.instance_id, request.validate, request.account_name,
            ).await?)
            }))
        }
        "read_instance_save_import_support" => Some(Box::pin(async move {
            to_json(
                crate::commands::commands_save_import::read_instance_save_import_support(
                    app.state::<DesktopState>(),
                    string_argument(&args, "instanceId")?,
                )
                .await?,
            )
        })),
        "preview_instance_save_import" => Some(Box::pin(async move {
            to_json(
                crate::commands::commands_save_import::preview_instance_save_import(
                    app.state::<DesktopState>(),
                    string_argument(&args, "instanceId")?,
                    string_argument(&args, "sourcePath")?,
                )
                .await?,
            )
        })),
        "import_instance_save" => Some(Box::pin(async move {
            to_json(
                crate::commands::commands_save_import::import_instance_save(
                    app.state::<DesktopState>(),
                    string_argument(&args, "instanceId")?,
                    string_argument(&args, "sourcePath")?,
                    string_argument(&args, "expectedFingerprint")?,
                )
                .await?,
            )
        })),
        "uninstall_steamcmd" => Some(Box::pin(async move {
            to_json(crate::commands::uninstall_steamcmd(app.state::<DesktopState>()).await?)
        })),
        "preview_dontstarve_world_start" => Some(Box::pin(async move {
            let id = args
                .get("instanceId")
                .or_else(|| args.get("instance_id"))
                .and_then(Value::as_str)
                .ok_or("Missing instance ID")?;
            to_json(
                crate::commands::commands_dst_world_state::preview_dontstarve_world_start(
                    app.state::<DesktopState>(),
                    id.into(),
                )
                .await?,
            )
        })),
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

fn string_argument(args: &Value, name: &str) -> Result<String, String> {
    args.get(name)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("Missing or invalid {name}"))
}

fn steam_login_target_argument(
    args: &Value,
) -> Result<crate::steam_workshop_account_sessions::SteamAccountLoginTarget, String> {
    serde_json::from_value(
        args.get("target")
            .cloned()
            .ok_or("Missing SteamCMD login target")?,
    )
    .map_err(|_| String::from("Invalid SteamCMD login target."))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct NexusReviewInput {
    instance_id: String,
    references: Vec<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct NexusInstallInput {
    instance_id: String,
    selections: Vec<crate::commands::nexus_mods::NexusFileSelection>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SteamModuleAccountInstallInput {
    module_id: String,
    validate: bool,
    account_name: String,
    target: crate::steam_workshop_account_sessions::SteamAccountLoginTarget,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SteamInstanceAccountUpdateInput {
    instance_id: String,
    validate: bool,
    account_name: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkshopAccountDownloadInput {
    instance_id: String,
    ids: Vec<String>,
    account_name: String,
    missing_only: Option<bool>,
    locale: Option<String>,
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
