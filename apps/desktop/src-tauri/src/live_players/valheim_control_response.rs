//! The adapter must first authenticate the current owned process and control endpoint.
//! Parsing a packet or recognizing its platform spelling does not authenticate its sender.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

pub(super) const MAX_RESPONSE_BYTES: usize = 64 * 1024;
const MAX_PLAYERS: usize = 256;
const MAX_ACCESS_ENTRIES: usize = 4096;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Operation {
    Query,
    Reload,
    Kick,
    Save,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub(super) enum ResponseError {
    #[error("The Valheim control response exceeds 64 KiB.")]
    TooLarge,
    #[error("Invalid Valheim control response: {0}.")]
    Invalid(&'static str),
    #[error("Valheim rejected the control operation: {0}")]
    Rejected(String),
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct AccessLists {
    pub admin: Vec<String>,
    pub banned: Vec<String>,
    pub permitted: Vec<String>,
}

/// All fields come from one validated native peer. Account namespaces and
/// advertised actions are projected only by the authenticated owning adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ValidatedPeer {
    pub session_key: String,
    pub name: String,
    pub host_id: String,
    pub platform_id: String,
    pub admin: bool,
}

#[derive(Debug)]
pub(crate) struct ControlSnapshot {
    pub players: Vec<ValidatedPeer>,
    pub access: AccessLists,
    pub capabilities: HashSet<Operation>,
    pub world_save: Option<WorldSaveReceipt>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct WorldSaveReceipt {
    pub previous_save_number: u32,
    pub committed_save_number: u32,
    evidence: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Packet {
    protocol: u32,
    nonce: String,
    pid: u32,
    op: Operation,
    status: Status,
    error: Option<String>,
    capabilities: Vec<Operation>,
    players: Vec<Player>,
    access: AccessLists,
    world_save: Option<WorldSaveReceipt>,
}

#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Status {
    Ok,
    Error,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Player {
    session_key: String,
    name: String,
    host_id: String,
    platform_id: String,
    admin: bool,
}

pub(super) fn parse(
    body: &[u8],
    expected_nonce: &str,
    expected_pid: u32,
    expected_op: Operation,
) -> Result<ControlSnapshot, ResponseError> {
    if body.len() > MAX_RESPONSE_BYTES {
        return Err(ResponseError::TooLarge);
    }
    if expected_pid == 0 || !guid_n(expected_nonce) {
        return Err(ResponseError::Invalid("invalid request expectation"));
    }
    // Typed deserialization rejects invalid UTF-8, missing/duplicate/unknown
    // object fields, wrong types and trailing JSON without echoing the payload.
    let packet: Packet =
        serde_json::from_slice(body).map_err(|_| ResponseError::Invalid("schema or UTF-8"))?;
    if packet.protocol != 1
        || packet.pid != expected_pid
        || packet.op != expected_op
        || packet.nonce != expected_nonce
    {
        return Err(ResponseError::Invalid("request or process mismatch"));
    }
    let mut capabilities = HashSet::new();
    if packet.capabilities.is_empty()
        || packet
            .capabilities
            .iter()
            .any(|cap| !capabilities.insert(*cap))
        || !capabilities.contains(&expected_op)
    {
        return Err(ResponseError::Invalid("missing or duplicate capability"));
    }
    validate_access(&packet.access)?;
    if packet.players.len() > MAX_PLAYERS {
        return Err(ResponseError::Invalid("player limit"));
    }
    if let Some(error) = &packet.error
        && !error.is_empty()
        && !bounded_text(error, 512)
    {
        return Err(ResponseError::Invalid("error text"));
    }
    let players = validate_players(packet.players)?;
    if packet.status == Status::Error {
        if packet.world_save.is_some() {
            return Err(ResponseError::Invalid("failure includes a save receipt"));
        }
        let error = packet
            .error
            .filter(|error| !error.is_empty())
            .ok_or(ResponseError::Invalid("missing failure detail"))?;
        return Err(ResponseError::Rejected(error));
    }
    if packet.error.is_some_and(|error| !error.is_empty()) {
        return Err(ResponseError::Invalid("success includes failure detail"));
    }
    match (expected_op, &packet.world_save) {
        (Operation::Save, Some(receipt))
            if receipt.evidence == "native_commit"
                && receipt.committed_save_number
                    == receipt.previous_save_number.wrapping_add(1) => {}
        (Operation::Save, _) => return Err(ResponseError::Invalid("missing native save commit")),
        (_, Some(_)) => return Err(ResponseError::Invalid("unexpected save receipt")),
        (_, None) => {}
    }
    Ok(ControlSnapshot {
        players,
        access: packet.access,
        capabilities,
        world_save: packet.world_save,
    })
}

fn validate_players(players: Vec<Player>) -> Result<Vec<ValidatedPeer>, ResponseError> {
    let mut sessions = HashSet::new();
    let mut hosts = HashSet::new();
    let mut validated = Vec::with_capacity(players.len());
    for player in players {
        if !guid_n(&player.session_key)
            || !bounded_text(&player.name, 256)
            || !bounded_token(&player.host_id, 256)
            || !platform_token(&player.platform_id)
            || !sessions.insert(player.session_key.clone())
            || !hosts.insert(player.host_id.clone())
        {
            return Err(ResponseError::Invalid(
                "invalid or duplicate player identity",
            ));
        }
        if let Some(steam_id) = player.platform_id.strip_prefix("Steam_") {
            // Steam sockets use the bare number. PlayFab sockets, including
            // crossplay Steam users, use the complete native PlatformUserID.
            if !canonical_nonzero_u64(steam_id)
                || (player.host_id != steam_id && player.host_id != player.platform_id)
            {
                return Err(ResponseError::Invalid(
                    "conflicting Steam platform identity",
                ));
            }
        } else if player.host_id != player.platform_id {
            return Err(ResponseError::Invalid(
                "conflicting native platform identity",
            ));
        }
        validated.push(ValidatedPeer {
            session_key: player.session_key,
            name: player.name,
            host_id: player.host_id,
            platform_id: player.platform_id,
            admin: player.admin,
        });
    }
    Ok(validated)
}

fn validate_access(access: &AccessLists) -> Result<(), ResponseError> {
    for list in [&access.admin, &access.banned, &access.permitted] {
        if list.len() > MAX_ACCESS_ENTRIES {
            return Err(ResponseError::Invalid("access entry limit"));
        }
        // This is a bounded read of native facts, including malformed historic
        // entries. Validation for future writes belongs to the storage codec.
        if list
            .iter()
            .any(|value| value.chars().count() > 1024 || value.chars().any(char::is_control))
        {
            return Err(ResponseError::Invalid("access entry text"));
        }
    }
    Ok(())
}

fn guid_n(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        && value.bytes().any(|byte| byte != b'0')
}

fn bounded_text(value: &str, max_chars: usize) -> bool {
    !value.trim().is_empty()
        && value.chars().count() <= max_chars
        && !value.chars().any(char::is_control)
}

fn bounded_token(value: &str, max_chars: usize) -> bool {
    bounded_text(value, max_chars)
        && !value.chars().any(char::is_whitespace)
        && !value.contains([',', '|'])
}

fn platform_token(value: &str) -> bool {
    let Some((platform, id)) = value.split_once('_') else {
        return false;
    };
    bounded_token(value, 256)
        && !platform.is_empty()
        && platform.bytes().all(|byte| byte.is_ascii_alphanumeric())
        && !id.is_empty()
}

fn canonical_nonzero_u64(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 20
        && !value.starts_with('0')
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value.parse::<u64>().is_ok_and(|id| id != 0)
}

#[cfg(test)]
#[path = "valheim_control_response_tests.rs"]
mod tests;
