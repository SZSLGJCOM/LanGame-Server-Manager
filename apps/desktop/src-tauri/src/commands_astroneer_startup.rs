//! First-world preparation under the existing start reservation and program locks.
use crate::astroneer_console::{Query, startup as console};
use crate::live_players::file_ipc::unreal_save_process::{
    self as process, GameProfile, NativeProcess,
};
use crate::live_players::file_ipc::unreal_world_save::installation;
use crate::state::{
    DesktopState, RuntimeStartReservationLease, spawn_blocking_storage_context_task,
};
use app_core::{InstanceDetails, InstanceStatus, ProcessIdentity};
use app_storage::StorageBootstrap;
use serde_json::Value;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

const POLL: Duration = Duration::from_secs(1);

// No Clone/Debug: a plan authorizes one submission and contains a credential.
pub(super) struct Plan {
    instance_id: String,
    config_path: String,
    declared_saves: String,
    settings: String,
    root: PathBuf,
    saves: PathBuf,
    label: String,
    password: String,
    port: u16,
    _pins: Vec<File>,
}

fn empty(path: &Path) -> Result<bool, String> {
    fs::read_dir(path)
        .map_err(|_| "无法检查 ASTRONEER 存档目录；未自动创建世界。".to_owned())?
        .next()
        .transpose()
        .map(|entry| entry.is_none())
        .map_err(|_| "ASTRONEER 存档目录检查不完整；未自动创建世界。".into())
}

pub(super) fn automatic_world_loading(settings: &Value) -> Result<bool, String> {
    match (
        settings.get("load_auto_save").and_then(Value::as_bool),
        settings
            .get("disable_server_travel")
            .and_then(Value::as_bool),
    ) {
        (Some(false), _) | (_, Some(true)) => Ok(false),
        (Some(true), Some(false)) => Ok(true),
        _ => Err("ASTRONEER 世界自动加载/切换配置不完整；未自动创建世界。".into()),
    }
}

fn port(instance: &InstanceDetails) -> Result<u16, String> {
    let mut ports = instance.ports.iter().filter(|port| port.name == "console");
    let binding = ports.next().ok_or("ASTRONEER 缺少原生控制台端口。")?;
    if binding.port == 0 || !binding.protocol.eq_ignore_ascii_case("tcp") || ports.next().is_some()
    {
        return Err("ASTRONEER 原生控制台端口配置无效。".into());
    }
    Ok(binding.port)
}

/// Caller owns the stopped instance mutation lock and independent program lock.
/// Call after automatic private-program/loader preparation, before spawning.
pub(super) fn prepare(instance: &InstanceDetails) -> Result<Option<Plan>, String> {
    if instance.summary.module_id != "astroneer" {
        return Ok(None);
    }
    let settings: Value = serde_json::from_str(&instance.settings_json)
        .map_err(|_| "ASTRONEER 世界初始化配置无效。")?;
    if !automatic_world_loading(&settings)? {
        return Ok(None);
    }
    let instance_root = super::commands_program_storage::instance_root(instance)?;
    if super::commands_program_storage::program_mode(instance)?
        != app_storage::InstanceProgramMode::Independent
    {
        return Err("ASTRONEER 首个世界只会在已准备好的独立实例程序中自动创建。".into());
    }
    // Independent programs include both private runtime copies and exclusively
    // owned library installations. Storage validates either ownership layout.
    let root = app_storage::resolve_instance_runtime_root(instance_root)
        .map_err(|error| error.to_string())?;
    let root = fs::canonicalize(root).map_err(|_| "ASTRONEER 独立程序目录不可用。")?;
    let saves = root.join("Astro/Saved/SaveGames");
    let pins = installation::pin(&saves, true)?;
    let saves = fs::canonicalize(saves).map_err(|_| "ASTRONEER 存档目录不可用。")?;
    if fs::canonicalize(&instance.saves_path).ok().as_ref() != Some(&saves) {
        return Err("ASTRONEER 声明的存档目录与当前独立运行副本不一致；未自动创建世界。".into());
    }
    // Every directory entry blocks creation, including unknown files, folders,
    // incomplete saves, backup files and reparse points. No extension allowlist.
    if !empty(&saves)? {
        return Ok(None);
    }
    if instance.active_run.is_some()
        || instance.summary.active_process_count != 0
        || !matches!(
            instance.summary.status,
            InstanceStatus::Stopped | InstanceStatus::Error
        )
    {
        return Err("ASTRONEER 首个世界预检需要已停止的实例。".into());
    }
    process::stopped(GameProfile::Astroneer, &root)?;
    let label = settings
        .get("active_save_file_name")
        .and_then(Value::as_str)
        .ok_or("ASTRONEER 缺少存档名称；原配置已保留。")?
        .to_owned();
    console::new_game_command(&label)?;
    let password = settings
        .get("console_password")
        .and_then(Value::as_str)
        .filter(|value| {
            !value.trim().is_empty()
                && value.chars().count() <= 128
                && !value.chars().any(char::is_control)
        })
        .ok_or("ASTRONEER 自动初始化需要有效的原生控制台密码。")?
        .to_owned();
    Ok(Some(Plan {
        instance_id: instance.summary.id.clone(),
        config_path: instance.config_file_path.clone(),
        declared_saves: instance.saves_path.clone(),
        settings: instance.settings_json.clone(),
        root,
        saves,
        label,
        password,
        port: port(instance)?,
        _pins: pins,
    }))
}

#[derive(Clone)]
pub(super) struct SpawnedTarget {
    pub pid: u32,
    pub identity: ProcessIdentity,
}

impl SpawnedTarget {
    fn verify(&self) -> Result<(), String> {
        if self.pid == 0
            || self.identity.creation_time == 0
            || self.identity.image_path.is_empty()
            || !app_runtime::process_matches_identity(self.pid, &self.identity)
                .map_err(|_| "ASTRONEER 启动进程归属检查失败。")?
        {
            return Err("ASTRONEER 本次启动进程已退出或身份已改变。".into());
        }
        Ok(())
    }
}

fn check_stop_request(
    scheduler: &Mutex<crate::state::RuntimeRestartScheduler>,
    instance_id: &str,
) -> Result<(), String> {
    // Stop publishes this flag before waiting for our instance mutation lock.
    // Manual starts have already cleared the preceding run's flag. In contrast,
    // restart_allowed requires a restart flight and cannot gate manual starts.
    if !scheduler
        .lock()
        .map_err(|_| "ASTRONEER 停止请求检查失败。")?
        .accepts_exit(instance_id)
    {
        return Err("ASTRONEER 已收到停止请求；停止自动世界初始化，已有存档已保留。".into());
    }
    Ok(())
}

async fn current(
    state: &DesktopState,
    storage: &StorageBootstrap,
    plan: &Plan,
    expected: &SpawnedTarget,
    reservation: &RuntimeStartReservationLease,
) -> Result<(), String> {
    reservation.ensure_current_for(&plan.instance_id)?;
    check_stop_request(&state.runtime_restart_scheduler, &plan.instance_id)?;
    super::ensure_storage_context_snapshot_current(state, storage, "ASTRONEER first world")?;
    let instance = app_storage::read_instance_details(&storage.paths, &plan.instance_id)
        .await
        .map_err(|error| error.to_string())?;
    validate_unregistered(&instance, plan)?;
    let expected = expected.clone();
    spawn_blocking_storage_context_task(reservation, move || expected.verify())
        .await
        .map_err(|_| "ASTRONEER 运行归属检查任务失败。")??;
    reservation.ensure_current_for(&plan.instance_id)
}

fn validate_unregistered(instance: &InstanceDetails, plan: &Plan) -> Result<(), String> {
    if instance.summary.id != plan.instance_id
        || instance.summary.module_id != "astroneer"
        || instance.config_file_path != plan.config_path
        || instance.saves_path != plan.declared_saves
        || instance.settings_json != plan.settings
        || port(instance)? != plan.port
        || instance.active_run.is_some()
        || instance.summary.active_process_count != 0
        || !matches!(
            instance.summary.status,
            InstanceStatus::Stopped | InstanceStatus::Error | InstanceStatus::Starting
        )
    {
        return Err("ASTRONEER 启动配置或运行记录已改变；停止自动世界初始化。".into());
    }
    Ok(())
}

fn selected<'a>(games: &'a Value, statistics: &Value) -> Option<&'a str> {
    let active = games.get("activeSaveName")?.as_str()?;
    if games.get("gameList")?.as_array().is_some()
        && !active.trim().is_empty()
        && active.chars().count() <= 256
        && !active.chars().any(char::is_control)
        && statistics.get("saveGameName")?.as_str()? == active
    {
        Some(active)
    } else {
        None
    }
}

fn native_empty(games: &Value, statistics: &Value) -> bool {
    games
        .get("gameList")
        .and_then(Value::as_array)
        .is_some_and(Vec::is_empty)
        && games.get("activeSaveName").and_then(Value::as_str) == Some("")
        && statistics.get("saveGameName").and_then(Value::as_str) == Some("")
}

/// Call before durable Running registration, retaining the same reservation,
/// owned bootstrap handle and program/mutation guards. The caller owns the
/// common deadline, cancellation and failed-start cleanup; native data is kept.
pub(super) async fn initialize_spawned(
    state: &DesktopState,
    storage: &StorageBootstrap,
    instance: &InstanceDetails,
    plan: Plan,
    reservation: &RuntimeStartReservationLease,
    expected: SpawnedTarget,
) -> Result<(), String> {
    validate_unregistered(instance, &plan)?;
    reservation.ensure_current_for(&plan.instance_id)?;
    let mut native: Option<NativeProcess> = None;
    let mut submitted = false;
    loop {
        current(state, storage, &plan, &expected, reservation).await?;
        let root = plan.root.clone();
        let root_run = expected.clone();
        let prior = native.clone();
        let captured = spawn_blocking_storage_context_task(reservation, move || {
            if let Some(expected) = prior {
                process::verify(
                    GameProfile::Astroneer,
                    &root,
                    root_run.pid,
                    &root_run.identity,
                    &expected,
                )?;
                Ok(Some(expected))
            } else {
                process::capture_optional(
                    GameProfile::Astroneer,
                    &root,
                    root_run.pid,
                    &root_run.identity,
                )
            }
        })
        .await
        .map_err(|_| "ASTRONEER 原生进程身份检查任务失败。")??;
        match captured {
            Some(process) => native = Some(process),
            None => {
                tokio::select! { biased; _ = reservation.cancelled() => {}, _ = tokio::time::sleep(POLL) => {} }
                continue;
            }
        }
        let responder = native.as_ref().ok_or("ASTRONEER Shipping 进程不可用。")?;
        let games = console::query_checked(
            plan.port,
            &plan.password,
            Query::Games,
            responder.pid,
            &responder.identity,
        )
        .await;
        let statistics = console::query_checked(
            plan.port,
            &plan.password,
            Query::Statistics,
            responder.pid,
            &responder.identity,
        )
        .await;
        current(state, storage, &plan, &expected, reservation).await?;
        let root = plan.root.clone();
        let expected_run = expected.clone();
        let responder_after = responder.clone();
        spawn_blocking_storage_context_task(reservation, move || {
            process::verify(
                GameProfile::Astroneer,
                &root,
                expected_run.pid,
                &expected_run.identity,
                &responder_after,
            )
        })
        .await
        .map_err(|_| "ASTRONEER 原生进程复核任务失败。")??;
        if let Err(console::QueryError::Ownership(message)) = &games {
            return Err(message.clone());
        }
        if let Err(console::QueryError::Ownership(message)) = &statistics {
            return Err(message.clone());
        }
        if let (Ok(games), Ok(statistics)) = (games, statistics) {
            if let Some(active) = selected(&games, &statistics) {
                if !submitted || active == plan.label {
                    return Ok(());
                }
                return Err("ASTRONEER 实际启用的世界与请求名称不一致；未重试或覆盖数据。".into());
            }
            if !submitted && native_empty(&games, &statistics) {
                if !empty(&plan.saves)? {
                    return Err(
                        "ASTRONEER 已出现存档或未知目录对象；停止自动创建，已有数据已保留。".into(),
                    );
                }
                submitted = true;
                console::create_once(
                    plan.port,
                    &plan.password,
                    &plan.label,
                    responder.pid,
                    &responder.identity,
                    || {
                        if reservation.is_cancelled() || !empty(&plan.saves)? {
                            return Err("ASTRONEER 世界创建前状态已改变；未重发命令。".into());
                        }
                        reservation.ensure_current_for(&plan.instance_id)?;
                        expected.verify()?;
                        process::verify(
                            GameProfile::Astroneer,
                            &plan.root,
                            expected.pid,
                            &expected.identity,
                            responder,
                        )?;
                        if reservation.is_cancelled() {
                            return Err("ASTRONEER 启动初始化已取消；未发送创建命令。".into());
                        }
                        check_stop_request(&state.runtime_restart_scheduler, &plan.instance_id)
                    },
                )
                .await?;
                continue;
            }
            if !submitted
                && games
                    .get("gameList")
                    .and_then(Value::as_array)
                    .is_some_and(|games| !games.is_empty())
            {
                return Err("ASTRONEER 已发现现有存档；跳过自动创建，未改变世界选择。".into());
            }
        }
        tokio::select! { biased; _ = reservation.cancelled() => {}, _ = tokio::time::sleep(POLL) => {} }
    }
}

#[cfg(test)]
#[path = "commands_astroneer_startup_tests.rs"]
mod tests;
