//! Native session identity and moderation for verified, fingerprinted Enshrouded builds.
use std::path::PathBuf;

use app_core::{InstanceDetails, ProcessIdentity, RuntimeLivePlayerIssueCode};

use super::cache::LivePlayerCollectionResult;
use super::service::failed_snapshot;

#[path = "enshrouded_native_io.rs"]
pub(crate) mod io;
#[path = "enshrouded_native_response.rs"]
mod response;
#[path = "enshrouded_native_transport.rs"]
mod transport;

#[derive(Clone)]
pub(super) struct Context {
    pub pid: u32,
    pub identity: ProcessIdentity,
    pub root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Target {
    pub run_id: String,
    pub sequence: u64,
    pub tick: u64,
    pub handle: u32,
    pub generation: u64,
    pub account: u64,
}
impl Target {
    fn encode(&self) -> String {
        format!(
            "{}:{}:{}:{}:{}:{}",
            self.run_id, self.sequence, self.tick, self.handle, self.generation, self.account
        )
    }
    fn parse(text: &str) -> Result<Self, String> {
        let parts: Vec<_> = text.split(':').collect();
        if parts.len() != 6 || !response::nonce(parts[0]) {
            return Err("A native action requires the selected player binding.".into());
        }
        let handle = response::decimal(parts[3])?;
        let target = Self {
            run_id: parts[0].to_owned(),
            sequence: response::decimal(parts[1])?,
            tick: response::decimal(parts[2])?,
            handle: u32::try_from(handle).map_err(|_| "Invalid native session handle.")?,
            generation: response::decimal(parts[4])?,
            account: response::decimal(parts[5])?,
        };
        if target.sequence == 0
            || target.handle == 0
            || target.handle & 0x3f >= 16
            || target.generation == 0
            || target.account == 0
        {
            return Err("The selected native player binding is incomplete.".into());
        }
        Ok(target)
    }
}

fn context(details: &InstanceDetails) -> Result<Context, String> {
    if details.summary.module_id != "enshrouded" {
        return Err("Native Enshrouded control requires its own module.".into());
    }
    let mut processes = details
        .active_run
        .as_ref()
        .ok_or("The Enshrouded server is stopped.")?
        .processes
        .iter()
        .filter(|process| process.is_primary && process.status == "running");
    let process = processes
        .next()
        .ok_or("The native server process is unavailable.")?;
    if processes.next().is_some() {
        return Err("The native server process is ambiguous.".into());
    }
    let pid = process.pid.ok_or("The native server PID is missing.")?;
    let identity = process
        .process_identity
        .clone()
        .ok_or("The native process identity is missing.")?;
    let image = PathBuf::from(&identity.image_path);
    if !image
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("enshrouded_server.exe"))
    {
        return Err("The native control executable does not match Enshrouded.".into());
    }
    let root = image
        .parent()
        .ok_or("The native server directory is missing.")?
        .to_owned();
    Ok(Context {
        pid,
        identity,
        root,
    })
}

pub(crate) async fn collect(
    details: &InstanceDetails,
    query: &app_core::ModulePlayerQuerySpec,
    request_id: &str,
    observed_at: u64,
) -> LivePlayerCollectionResult {
    let instance_id = details.summary.id.clone();
    let failure = |message: String| {
        Box::new(failed_snapshot(
            &instance_id,
            request_id.to_owned(),
            app_core::ModulePlayerListSource::FileIpc,
            RuntimeLivePlayerIssueCode::ExtensionUnavailable,
            &message,
            false,
        ))
    };
    let context = context(details).map_err(failure)?;
    let extension_status =
        io::extension_status(&context.root, context.pid, context.identity.creation_time);
    let native_failure = |message: String| {
        let mut snapshot = failure(message);
        if let Some(issue) = &mut snapshot.issue {
            issue.extension_status = extension_status;
            if extension_status.is_none() {
                issue.code = RuntimeLivePlayerIssueCode::ProtocolIncomplete;
            }
        }
        snapshot
    };
    // Establish native availability before consuming the query. Name-only
    // fallback owns one query when the extension is absent or incompatible.
    let packet = tokio::task::spawn_blocking(move || transport::read_packet(&context))
        .await
        .map_err(|_| native_failure("The native player collector worker failed.".into()))?
        .map_err(native_failure)?;
    let mut projected =
        response::project(packet, &instance_id, request_id, observed_at).map_err(native_failure)?;
    let native_count = projected.public_snapshot.entries.len();
    let evidence =
        super::server_query::collect_a2s_player_evidence(details, query, request_id, observed_at)
            .await?;
    if native_count != evidence.returned_players
        || evidence
            .info
            .is_some_and(|info| info.current_players != native_count)
    {
        return Err(native_failure("The native player identities and live query count are in transition. Refresh players before moderation.".into()));
    }
    projected.public_snapshot.max_players = evidence.info.map(|info| info.max_players);
    Ok(projected)
}

pub(crate) struct Receipt {
    pub account_id: u64,
    pub banned: Option<bool>,
}

pub(crate) async fn execute(details: &InstanceDetails, command: &str) -> Result<Receipt, String> {
    let context = context(details)?;
    let parts: Vec<_> = command.split_ascii_whitespace().collect();
    if parts.len() != 2 {
        return Err("Invalid native moderation command.".into());
    }
    let action = match parts[0] {
        "kick" => "kick",
        "ban" => "ban",
        "unban" => "unban",
        _ => return Err("Unsupported native moderation action.".into()),
    }
    .to_owned();
    let (target, account) = if action == "unban" {
        let account = response::decimal(parts[1])?;
        if account == 0 {
            return Err("A native accountId is required to unban an account.".into());
        }
        (None, account)
    } else {
        let target = Target::parse(parts[1])?;
        let account = target.account;
        (Some(target), account)
    };
    let banned = match action.as_str() {
        "ban" => Some(true),
        "unban" => Some(false),
        _ => None,
    };
    tokio::task::spawn_blocking(move || transport::execute(&context, &action, target, account))
        .await
        .map_err(|_| "The native moderation worker failed; its outcome is unknown.")??;
    Ok(Receipt {
        account_id: account,
        banned,
    })
}

#[cfg(test)]
#[path = "enshrouded_native_tests.rs"]
mod tests;
