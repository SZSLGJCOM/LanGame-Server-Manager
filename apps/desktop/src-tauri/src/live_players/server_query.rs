use std::collections::HashMap;
use std::net::{SocketAddr, UdpSocket};
use std::time::Duration;

use app_core::{
    InstanceDetails, ModulePlayerListSource, ModulePlayerQuerySpec, RuntimeLivePlayerEntry,
    RuntimeLivePlayerIssue, RuntimeLivePlayerIssueCode, RuntimeLivePlayerSnapshot,
    RuntimeLivePlayerStatus,
};

use super::cache::{CachedLivePlayerSnapshot, LivePlayerCollectionResult};
use super::service::{failed_snapshot, misconfigured_snapshot};

const MAX_RESPONSE_BYTES: usize = app_storage::A2S_MAX_RESPONSE_BYTES;
const QUERY_BUDGET: Duration = Duration::from_secs(3);

pub(crate) async fn collect_a2s_players(
    details: &InstanceDetails,
    query: &ModulePlayerQuerySpec,
    request_id: &str,
    observed_at: u64,
) -> LivePlayerCollectionResult {
    let instance_id = &details.summary.id;
    let source = ModulePlayerListSource::ServerQuery;
    if let Some((setting_key, summary)) = app_storage::steam_player_query_visibility_restriction(
        &details.summary.module_id,
        &details.settings_json,
    ) {
        let mut snapshot = super::service::unsupported_snapshot(instance_id, request_id.to_owned());
        snapshot.source = Some(source);
        snapshot.issue = Some(RuntimeLivePlayerIssue {
            code: RuntimeLivePlayerIssueCode::QueryUnavailable,
            setting_keys: vec![String::from(setting_key)],
            summary: String::from(summary),
        });
        return Err(snapshot.into());
    }
    let Some((host, port)) =
        app_storage::resolve_player_query_target(&details.summary.bind_ip, query, &details.ports)
    else {
        return Err(misconfigured_snapshot(
            instance_id,
            request_id.to_owned(),
            source,
            RuntimeLivePlayerIssueCode::QueryUnavailable,
            "The server has no configured UDP query port.",
            Vec::new(),
        )
        .into());
    };
    // The endpoint belongs to the managed local instance. Do not perform unbounded DNS resolution.
    let address = host
        .parse()
        .map(|ip| SocketAddr::new(ip, port))
        .map_err(|_| {
            Box::new(misconfigured_snapshot(
                instance_id,
                request_id.to_owned(),
                source,
                RuntimeLivePlayerIssueCode::QueryUnavailable,
                "The query binding is not an IP address.",
                Vec::new(),
            ))
        })?;
    if port == 0
        || details
            .ports
            .iter()
            .filter(|binding| binding.protocol.eq_ignore_ascii_case("udp") && binding.port == port)
            .count()
            != 1
        || UdpSocket::bind(SocketAddr::new(address.ip(), 0)).is_err()
    {
        return Err(misconfigured_snapshot(
            instance_id,
            request_id.to_owned(),
            source,
            RuntimeLivePlayerIssueCode::QueryUnavailable,
            "The query requires one nonzero port on a local interface.",
            Vec::new(),
        )
        .into());
    }
    let result = tokio::task::spawn_blocking(move || query_players(address)).await;
    let (players, count) = match result {
        Ok(Ok(result)) => result,
        _ => {
            return Err(failed_snapshot(
                instance_id,
                request_id.to_owned(),
                source,
                RuntimeLivePlayerIssueCode::QueryUnavailable,
                "The server did not return a complete player-query response.",
                false,
            )
            .into());
        }
    };
    Ok(project_players(
        instance_id,
        request_id,
        observed_at,
        players,
        count,
    ))
}

#[derive(Debug)]
struct QueryPlayer {
    index: u8,
    name: String,
    seconds: f32,
}

fn query_players(
    address: SocketAddr,
) -> Result<(Vec<QueryPlayer>, Option<app_storage::QueriedPlayerCount>), String> {
    let client = app_storage::A2sClient::connect(address, QUERY_BUDGET)
        .map_err(|error| error.to_string())?;
    let packet = client.players().map_err(|error| error.to_string())?;
    let players = parse_players(&packet)?;
    // Some servers deliberately return an empty/anonymous player response while INFO reports players.
    // An empty PLAYER response alone is therefore not evidence that a server is empty.
    let count = client
        .info()
        .ok()
        .and_then(|packet| app_storage::parse_a2s_info_payload(&packet[5..]));
    Ok((players, count))
}

fn parse_players(packet: &[u8]) -> Result<Vec<QueryPlayer>, String> {
    if packet.len() > MAX_RESPONSE_BYTES || packet.get(..5) != Some(b"\xff\xff\xff\xffD") {
        return Err("Invalid A2S_PLAYER response".into());
    }
    let count = *packet.get(5).ok_or("Missing A2S_PLAYER count")? as usize;
    let mut offset = 6;
    let mut players = Vec::with_capacity(count);
    for row in 0..count {
        let _wire_index = *packet.get(offset).ok_or("Missing player-query index")?;
        // A query index is not a player identity; snapshot-local row keys also support repeated names.
        let index = row as u8;
        offset += 1;
        let tail = packet.get(offset..).ok_or("Missing player-query name")?;
        let end = tail
            .iter()
            .position(|byte| *byte == 0)
            .ok_or("Unterminated player-query name")?;
        let name = std::str::from_utf8(&tail[..end])
            .map_err(|_| "Invalid player-query name encoding")?
            .to_owned();
        if name.chars().count() > 256 || name.chars().any(char::is_control) {
            return Err("Invalid player-query name".into());
        }
        offset += end + 1;
        let stats = packet
            .get(offset..offset + 8)
            .ok_or("Incomplete player-query statistics")?;
        let seconds = f32::from_le_bytes(
            stats[4..8]
                .try_into()
                .map_err(|_| "Invalid player-query duration")?,
        );
        if !seconds.is_finite() || seconds < 0.0 {
            return Err("Invalid player-query duration".into());
        }
        players.push(QueryPlayer {
            index,
            name,
            seconds,
        });
        offset += 8;
    }
    if offset != packet.len() {
        return Err("Unexpected data after player-query response".into());
    }
    Ok(players)
}

fn project_players(
    instance_id: &str,
    request_id: &str,
    observed_at: u64,
    players: Vec<QueryPlayer>,
    count: Option<app_storage::QueriedPlayerCount>,
) -> CachedLivePlayerSnapshot {
    let returned = players.len();
    let entries: Vec<_> = players
        .into_iter()
        .filter(|player| !player.name.trim().is_empty())
        .map(|player| RuntimeLivePlayerEntry {
            player_key: format!("{request_id}:{}", player.index),
            display_name: player.name,
            identifiers: Vec::new(),
            available_action_ids: Vec::new(),
            ping_ms: None,
            session_started_at_unix_ms: Some(
                observed_at.saturating_sub((f64::from(player.seconds) * 1000.0) as u64),
            ),
            role: None,
            attributes: Vec::new(),
        })
        .collect();
    let complete = entries.len() == returned
        && count.is_none_or(|info| info.current_players == returned)
        && (returned > 0 || count.is_some_and(|info| info.current_players == 0));
    let issue = (!complete).then(|| RuntimeLivePlayerIssue {
        code: RuntimeLivePlayerIssueCode::NamesUnavailable,
        setting_keys: Vec::new(),
        summary: "The server query did not identify every connected player.".into(),
    });
    CachedLivePlayerSnapshot {
        public_snapshot: RuntimeLivePlayerSnapshot {
            snapshot_id: request_id.into(),
            instance_id: instance_id.into(),
            status: RuntimeLivePlayerStatus::Ready,
            source: Some(ModulePlayerListSource::ServerQuery),
            observed_at_unix_ms: Some(observed_at),
            expires_at_unix_ms: None,
            complete,
            truncated: false,
            stale: false,
            current_players: count
                .map(|info| info.current_players)
                .or((returned > 0).then_some(returned)),
            max_players: count.map(|info| info.max_players),
            entries,
            issue,
        },
        private_action_bindings: HashMap::new(),
        collected_at: observed_at,
    }
}

#[cfg(test)]
#[path = "server_query_tests.rs"]
mod tests;
