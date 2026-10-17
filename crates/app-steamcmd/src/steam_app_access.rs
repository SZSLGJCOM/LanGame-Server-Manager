//! Current invocation evidence for one trusted app_update command. Diagnostic
//! content-log tails may contain older attempts and cannot establish access.
use super::steamcmd_stream::StreamingCommandResult;
use super::*;

#[derive(Debug, Default)]
pub(super) struct SteamAppCommandEvidence {
    failures: Vec<(u32, bool)>,
}

impl SteamAppCommandEvidence {
    pub(super) fn observe_line(&mut self, line: &str) {
        let Some((app_id, permission_denied)) = app_failure(line) else {
            return;
        };
        if let Some((_, denied)) = self.failures.iter_mut().find(|(id, _)| *id == app_id) {
            *denied |= permission_denied;
        } else if self.failures.len() < 32 {
            // A managed script updates one application. Keep evidence bounded
            // even if a faulty native process reports unrelated applications.
            self.failures.push((app_id, permission_denied));
        }
    }

    fn failure_for(&self, app_id: u32) -> Option<bool> {
        self.failures
            .iter()
            .find(|(id, _)| *id == app_id)
            .map(|(_, denied)| *denied)
    }
}

pub(super) fn verify_command_result(
    app_id: u32,
    account_name: Option<&str>,
    output: &StreamingCommandResult,
) -> Result<(), SteamCmdError> {
    if let Some(account_name) = account_name {
        super::steamcmd_account::verify_authenticated_account(
            account_name,
            output.authenticated_account_name.as_deref(),
            &output.excerpt,
        )?;
    }
    // Also inspect the tail for callers constructing a result directly. The
    // streaming evidence remains authoritative after that tail has rolled over.
    let current_failure = output
        .app_command_evidence
        .failure_for(app_id)
        .into_iter()
        .chain(output.excerpt.lines().filter_map(|line| {
            app_failure(line)
                .filter(|(id, _)| *id == app_id)
                .map(|(_, denied)| denied)
        }))
        .reduce(|previous, denied| previous || denied);
    if current_failure == Some(true) {
        return Err(SteamCmdError::SteamAppAccountRequired {
            app_id,
            output_excerpt: failure_context(app_id, output),
        });
    }
    if output.success && current_failure.is_none() {
        return Ok(());
    }
    Err(SteamCmdError::SteamCmdCommandFailed {
        output_excerpt: failure_context(app_id, output),
    })
}

pub(super) fn failure_context(app_id: u32, output: &StreamingCommandResult) -> String {
    let mut context = steamcmd_failure_context(
        &output.excerpt,
        output.exit_code,
        output.content_log_excerpt.as_deref(),
    );
    if output.app_command_evidence.failure_for(app_id).is_some() {
        context.push_str(&format!(
            "\nSteamCMD reported an installation failure for AppID {app_id} during this invocation."
        ));
    }
    context
}

fn app_failure(line: &str) -> Option<(u32, bool)> {
    let message = line
        .strip_prefix('[')
        .and_then(|rest| rest.split_once(']'))
        .filter(|(stamp, _)| {
            stamp.len() == 19 && stamp.as_bytes()[4] == b'-' && stamp.as_bytes()[10] == b' '
        })
        .map_or(line, |(_, message)| message)
        .trim();
    let lower = message.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("error! failed to install app '") {
        let (app, result) = rest.split_once("' (")?;
        let (result, tail) = result.split_once(')')?;
        if !matches!(tail.trim(), "" | "." | "quit" | ".quit") {
            return None;
        }
        return Some((app_id(app)?, permission_result(result)));
    }
    if let Some(rest) = lower.strip_prefix("error! app '") {
        let (app, state) = rest.split_once("' state is 0x")?;
        let state = state.strip_suffix(" after update job.")?;
        if !state.is_empty() && state.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Some((app_id(app)?, false));
        }
    }
    // Native update cancellation rows identify the app. No subscription and
    // no license are entitlement results; AccessDenied here may be local I/O.
    if let Some(rest) = lower.strip_prefix("appid ") {
        let (app, reason) = rest.split_once(" update canceled : ")?;
        let result = reason
            .rsplit_once(" (")
            .and_then(|(_, result)| result.strip_suffix(')'))
            .unwrap_or(reason);
        return Some((app_id(app)?, license_result(result)));
    }
    None
}

fn app_id(value: &str) -> Option<u32> {
    value
        .parse::<u32>()
        .ok()
        .filter(|id| *id > 0 && value.bytes().all(|byte| byte.is_ascii_digit()))
}

fn license_result(result: &str) -> bool {
    matches!(
        result,
        "no subscription" | "nosubscription" | "no license" | "nolicense"
    )
}

fn permission_result(result: &str) -> bool {
    license_result(result) || matches!(result, "access denied" | "accessdenied")
}

#[cfg(test)]
#[path = "steam_app_access_tests.rs"]
mod tests;
