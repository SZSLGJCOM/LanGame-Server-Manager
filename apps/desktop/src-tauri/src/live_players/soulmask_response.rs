use std::collections::{HashMap, HashSet};

use app_core::{
    ModulePlayerListSource, RuntimeLivePlayerEntry, RuntimeLivePlayerIdentifier,
    RuntimeLivePlayerIssueCode, RuntimeLivePlayerSnapshot, RuntimeLivePlayerStatus,
    RuntimePlayerIdentityKind,
};

use super::{FetchError, MAX_PAGES, MAX_RESPONSE_BYTES, failure};
use crate::live_players::cache::{CachedLivePlayerSnapshot, LivePlayerCollectionResult};

const MAX_PLAYERS: usize = 1024;
const PAGE_START: &str = "=========QUERY INTERACTIVE MODE========\r\n";
const PAGE_END: &str = "|  Enter any number to goto that page.|\r\n\
|  Enter n to show next page.         |\r\n\
|  Enter q to exit interactive mode.  |\r\n\
=======================================\r\n";

#[derive(Debug, PartialEq, Eq)]
pub(super) struct NativePage<'a> {
    pub payload: &'a [u8],
    pub pagination: Option<(usize, usize)>,
}

pub(super) fn split_page(body: &[u8]) -> Result<NativePage<'_>, FetchError> {
    let text = std::str::from_utf8(body).map_err(|_| FetchError::Incomplete)?;
    if !text.ends_with("\r\n") {
        return Err(FetchError::Incomplete);
    }
    let Some(offset) = text.find(PAGE_START) else {
        return Ok(NativePage {
            payload: body,
            pagination: None,
        });
    };
    if offset != 0 && !text[..offset].ends_with("\r\n") {
        return Err(FetchError::Incomplete);
    }
    let label = text[offset + PAGE_START.len()..]
        .strip_suffix(PAGE_END)
        .and_then(|label| label.strip_suffix("\r\n"))
        .and_then(|label| label.strip_prefix("|  PAGE:"))
        .and_then(|label| label.strip_suffix('|'))
        .ok_or(FetchError::Incomplete)?;
    let (page, total) = label
        .trim()
        .split_once(" of ")
        .ok_or(FetchError::Incomplete)?;
    let number = |value: &str| {
        if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(FetchError::Incomplete);
        }
        value.parse::<usize>().map_err(|_| FetchError::Incomplete)
    };
    let page = number(page)?;
    let total = number(total)?;
    if page == 0 || page > total {
        return Err(FetchError::Incomplete);
    }
    if total > MAX_PAGES {
        return Err(FetchError::TooLarge);
    }
    Ok(NativePage {
        payload: &body[..offset],
        pagination: Some((page, total)),
    })
}

pub(super) fn parse(
    instance_id: &str,
    body: &[u8],
    request_id: &str,
    observed_at: u64,
) -> LivePlayerCollectionResult {
    let invalid = || {
        failure(
            instance_id,
            request_id,
            RuntimeLivePlayerIssueCode::ProtocolIncomplete,
            "The Soulmask response is not a complete online-player identity table.",
            false,
        )
    };
    let limit = || {
        failure(
            instance_id,
            request_id,
            RuntimeLivePlayerIssueCode::CaptureLimit,
            "The Soulmask player response exceeds the collection limit.",
            true,
        )
    };
    if body.len() > MAX_RESPONSE_BYTES {
        return Err(limit());
    }
    let text = std::str::from_utf8(body).map_err(|_| invalid())?;
    // Each table row is line-terminated on the wire. An EOF in the middle of a
    // row cannot produce a complete empty/partial list, even after a nonce.
    if !text.ends_with("\r\n") {
        return Err(invalid());
    }
    let mut lines = text.split("\r\n");
    let header = lines.next().ok_or_else(invalid)?;
    if header.split('|').map(str::trim).collect::<Vec<_>>()
        != ["", "Account", "PlayerName", "PawnID", "Position", ""]
    {
        return Err(invalid());
    }
    let mut entries = Vec::new();
    let mut seen = HashSet::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let row = line
            .strip_prefix('|')
            .and_then(|line| line.strip_suffix('|'))
            .ok_or_else(invalid)?;
        let (account, remaining) = row.split_once('|').ok_or_else(invalid)?;
        let (remaining, position) = remaining.rsplit_once('|').ok_or_else(invalid)?;
        let (name, pawn) = remaining.rsplit_once('|').ok_or_else(invalid)?;
        let account = account.trim();
        let name = name
            .trim()
            .strip_prefix('\'')
            .and_then(|name| name.strip_suffix('\''))
            .ok_or_else(invalid)?;
        if account.len() != 17
            || !account.bytes().all(|byte| byte.is_ascii_digit())
            || name.trim().is_empty()
            || name.chars().count() > 256
            || name.chars().any(char::is_control)
            || pawn.trim().is_empty()
            || pawn.chars().any(char::is_control)
            || position.trim().is_empty()
            || position.chars().any(char::is_control)
            || !seen.insert(account.to_owned())
        {
            return Err(invalid());
        }
        if entries.len() == MAX_PLAYERS {
            return Err(limit());
        }
        // Echo exposes the real Steam account and current character name. Row
        // management stays disabled: the existing actions have other targets.
        entries.push(RuntimeLivePlayerEntry {
            player_key: format!("soulmask:{account}"),
            display_name: name.to_owned(),
            identifiers: vec![RuntimeLivePlayerIdentifier {
                kind: RuntimePlayerIdentityKind::SteamId,
                value: account.to_owned(),
                stable: true,
            }],
            available_action_ids: Vec::new(),
            ping_ms: None,
            session_started_at_unix_ms: None,
            role: None,
            attributes: Vec::new(),
        });
    }
    Ok(CachedLivePlayerSnapshot {
        public_snapshot: RuntimeLivePlayerSnapshot {
            snapshot_id: request_id.to_owned(),
            instance_id: instance_id.to_owned(),
            status: RuntimeLivePlayerStatus::Ready,
            source: Some(ModulePlayerListSource::TcpConsole),
            observed_at_unix_ms: Some(observed_at),
            expires_at_unix_ms: None,
            complete: true,
            truncated: false,
            stale: false,
            current_players: Some(entries.len()),
            max_players: None,
            entries,
            issue: None,
        },
        private_action_bindings: HashMap::new(),
        collected_at: observed_at,
    })
}
