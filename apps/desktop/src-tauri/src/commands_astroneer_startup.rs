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
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const CONSOLE_TIMEOUT: Duration = Duration::from_secs(120);
const WORLD_TIMEOUT: Duration = Duration::from_secs(120);
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

fn enabled(settings: &Value) -> Result<bool, String> {
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
    if !enabled(&settings)? {
        return Ok(None);
    }
    let instance_root = super::commands_program_storage::instance_root(instance)?;
    if super::commands_program_storage::program_mode(instance)?
        != app_storage::InstanceProgramMode::Independent
    {
        return Err("ASTRONEER 首个世界只会在已准备好的独立实例程序中自动创建。".into());
    }
    let root = app_storage::resolve_instance_runtime_root(instance_root)
        .map_err(|error| error.to_string())?;
    let root = fs::canonicalize(root).map_err(|_| "ASTRONEER 独立程序目录不可用。")?;
    if root
        != fs::canonicalize(instance_root.join("runtime"))
            .map_err(|_| "ASTRONEER 独立程序归属无法核实。")?
    {
        return Err("ASTRONEER 程序不是当前实例的独立运行副本。".into());
    }
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
struct Run {
    run_id: i64,
    session: String,
    key: String,
    pid: u32,
    identity: ProcessIdentity,
}

fn run(instance: &InstanceDetails) -> Result<Run, String> {
    let active = instance
        .active_run
        .as_ref()
        .ok_or("ASTRONEER 当前启动记录不可用。")?;
    if !matches!(
        instance.summary.status,
        InstanceStatus::Running | InstanceStatus::Starting
    ) || active.process_count != 1
        || active.processes.len() != 1
        || instance.summary.active_process_count != 1
        || active.run_id <= 0
    {
        return Err("ASTRONEER 当前启动进程不唯一；未发送创建命令。".into());
    }
    let primary = &active.processes[0];
    let session = active
        .session_id
        .as_ref()
        .filter(|value| !value.is_empty())
        .ok_or("ASTRONEER 启动会话不可用。")?;
    if !primary.is_primary
        || primary.process_key.trim().is_empty()
        || primary.run_id != active.run_id
        || primary.session_id != active.session_id
        || primary.pid != active.pid
        || primary.status != "running"
        || primary.exit_code.is_some()
        || primary.crash_flag
    {
        return Err("ASTRONEER 当前启动记录发生变化。".into());
    }
    Ok(Run {
        run_id: active.run_id,
        session: session.clone(),
        key: primary.process_key.clone(),
        pid: primary
            .pid
            .filter(|pid| *pid > 0)
            .ok_or("ASTRONEER 当前进程不可用。")?,
        identity: primary
            .process_identity
            .clone()
            .filter(|value| value.creation_time > 0 && !value.image_path.is_empty())
            .ok_or("ASTRONEER 当前进程身份不可用。")?,
    })
}

fn tracked(
    supervisor: &Arc<Mutex<app_runtime::RuntimeSupervisor>>,
    id: &str,
    expected: &Run,
) -> Result<(), String> {
    let mut supervisor = supervisor
        .lock()
        .map_err(|_| "ASTRONEER 运行归属检查失败。")?;
    if !supervisor.tracked_instances().iter().any(|value| {
        value.summary.id == id
            && value.run_id == expected.run_id
            && value.session_id.as_deref() == Some(&expected.session)
            && value.pid == Some(expected.pid)
    }) || !supervisor
        .matches_running_process(id, expected.run_id, &expected.key, expected.pid)
        .unwrap_or(false)
    {
        return Err("ASTRONEER 已不属于本次启动；停止自动世界初始化。".into());
    }
    Ok(())
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
        return Err("ASTRONEER 已收到停止请求；停止自动世界初始化，服务器仍受管理。".into());
    }
    Ok(())
}

async fn current(
    state: &DesktopState,
    storage: &StorageBootstrap,
    plan: &Plan,
    expected: &Run,
    reservation: &RuntimeStartReservationLease,
) -> Result<(), String> {
    if reservation.is_cancelled() {
        return Err("ASTRONEER 启动初始化已取消；已有进程与数据仍受管理。".into());
    }
    check_stop_request(&state.runtime_restart_scheduler, &plan.instance_id)?;
    super::ensure_storage_context_snapshot_current(state, storage, "ASTRONEER first world")?;
    let instance = app_storage::read_instance_details(&storage.paths, &plan.instance_id)
        .await
        .map_err(|error| error.to_string())?;
    let actual = run(&instance)?;
    if instance.summary.module_id != "astroneer"
        || instance.config_file_path != plan.config_path
        || instance.saves_path != plan.declared_saves
        || instance.settings_json != plan.settings
        || port(&instance)? != plan.port
        || actual.run_id != expected.run_id
        || actual.session != expected.session
        || actual.key != expected.key
        || actual.pid != expected.pid
        || actual.identity != expected.identity
    {
        return Err("ASTRONEER 启动配置或进程代次已改变；停止自动世界初始化。".into());
    }
    let supervisor = state.runtime_supervisor.clone();
    let id = plan.instance_id.clone();
    let expected = expected.clone();
    spawn_blocking_storage_context_task(reservation, move || tracked(&supervisor, &id, &expected))
        .await
        .map_err(|_| "ASTRONEER 运行归属检查任务失败。")?
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

/// Call only after durable registration and supervisor.insert_running, while
/// retaining the same program/mutation guards in the start worker. Errors are
/// visible start warnings: they never stop, unregister or release the server.
pub(super) async fn initialize(
    state: &DesktopState,
    storage: &StorageBootstrap,
    instance: &InstanceDetails,
    plan: Plan,
    reservation: &RuntimeStartReservationLease,
) -> Result<(), String> {
    if instance.summary.id != plan.instance_id {
        return Err("ASTRONEER 初始化计划归属不匹配。".into());
    }
    let registered = app_storage::read_instance_details(&storage.paths, &plan.instance_id)
        .await
        .map_err(|error| error.to_string())?;
    let expected = run(&registered)?;
    let mut native: Option<NativeProcess> = None;
    let mut deadline = Instant::now() + CONSOLE_TIMEOUT;
    let mut submitted = false;
    loop {
        current(state, storage, &plan, &expected, reservation).await?;
        if Instant::now() >= deadline {
            return Err(if submitted {
                "ASTRONEER 首个世界创建结果超时未知；服务器与数据已保留，不会自动重发。"
            } else {
                "ASTRONEER 原生控制台尚未就绪；未自动创建世界，服务器仍受管理。"
            }
            .into());
        }
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
                Ok(expected)
            } else {
                process::capture(
                    GameProfile::Astroneer,
                    &root,
                    root_run.pid,
                    &root_run.identity,
                )
            }
        })
        .await
        .map_err(|_| "ASTRONEER 原生进程身份检查任务失败。")?;
        match captured {
            Ok(process) => native = Some(process),
            Err(error) if native.is_some() => return Err(error),
            Err(_) => {
                tokio::time::sleep(POLL).await;
                continue;
            }
        }
        let responder = native.as_ref().ok_or("ASTRONEER Shipping 进程不可用。")?;
        let games = console::query(
            plan.port,
            &plan.password,
            Query::Games,
            responder.pid,
            &responder.identity,
        )
        .await;
        let statistics = console::query(
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
                deadline = Instant::now() + WORLD_TIMEOUT;
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
                        tracked(&state.runtime_supervisor, &plan.instance_id, &expected)?;
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
