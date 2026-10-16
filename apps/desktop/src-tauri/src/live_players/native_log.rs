use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use app_core::{
    InstanceDetails, ModulePlayerQuerySpec, RuntimeLivePlayerEntry, RuntimeLivePlayerIdentifier,
    RuntimeLivePlayerIssue, RuntimeLivePlayerIssueCode, RuntimePlayerIdentityKind,
};

use super::cache::LivePlayerCollectionResult;
use super::server_query::{A2sPlayerEvidence, collect_a2s_player_evidence};
use crate::runtime_log_stream::{RuntimeLogTailState, read_runtime_log_delta_bounded};

#[path = "enshrouded_roster.rs"]
mod roster;
use roster::NameRoster;
#[path = "valheim_roster.rs"]
mod valheim_roster;
use valheim_roster::ValheimPeers;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Game {
    Enshrouded,
    Valheim,
}

const MAX_TRACKED_INSTANCES: usize = 128;
const READ_BUDGET: usize = 4 * 1024 * 1024;
const READ_CHUNK: usize = 64 * 1024;
const MAX_LINE: usize = 8 * 1024;

#[derive(Clone, Default)]
pub(crate) struct NativePlayerLogRegistry {
    logs: Arc<Mutex<HashMap<String, TrackedLog>>>,
}

#[derive(Clone, PartialEq, Eq)]
struct RunKey {
    game: Game,
    run_id: i64,
    pid: u32,
    identity: Option<(u64, String)>,
    path: PathBuf,
}

impl RunKey {
    fn from_details(details: &InstanceDetails) -> Option<Self> {
        let run = details.active_run.as_ref()?;
        let process = run.processes.iter().find(|process| process.is_primary)?;
        Some(Self {
            game: match details.summary.module_id.as_str() {
                "enshrouded" => Game::Enshrouded,
                "valheim" => Game::Valheim,
                _ => return None,
            },
            run_id: run.run_id,
            pid: process.pid?,
            identity: process
                .process_identity
                .as_ref()
                .map(|identity| (identity.creation_time, identity.image_path.clone())),
            path: PathBuf::from(process.log_path.as_ref()?),
        })
    }
}

struct TrackedLog {
    key: RunKey,
    tail: RuntimeLogTailState,
    roster: NameRoster,
    valheim_peers: ValheimPeers,
    caught_up: bool,
    needs_replay: bool,
    fault: Option<RuntimeLivePlayerIssueCode>,
    touched: Instant,
}

impl TrackedLog {
    fn new(key: RunKey) -> Self {
        Self {
            key,
            tail: RuntimeLogTailState::default(),
            roster: NameRoster::default(),
            valheim_peers: ValheimPeers::default(),
            caught_up: false,
            needs_replay: false,
            fault: None,
            touched: Instant::now(),
        }
    }

    fn advance(&mut self) {
        if self.needs_replay {
            self.tail = RuntimeLogTailState::default();
            self.needs_replay = false;
        }
        self.caught_up = false;
        self.fault = None;
        // Only the primary process's owned logical run is eligible. The
        // managed reader validates segment ownership and reports retention gaps.
        let end = app_storage::managed_console_log::open_log_segments(&self.key.path)
            .ok()
            .flatten()
            .and_then(|segments| {
                segments.last().and_then(|segment| {
                    segment
                        .file
                        .metadata()
                        .ok()?
                        .len()
                        .checked_add(segment.start_offset)
                })
            });
        if end.is_none_or(|end| end < self.tail.byte_offset) {
            self.read_failed();
            return;
        }
        let mut remaining = READ_BUDGET;
        while remaining > 0 {
            let before = self.tail.byte_offset;
            let delta = match read_runtime_log_delta_bounded(
                &self.key.path,
                &mut self.tail,
                remaining.min(READ_CHUNK),
                MAX_LINE,
            ) {
                Ok(delta) => delta,
                Err(_) => {
                    self.read_failed();
                    return;
                }
            };
            if delta.byte_offset != before.saturating_add(delta.bytes_read as u64)
                || delta.stream_error.is_some()
            {
                self.invalidate();
                self.fault = Some(RuntimeLivePlayerIssueCode::LogUnavailable);
            }
            // The reader enters discard mode at the end of the first chunk
            // that exceeds its line limit. Observe every chunk so a later
            // delimiter cannot hide that missing event from this roster.
            if self.tail.is_discarding_overlong_line()
                || delta.lines.iter().any(|line| line.len() > MAX_LINE)
            {
                self.invalidate();
                self.fault = Some(RuntimeLivePlayerIssueCode::CaptureLimit);
            }
            for line in delta.lines {
                match self.key.game {
                    Game::Enshrouded => self.roster.apply(&line),
                    Game::Valheim => self.valheim_peers.apply(&mut self.roster, &line),
                }
            }
            remaining = remaining.saturating_sub(delta.bytes_read);
            if !delta.limit_exhausted {
                self.caught_up = self.tail.pending_text.is_empty();
                return;
            }
            if delta.bytes_read == 0 {
                break;
            }
        }
        self.fault = Some(RuntimeLivePlayerIssueCode::CaptureLimit);
    }

    fn invalidate(&mut self) {
        self.roster.invalidate();
        self.valheim_peers = ValheimPeers::default();
    }

    fn read_failed(&mut self) {
        self.invalidate();
        self.fault = Some(RuntimeLivePlayerIssueCode::LogUnavailable);
        self.needs_replay = true;
    }
}

impl NativePlayerLogRegistry {
    // The existing collection entry holds the instance mutation lease across
    // these reads and verifies the process identity before and after collection.
    fn take(&self, instance: &str, key: RunKey) -> Result<TrackedLog, ()> {
        let mut logs = self.logs.lock().map_err(|_| ())?;
        Ok(logs
            .remove(instance)
            .filter(|log| log.key == key)
            .unwrap_or_else(|| TrackedLog::new(key)))
    }

    fn remember(&self, instance: String, mut log: TrackedLog) -> Result<(), ()> {
        let mut logs = self.logs.lock().map_err(|_| ())?;
        if logs.len() >= MAX_TRACKED_INSTANCES
            && let Some(oldest) = logs
                .iter()
                .min_by_key(|(_, log)| log.touched)
                .map(|(id, _)| id.clone())
        {
            logs.remove(&oldest);
        }
        log.touched = Instant::now();
        logs.insert(instance, log);
        Ok(())
    }
}

pub(crate) async fn collect(
    registry: &NativePlayerLogRegistry,
    details: &InstanceDetails,
    query: &ModulePlayerQuerySpec,
    request_id: &str,
    observed_at: u64,
) -> LivePlayerCollectionResult {
    let tracked =
        RunKey::from_details(details).and_then(|key| registry.take(&details.summary.id, key).ok());
    let before = match tracked {
        Some(mut log) => tokio::task::spawn_blocking(move || {
            log.advance();
            log
        })
        .await
        .ok(),
        None => None,
    };
    let queried = collect_a2s_player_evidence(details, query, request_id, observed_at).await;
    let Some(mut log) = before else {
        return queried.map(|mut evidence| {
            if !evidence.snapshot.public_snapshot.complete {
                set_issue(&mut evidence, RuntimeLivePlayerIssueCode::LogUnavailable);
            }
            evidence.snapshot
        });
    };
    let before_revision = log.roster.revision;
    let before_caught_up = log.caught_up;
    let registry = registry.clone();
    let instance = details.summary.id.clone();
    let snapshot_id = request_id.to_owned();
    tokio::task::spawn_blocking(move || {
        log.advance();
        let result = queried.map(|mut evidence| {
            supplement(&mut evidence, &mut log, before_revision, before_caught_up);
            evidence.snapshot
        });
        if registry.remember(instance.clone(), log).is_err() {
            return Err(Box::new(super::service::failed_snapshot(
                &instance,
                snapshot_id,
                app_core::ModulePlayerListSource::ServerQuery,
                RuntimeLivePlayerIssueCode::IoFailed,
                "The current-run player tracker is unavailable.",
                false,
            )));
        }
        result
    })
    .await
    .map_err(|_| {
        Box::new(super::service::failed_snapshot(
            &details.summary.id,
            request_id.to_owned(),
            app_core::ModulePlayerListSource::ServerQuery,
            RuntimeLivePlayerIssueCode::IoFailed,
            "The current-run player log could not be read.",
            false,
        ))
    })?
}

fn supplement(
    evidence: &mut A2sPlayerEvidence,
    log: &mut TrackedLog,
    before_revision: u64,
    before_caught_up: bool,
) {
    let consistent_count = evidence
        .info
        .filter(|count| {
            count.current_players == evidence.returned_players
                // Valheim does not publish user-data rows to Steam's PLAYER query.
                // Its names require the complete current-run character lifecycle below.
                || (log.key.game == Game::Valheim && evidence.returned_players == 0)
        })
        .map(|count| count.current_players);
    let stable_read = before_caught_up
        && log.caught_up
        && !log.needs_replay
        && before_revision == log.roster.revision;
    // Do not clear a join consumed after a zero-player INFO response. A
    // continuous, unchanged cursor lets an actual empty server reset a gap.
    if stable_read && consistent_count == Some(0) {
        log.roster.establish_empty();
        log.valheim_peers = ValheimPeers::default();
    }
    let mut remaining = log.roster.names.clone();
    let known_names_match = evidence
        .snapshot
        .public_snapshot
        .entries
        .iter()
        .all(|entry| {
            if let Some(count) = remaining.get_mut(&entry.display_name)
                && *count > 0
            {
                *count -= 1;
                true
            } else {
                false
            }
        });
    let complete_log = stable_read
        && !log.roster.incomplete
        && log.valheim_peers.is_stable()
        && known_names_match
        && consistent_count == Some(log.roster.count());
    if evidence.snapshot.public_snapshot.complete
        && (log.key.game != Game::Valheim || !complete_log)
    {
        if consistent_count == Some(0) && before_revision != log.roster.revision {
            set_issue(evidence, RuntimeLivePlayerIssueCode::NamesUnavailable);
        }
        // Complete query names remain useful when native character identity is
        // unproven. Do not guess an owner or expose a name-ban identifier.
        return;
    }
    if complete_log {
        let valheim = log.key.game == Game::Valheim;
        let make_entry = |player_key: String, name: &str| RuntimeLivePlayerEntry {
            player_key,
            display_name: name.to_owned(),
            identifiers: if valheim {
                vec![RuntimeLivePlayerIdentifier {
                    kind: RuntimePlayerIdentityKind::PlayerName,
                    value: name.to_owned(),
                    stable: false,
                }]
            } else {
                Vec::new()
            },
            available_action_ids: Vec::new(),
            ping_ms: None,
            session_started_at_unix_ms: None,
            role: None,
            attributes: Vec::new(),
        };
        let mut entries = Vec::with_capacity(log.roster.count());
        if valheim {
            for (key, name) in log.valheim_peers.entries() {
                entries.push(make_entry(key.to_owned(), name));
            }
            entries.sort_by(|left, right| {
                left.display_name
                    .cmp(&right.display_name)
                    .then_with(|| left.player_key.cmp(&right.player_key))
            });
        } else {
            for (name, count) in &log.roster.names {
                for _ in 0..*count {
                    entries.push(make_entry(
                        format!(
                            "{}:log:{}",
                            evidence.snapshot.public_snapshot.snapshot_id,
                            entries.len()
                        ),
                        name,
                    ));
                }
            }
        }
        evidence.snapshot.public_snapshot.entries = entries;
        evidence.snapshot.public_snapshot.complete = true;
        evidence.snapshot.public_snapshot.issue = None;
    } else {
        set_issue(
            evidence,
            log.fault
                .unwrap_or(RuntimeLivePlayerIssueCode::NamesUnavailable),
        );
    }
}

fn set_issue(evidence: &mut A2sPlayerEvidence, code: RuntimeLivePlayerIssueCode) {
    evidence.snapshot.public_snapshot.complete = false;
    evidence.snapshot.public_snapshot.issue = Some(RuntimeLivePlayerIssue {
        code,
        setting_keys: Vec::new(),
        summary:
            "The current server run's player log and query do not establish a complete name list."
                .into(),
    });
}

#[cfg(test)]
#[path = "native_log_tests.rs"]
mod tests;
