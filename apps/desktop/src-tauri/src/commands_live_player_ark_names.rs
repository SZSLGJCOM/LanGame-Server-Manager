use std::collections::HashMap;

use app_core::{
    InstanceDetails, RuntimeLivePlayerEntry, RuntimeLivePlayerIssue, RuntimeLivePlayerIssueCode,
    RuntimePlayerIdentityKind,
};

use crate::commands::{
    commands_ark_map::{self, protocol},
    commands_ark_tools as ark,
    commands_program_storage::instance_root,
};

#[path = "commands_live_player_ark_profiles.rs"]
mod profiles;

fn needs_unicode_name(entry: &RuntimeLivePlayerEntry) -> bool {
    entry.display_name.contains(['?', '\u{fffd}'])
}

/// Stock ARK RCON can replace Unicode with question marks before transmission.
/// Keep ListPlayers authoritative for presence and moderation; enrich only its
/// display names using the same account's Unicode name in its native profile.
/// An already installed extension can fill names not saved by the game yet.
pub(super) async fn supplement(
    details: &InstanceDetails,
    process_key: Option<&str>,
    request_id: &str,
    entries: &mut [RuntimeLivePlayerEntry],
) -> Option<RuntimeLivePlayerIssue> {
    if !entries.iter().any(needs_unicode_name) {
        return None;
    }
    let accounts = entries
        .iter()
        .filter(|entry| needs_unicode_name(entry))
        .flat_map(account_ids)
        .map(str::to_owned)
        .collect();
    let (mut names, profile_issue) = match profiles::collect(details, process_key, accounts).await {
        Ok(result) => (result.names, result.issue),
        Err(error) => (HashMap::new(), Some(error)),
    };
    let extension_issue = if entries
        .iter()
        .filter(|entry| needs_unicode_name(entry))
        .all(|entry| account_name(entry, &names).is_some())
    {
        None
    } else {
        match read_names(details, process_key, request_id)
            .await
            .and_then(|snapshot| extension_names(&snapshot.players))
        {
            Ok(extension) => {
                names.extend(extension);
                None
            }
            Err(error) => Some(error),
        }
    };
    if apply_names(entries, &names) {
        return None;
    }
    let reason = [profile_issue, extension_issue]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ");
    Some(RuntimeLivePlayerIssue {
        code: RuntimeLivePlayerIssueCode::NamesUnavailable,
        setting_keys: Vec::new(),
        summary: format!(
            "A complete account name is not yet available from the current map's native player profile or its installed extension. {reason}"
        ),
        extension_status: None,
    })
}

async fn read_names(
    details: &InstanceDetails,
    process_key: Option<&str>,
    request_id: &str,
) -> Result<protocol::Snapshot, String> {
    // The caller owns the instance mutation guard and verifies the process before
    // and after collection. Calling the public read_ark_map would reacquire it.
    let process_key = process_key.unwrap_or("main");
    let run = details.active_run.as_ref().ok_or("ARK is not running.")?;
    let run_id = run
        .processes
        .iter()
        .find(|process| process.process_key == process_key)
        .map(|process| process.run_id)
        .unwrap_or(run.run_id);
    let target = commands_ark_map::selected_target(details, process_key, run_id)?;
    let instance_path = instance_root(details)?.to_owned();
    let installed = tokio::task::spawn_blocking(move || {
        let root = app_storage::resolve_instance_runtime_root(&instance_path)
            .map_err(|error| error.to_string())?;
        root.join("ShooterGame/Binaries/Win64/ArkApi/Plugins/LgsmArkTools/LgsmArkTools.dll")
            .try_exists()
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("ARK name extension inspection failed: {error}"))??;
    if !installed {
        return Err("ARK name extension is not installed.".into());
    }
    let map = app_core::ark_maps::processes(details)?
        .into_iter()
        .find(|map| map.process_key == process_key)
        .ok_or("ARK map was removed.")?;
    ark::ensure_endpoint(&target).await?;
    let response = ark::dispatch(&target, &format!("LgsmArkTools.Players {request_id}")).await?;
    protocol::snapshot(
        &response,
        &target.summary.module_id,
        request_id,
        &map.map_name,
    )
}

fn extension_names(players: &[protocol::Player]) -> Result<HashMap<String, String>, String> {
    let mut names = HashMap::new();
    let mut identities = std::collections::HashSet::new();
    for player in players {
        if player.account_id.is_empty() {
            continue;
        }
        if !identities.insert(player.account_id.as_str()) {
            return Err("ARK player names contain a duplicate account identity.".into());
        }
        if let Some(name) = player
            .account_name
            .as_deref()
            .filter(|name| !name.trim().is_empty() && !name.contains('\u{fffd}'))
        {
            names.insert(player.account_id.clone(), name.to_owned());
        }
    }
    Ok(names)
}

fn account_ids(entry: &RuntimeLivePlayerEntry) -> impl Iterator<Item = &str> {
    entry
        .identifiers
        .iter()
        .filter(|id| {
            id.stable
                && matches!(
                    id.kind,
                    RuntimePlayerIdentityKind::SteamId
                        | RuntimePlayerIdentityKind::EosId
                        | RuntimePlayerIdentityKind::ArkAccountId
                )
        })
        .map(|id| id.value.as_str())
}

fn account_name<'a>(
    entry: &RuntimeLivePlayerEntry,
    names: &'a HashMap<String, String>,
) -> Option<&'a String> {
    account_ids(entry).find_map(|id| names.get(id))
}

fn apply_names(entries: &mut [RuntimeLivePlayerEntry], names: &HashMap<String, String>) -> bool {
    let mut complete = true;
    for entry in entries.iter_mut().filter(|entry| needs_unicode_name(entry)) {
        if let Some(name) = account_name(entry, names) {
            entry.display_name = name.to_owned();
        } else {
            complete = false;
        }
    }
    complete
}

#[cfg(test)]
#[path = "commands_live_player_ark_names_tests.rs"]
mod tests;
