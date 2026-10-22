//! Closed native-save bridges. Only a request-bound native completion can
//! confirm a write; returning from a void invocation remains unconfirmed.
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use app_core::{InstanceDetails, InstanceStatus, ProcessIdentity};
use serde::{Deserialize, Serialize};
use tokio::sync::OwnedMutexGuard;

use crate::commands::{InstanceRuntimeCommandResult, WorldSaveCompletion};
use crate::state::{DesktopState, StorageContextTaskLease, spawn_blocking_storage_context_task};

use super::transport;
use super::unreal_save_process as process;

#[path = "unreal_save_installation.rs"]
pub(crate) mod installation;
#[path = "unreal_save_retired.rs"]
mod retired;
pub(crate) use retired::cleanup_retired;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Game {
    Abiotic,
    Moria,
    Nightingale,
    Conan,
    Scum,
}
impl Game {
    fn from_details(details: &InstanceDetails) -> Result<Self, String> {
        match details.summary.module_id.as_str() {
            "abioticfactor" => Ok(Self::Abiotic),
            "returntomoria" => Ok(Self::Moria),
            "nightingale" => Ok(Self::Nightingale),
            "conanexiles" => Ok(Self::Conan),
            "scum" => Ok(Self::Scum),
            _ => Err("This module has no declared Unreal world-save bridge.".into()),
        }
    }
    fn id(self) -> &'static str {
        match self {
            Self::Abiotic => "abioticfactor",
            Self::Moria => "returntomoria",
            Self::Nightingale => "nightingale",
            Self::Conan => "conanexiles",
            Self::Scum => "scum",
        }
    }
    fn project(self) -> &'static str {
        match self {
            Self::Abiotic => "AbioticFactor",
            Self::Moria => "Moria",
            Self::Nightingale => "NWX",
            Self::Conan => "ConanSandbox",
            Self::Scum => "SCUM",
        }
    }
    fn executable(self) -> &'static str {
        match self {
            Self::Abiotic => "AbioticFactorServer-Win64-Shipping.exe",
            Self::Moria => "MoriaServer-Win64-Shipping.exe",
            Self::Nightingale => "NWXServer-Win64-Shipping.exe",
            Self::Conan => "ConanSandboxServer-Win64-Shipping.exe",
            Self::Scum => "SCUMServer.exe",
        }
    }
    fn win64(self, root: &Path) -> PathBuf {
        root.join(self.project()).join("Binaries/Win64")
    }
    fn payload(self) -> Vec<u8> {
        let policy = match self {
            Self::Abiotic => include_str!(
                "../../../../../modules/abioticfactor/control/LgsmWorldSave/Scripts/main.lua"
            ),
            Self::Moria => include_str!(
                "../../../../../modules/returntomoria/control/LgsmWorldSave/Scripts/main.lua"
            ),
            Self::Nightingale => include_str!(
                "../../../../../modules/nightingale/control/LgsmWorldSave/Scripts/main.lua"
            ),
            Self::Conan => include_str!(
                "../../../../../modules/conanexiles/control/LgsmWorldSave/Scripts/main.lua"
            ),
            Self::Scum => {
                include_str!("../../../../../modules/scum/control/LgsmWorldSave/Scripts/main.lua")
            }
        };
        [
            policy.as_bytes(),
            b"\n",
            include_bytes!("unreal_save_protocol.lua"),
        ]
        .concat()
    }
    fn profile(self) -> &'static str {
        match self {
            Self::Conan => {
                include_str!("../../../../../modules/conanexiles/control/UE4SS-settings.ini")
            }
            Self::Scum => include_str!("../../../../../modules/scum/extensions/UE4SS-settings.ini"),
            _ => include_str!("../../../../../modules/abioticfactor/control/UE4SS-settings.ini"),
        }
    }
    fn native_payload(self) -> Option<(&'static str, &'static [u8])> {
        #[cfg(windows)]
        match self {
            Self::Moria => Some((
                "native/LgsmMoriaSaveControl.dll",
                include_bytes!(concat!(
                    env!("OUT_DIR"),
                    "/native-world-save/LgsmMoriaSaveControl.dll"
                )),
            )),
            Self::Nightingale => Some((
                "native/LgsmNightingaleSaveControl.dll",
                include_bytes!(concat!(
                    env!("OUT_DIR"),
                    "/native-world-save/LgsmNightingaleSaveControl.dll"
                )),
            )),
            Self::Conan => Some((
                "native/LgsmConanSaveControl.dll",
                include_bytes!(concat!(
                    env!("OUT_DIR"),
                    "/native-world-save/LgsmConanSaveControl.dll"
                )),
            )),
            Self::Scum => Some((
                "native/LgsmScumSaveControl.dll",
                include_bytes!(concat!(
                    env!("OUT_DIR"),
                    "/scum-save-control/LgsmScumSaveControl.dll"
                )),
            )),
            _ => None,
        }
        #[cfg(not(windows))]
        {
            None
        }
    }
    fn process_profile(self) -> process::GameProfile {
        match self {
            Self::Abiotic => process::GameProfile::Abiotic,
            Self::Moria => process::GameProfile::Moria,
            Self::Nightingale => process::GameProfile::Nightingale,
            Self::Conan => process::GameProfile::Conan,
            Self::Scum => process::GameProfile::Scum,
        }
    }
}

fn now() -> Result<u64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .map_err(|_| "The system clock is invalid.".into())
}
fn nonce(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}
fn runtime_root(details: &InstanceDetails) -> Result<PathBuf, String> {
    let instance = Path::new(&details.config_file_path)
        .parent()
        .and_then(Path::parent)
        .ok_or("The instance configuration has no private runtime root.")?;
    let path =
        app_storage::resolve_instance_runtime_root(instance).map_err(|error| error.to_string())?;
    fs::canonicalize(path).map_err(|_| {
        "The instance program directory is unavailable for automatic save preparation.".into()
    })
}
fn root(details: &InstanceDetails) -> Result<PathBuf, String> {
    let instance = Path::new(&details.config_file_path)
        .parent()
        .and_then(Path::parent)
        .ok_or("The instance configuration has no private runtime root.")?;
    if app_storage::instance_program_mode(instance).map_err(|error| error.to_string())?
        != app_storage::InstanceProgramMode::Independent
    {
        return Err("Native saving requires the independent program directory prepared automatically during startup.".into());
    }
    runtime_root(details)
}
struct Context {
    game: Game,
    root: PathBuf,
    pid: u32,
    identity: ProcessIdentity,
    process_key: String,
    launch_token: String,
    native: process::NativeProcess,
}
fn context(details: &InstanceDetails) -> Result<Context, String> {
    let game = Game::from_details(details)?;
    if !matches!(details.summary.status, InstanceStatus::Running)
        || details.summary.active_process_count != 1
    {
        return Err("Native world saving requires one current managed running process.".into());
    }
    let run = details
        .active_run
        .as_ref()
        .ok_or("The managed run is unavailable.")?;
    if run.process_count != 1 || run.processes.len() != 1 {
        return Err("The managed run is ambiguous.".into());
    }
    let process = &run.processes[0];
    let pid = process
        .pid
        .filter(|pid| *pid > 0)
        .ok_or("The managed PID is unavailable.")?;
    let identity = process
        .process_identity
        .clone()
        .filter(|id| id.creation_time > 0)
        .ok_or("The managed process identity is unavailable.")?;
    if !process.is_primary
        || process.status != "running"
        || process.exit_code.is_some()
        || process.crash_flag
        || process.run_id != run.run_id
        || process.session_id != run.session_id
        || run.pid != Some(pid)
    {
        return Err("The current native process context is invalid.".into());
    }
    let root = root(details)?;
    let image = fs::canonicalize(&identity.image_path)
        .map_err(|_| "The current executable is unavailable.")?;
    let shipping = game.win64(&root).join(game.executable());
    let bootstrap = root.join(game.process_profile().bootstrap());
    if image != fs::canonicalize(shipping).map_err(|_| "The fixed native executable is missing.")?
        && !fs::canonicalize(bootstrap).is_ok_and(|path| path == image)
    {
        return Err("The managed image does not belong to this private game runtime.".into());
    }
    let launch_token = session_token(game, &root)?;
    let native = process::capture(game.process_profile(), &root, pid, &identity)?;
    Ok(Context {
        game,
        root,
        pid,
        identity,
        process_key: process.process_key.clone(),
        launch_token,
        native,
    })
}

#[derive(Serialize)]
struct Request<'a> {
    protocol: u32,
    request_id: &'a str,
    requested_at: u64,
    action: &'a str,
    boot_id: &'a str,
    world_token: &'a str,
    #[serde(skip)]
    launch_token: &'a str,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    protocol: u32,
    request_id: String,
    timestamp: u64,
    action: String,
    boot_id: String,
    world_token: String,
    ready: bool,
    invoked: bool,
    call_returned: bool,
    write_confirmed: bool,
    error: Option<String>,
    launch_token: String,
    native_process_id: Option<u32>,
    native_creation_time: Option<String>,
}
fn parse(body: &[u8], request: &Request<'_>) -> Result<Reply, String> {
    parse_with_window(body, request, 6)
}
fn parse_with_window(
    body: &[u8],
    request: &Request<'_>,
    completion_seconds: u64,
) -> Result<Reply, String> {
    let reply: Reply =
        serde_json::from_slice(body).map_err(|_| "The native save receipt is invalid.")?;
    if reply.protocol != 1
        || reply.request_id != request.request_id
        || reply.action != request.action
        || !nonce(&reply.boot_id)
        || reply.timestamp < request.requested_at
        || reply.timestamp > request.requested_at.saturating_add(completion_seconds)
        || (reply.write_confirmed
            && (request.action != "save"
                || !reply.ready
                || !reply.invoked
                || !reply.call_returned
                || reply.error.is_some()))
        || !valid_launch_token(&reply.launch_token)
        || reply.launch_token != request.launch_token
        || (request.action == "save"
            && (reply.boot_id != request.boot_id || reply.world_token != request.world_token))
    {
        return Err("The native save receipt does not match this request.".into());
    }
    Ok(reply)
}

fn validate_native_identity(
    reply: &Reply,
    native: Option<&process::NativeProcess>,
) -> Result<(), String> {
    match (native, reply.native_process_id, reply.native_creation_time.as_deref()) {
        (Some(expected), Some(pid), Some(created))
            if pid == expected.pid && created == expected.identity.creation_time.to_string() => Ok(()),
        (Some(_), None, None) if !reply.ready && !reply.invoked && !reply.call_returned => Ok(()),
        (None, None, None) => Ok(()),
        _ => Err("The native save receipt is missing its current Shipping process identity or belongs to another process generation.".into()),
    }
}

fn parse_context(context: &Context, body: &[u8], request: &Request<'_>) -> Result<Reply, String> {
    let reply = if matches!(
        context.game,
        Game::Abiotic | Game::Conan | Game::Nightingale | Game::Scum
    ) && request.action == "save"
    {
        parse_with_window(body, request, 36)?
    } else {
        parse(body, request)?
    };
    if reply.write_confirmed
        && !matches!(context.game, Game::Moria | Game::Conan | Game::Nightingale)
    {
        return Err("This native adapter has no request-bound completion contract.".into());
    }
    let native = matches!(
        context.game,
        Game::Moria | Game::Nightingale | Game::Conan | Game::Scum
    )
    .then_some(&context.native);
    validate_native_identity(&reply, native)?;
    Ok(reply)
}
fn exchange(context: &Context, request: &Request<'_>) -> Result<Vec<u8>, String> {
    let body = serde_json::to_vec(request).map_err(|_| "Cannot encode the native save request.")?;
    let deadline = Instant::now()
        + Duration::from_secs(
            if matches!(
                context.game,
                Game::Abiotic | Game::Conan | Game::Nightingale | Game::Scum
            ) && request.action == "save"
            {
                35
            } else {
                5
            },
        );
    let directory = context.root.join("langame_world_save");
    let _pin = transport::open_directory(&context.root, &directory)
        .map_err(|_| "The prepared native exchange is unavailable.")?;
    process::verify(
        context.game.process_profile(),
        &context.root,
        context.pid,
        &context.identity,
        &context.native,
    )?;
    transport::publish_body(
        &context.root,
        &directory,
        request.request_id,
        &body,
        deadline,
    )
    .map_err(|_| "Native save publication failed or is uncertain; it was not retried.")?;
    loop {
        process::verify(
            context.game.process_profile(),
            &context.root,
            context.pid,
            &context.identity,
            &context.native,
        )?;
        if Instant::now() >= deadline {
            return Err("Native save receipt timed out; the request was not retried.".into());
        }
        if let Some(body) = transport::response_for_request(
            &context.root,
            &directory.join("response.json"),
            request.request_id,
        )
        .map_err(|_| "The native save receipt could not be read.")?
        {
            return Ok(body);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
fn preflight(context: &Context) -> Result<Reply, String> {
    let id = uuid::Uuid::new_v4().simple().to_string();
    let request = Request {
        protocol: 1,
        request_id: &id,
        requested_at: now()?,
        action: "saveprobe",
        boot_id: "",
        world_token: "",
        launch_token: &context.launch_token,
    };
    let reply = parse_context(context, &exchange(context, &request)?, &request)?;
    if !reply.ready
        || !nonce(&reply.world_token)
        || reply.invoked
        || reply.call_returned
        || reply.error.is_some()
    {
        return Err("The current world, native signature, or save queue is not ready. No manual save was called.".into());
    }
    Ok(reply)
}
fn invoke_once(
    context: &Context,
    ready: &Reply,
) -> Result<(&'static str, WorldSaveCompletion), String> {
    let id = uuid::Uuid::new_v4().simple().to_string();
    let request = Request {
        protocol: 1,
        request_id: &id,
        requested_at: now()?,
        action: "save",
        boot_id: &ready.boot_id,
        world_token: &ready.world_token,
        launch_token: &context.launch_token,
    };
    let reply =
        exchange(context, &request).and_then(|body| parse_context(context, &body, &request));
    classify_completion(context.game, reply)
}

fn classify_completion(
    game: Game,
    reply: Result<Reply, String>,
) -> Result<(&'static str, WorldSaveCompletion), String> {
    // These native paths acknowledge invocation without a storage result.
    // Callers must still parse the correlated receipt before this classification.
    if matches!(game, Game::Abiotic | Game::Scum)
        && reply.as_ref().is_ok_and(|reply| {
            reply.action == "save"
                && reply.ready
                && reply.invoked
                && reply.call_returned
                && !reply.write_confirmed
                && reply.error.is_none()
        })
    {
        return Ok((
            "The native current-world save request was processed; storage completion is not verified.",
            WorldSaveCompletion::Requested,
        ));
    }
    classify(reply).map(|message| (message, WorldSaveCompletion::Confirmed))
}
fn classify(reply: Result<Reply, String>) -> Result<&'static str, String> {
    const UNKNOWN: &str = "Save verification failed: the program could not certify this request's complete native write result. Success was not reported and the save was not resent.";
    match reply {
        Ok(reply)
            if reply.write_confirmed
                && reply.ready
                && reply.invoked
                && reply.call_returned
                && reply.error.is_none() =>
        {
            Ok("The native current-world save completed successfully for this request.")
        }
        Ok(reply)
            if reply.ready
                && reply.invoked
                && reply.call_returned
                && reply.error.as_deref() == Some("save_failed") =>
        {
            Err("The native current-world save returned failure. It was not retried.".into())
        }
        Ok(reply)
            if reply.ready && reply.invoked && reply.call_returned && reply.error.is_none() =>
        {
            Err(UNKNOWN.into())
        }
        Ok(reply)
            if !reply.ready
                && !reply.invoked
                && !reply.call_returned
                && matches!(
                    reply.error.as_deref(),
                    Some(
                        "api_unavailable"
                            | "world_unavailable"
                            | "world_ambiguous"
                            | "save_busy"
                            | "target_changed"
                            | "request_expired"
                            | "dispatcher_timeout"
                            | "dispatcher_unavailable"
                    )
                ) =>
        {
            Err("The native save was rejected before invocation. No manual save was called.".into())
        }
        _ => Err(UNKNOWN.into()),
    }
}

/// Startup refresh changes only an already owned payload while stopped. It
/// never creates the first installation or changes the operator's loader.
pub(crate) fn refresh(details: &InstanceDetails) -> Result<bool, String> {
    let game = Game::from_details(details)?;
    let runtime = runtime_root(details)?;
    if !installation::owned(game, &runtime)? {
        return Ok(false);
    }
    stopped(details)?;
    let root = root(details)?;
    process::stopped(game.process_profile(), &root)?;
    installation::install(game, &root, &game.payload())?;
    Ok(true)
}
const LAUNCH_ENV: &str = "LGSM_UNREAL_WORLD_SAVE_RUN";
fn valid_launch_token(value: &str) -> bool {
    value.strip_prefix("LGSM_UWS_").is_some_and(nonce)
}
fn session_token(game: Game, root: &Path) -> Result<String, String> {
    let _pin = installation::pin(&root.join("langame_world_save"), false)?;
    let bytes = installation::read(&root.join("langame_world_save/session.txt"), 256)?;
    let text =
        std::str::from_utf8(&bytes).map_err(|_| "The native save launch session is invalid.")?;
    let prefix = format!("LGSM_UNREAL_SAVE_1\nmodule={}\nrun=", game.id());
    let token = text
        .strip_prefix(&prefix)
        .and_then(|value| value.strip_suffix('\n'))
        .filter(|value| valid_launch_token(value))
        .ok_or("The native save launch session is invalid.")?;
    Ok(token.to_owned())
}
/// Only the new managed launch inherits this fresh token. An older Shipping
/// process sharing the exchange cannot claim the new boot's native receipt.
pub(crate) fn startup_environment(
    details: &InstanceDetails,
) -> Result<Option<(String, String)>, String> {
    if !refresh(details)? {
        return Ok(None);
    }
    let game = Game::from_details(details)?;
    let root = root(details)?;
    let directory = root.join("langame_world_save");
    let _pin = installation::pin(&directory, false)?;
    let path = directory.join("session.txt");
    let original = if installation::exists(&path)? {
        session_token(game, &root)?;
        Some(installation::read(&path, 256)?)
    } else {
        None
    };
    let token = format!("LGSM_UWS_{}", uuid::Uuid::new_v4().simple());
    let body = format!("LGSM_UNREAL_SAVE_1\nmodule={}\nrun={token}\n", game.id());
    installation::publish(&path, original.as_deref(), body.as_bytes())?;
    Ok(Some((LAUNCH_ENV.to_owned(), token)))
}
fn stopped(details: &InstanceDetails) -> Result<(), String> {
    if !matches!(
        details.summary.status,
        InstanceStatus::Stopped | InstanceStatus::Error
    ) || details.summary.active_process_count != 0
        || details.active_run.as_ref().is_some_and(|run| {
            run.process_count != 0
                || run
                    .processes
                    .iter()
                    .any(|process| process.status == "running")
        })
    {
        return Err(
            "Stop this instance before preparing its optional native save extension.".into(),
        );
    }
    Ok(())
}
fn owned_worker<L: StorageContextTaskLease, T: Send + 'static>(
    lease: &L,
    mutation: OwnedMutexGuard<()>,
    operation: impl FnOnce() -> T + Send + 'static,
) -> tokio::task::JoinHandle<T> {
    spawn_blocking_storage_context_task(lease, move || {
        let _mutation = mutation;
        operation()
    })
}
pub(crate) fn validate_preparation(details: &InstanceDetails) -> Result<PathBuf, String> {
    let game = Game::from_details(details)?;
    stopped(details)?;
    let runtime = root(details)?;
    process::stopped(game.process_profile(), &runtime)?;
    Ok(runtime)
}

/// Caller retains stopped instance and program permits through publication.
pub(crate) fn ensure_prepared(details: &InstanceDetails) -> Result<(), String> {
    let root = validate_preparation(details)?;
    let game = Game::from_details(details)?;
    installation::install(game, &root, &game.payload())
}
pub(crate) async fn save_world(
    state: &DesktopState,
    details: &InstanceDetails,
    submitted_at_unix_ms: u128,
    mutation: Option<OwnedMutexGuard<()>>,
) -> Result<InstanceRuntimeCommandResult, String> {
    let mutation = mutation.ok_or("Native saving requires the instance mutation reservation.")?;
    let lease = state.begin_storage_context_operation("Unreal immediate native world save")?;
    let details = details.clone();
    owned_worker(&lease, mutation, move || {
        let context = context(&details)?;
        let _pins = installation::validate(context.game, &context.root)?;
        if !installation::complete(context.game, &context.root, &context.game.payload())? {
            return Err("The owned native save extension needs preparation while the instance is stopped.".into());
        }
        let ready = preflight(&context)?;
        let (message, completion) = invoke_once(&context, &ready)?;
        let (command, response) = if context.game == Game::Scum {
            ("save_persistent_state", format!("{message} The native AutoSave operation covers global state, pending transform changes and registered persistent entities."))
        } else { ("save", message.into()) };
        Ok(InstanceRuntimeCommandResult { instance_id: details.summary.id.clone(), process_key: context.process_key,
            display_name: format!("{} native persistence", context.game.id()), pid: context.pid, command: command.into(),
            response_text: Some(response), write_confirmation_pending: false,
            save_completion: Some(completion),
            submitted_at_unix_ms })
    }).await.map_err(|_| "Native save worker failed; invocation or completion may be unknown. The request was not retried.".to_owned())?
}

#[cfg(test)]
#[path = "unreal_world_save_tests.rs"]
mod tests;
