use std::collections::{HashMap, HashSet};

use app_core::{
    RuntimeLivePlayerAttribute, RuntimeLivePlayerEntry, RuntimeLivePlayerIdentifier,
    RuntimeLivePlayerStatus, RuntimePlayerIdentityKind,
};

pub(super) const MAX_PLAYER_ENTRIES: usize = 256;
pub(super) const MAX_CAPTURE_BYTES: usize = 64 * 1024;
pub(super) const MAX_LINE_BYTES: usize = 4 * 1024;
pub(super) const MAX_FIELD_CHARS: usize = 256;

const BEGIN_MARKER: &str = "[LGM-DST-PLAYERS-BEGIN]";
const ROW_MARKER: &str = "[LGM-DST-PLAYER]";
const END_MARKER: &str = "[LGM-DST-PLAYERS-END]";

#[derive(Debug)]
pub(super) enum DstCaptureOutcome {
    Complete(CollectedLivePlayers),
    Incomplete(PartialLivePlayers),
    NoCapture,
}

#[derive(Debug)]
pub(super) struct CollectedLivePlayers {
    pub status: RuntimeLivePlayerStatus,
    pub complete: bool,
    pub truncated: bool,
    pub current_players: Option<usize>,
    #[cfg(test)]
    pub raw_valid_count: usize,
    #[cfg(test)]
    pub unique_count: usize,
    #[cfg(test)]
    pub stored_count: usize,
    pub players: Vec<CollectedLivePlayer>,
}

#[derive(Debug)]
pub(super) struct PartialLivePlayers {
    #[cfg(test)]
    pub status: RuntimeLivePlayerStatus,
    #[cfg(test)]
    pub complete: bool,
    pub truncated: bool,
    #[cfg(test)]
    pub current_players: Option<usize>,
    #[cfg(test)]
    pub raw_valid_count: usize,
    #[cfg(test)]
    pub unique_count: usize,
    #[cfg(test)]
    pub stored_count: usize,
    pub players: Vec<CollectedLivePlayer>,
}

#[derive(Debug)]
pub(super) struct CollectedLivePlayer {
    pub entry: RuntimeLivePlayerEntry,
    pub action_bindings: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarkerKind {
    Begin,
    Row,
    End,
}

struct CaptureAccumulator {
    request_id: String,
    capture_bytes: usize,
    malformed: bool,
    truncated: bool,
    raw_valid_count: usize,
    unique_count: usize,
    seen_ids: HashSet<String>,
    players: Vec<CollectedLivePlayer>,
}

pub(super) fn parse_dst_client_table<T: AsRef<str>>(
    lines: &[String],
    request_id: &str,
    validated_action_ids: &[T],
) -> DstCaptureOutcome {
    if !valid_request_id(request_id) {
        return DstCaptureOutcome::NoCapture;
    }

    let mut capture: Option<CaptureAccumulator> = None;
    for line in lines {
        if let Some(active) = capture.as_mut() {
            let line_bytes = line.len().saturating_add(1);
            if line.len() > MAX_LINE_BYTES
                || active.capture_bytes.saturating_add(line_bytes) > MAX_CAPTURE_BYTES
            {
                active.truncated = true;
                return DstCaptureOutcome::Incomplete(active.take_partial());
            }
            active.capture_bytes += line_bytes;
        } else if line.len() > MAX_LINE_BYTES {
            continue;
        }

        let Some((kind, fields)) = protocol_fields(line) else {
            continue;
        };
        match kind {
            MarkerKind::Begin if fields.len() == 2 && fields[1] == request_id => {
                if let Some(active) = capture.as_mut() {
                    active.malformed = true;
                } else {
                    capture = Some(CaptureAccumulator::new(request_id, line.len() + 1));
                }
            }
            MarkerKind::Row => {
                let Some(active) = capture.as_mut() else {
                    continue;
                };
                if fields.get(1).copied() != Some(request_id) {
                    continue;
                }
                if fields.len() != 7 || !active.push_row(&fields) {
                    active.malformed = true;
                }
            }
            MarkerKind::End => {
                let Some(mut active) = capture.take() else {
                    continue;
                };
                if fields.get(1).copied() != Some(request_id) {
                    capture = Some(active);
                    continue;
                }
                let declared_count = if fields.len() == 3 {
                    fields[2].parse::<usize>().ok()
                } else {
                    None
                };
                if !active.malformed && declared_count == Some(active.raw_valid_count) {
                    return DstCaptureOutcome::Complete(
                        active.finish_complete(validated_action_ids),
                    );
                }
                active.malformed = true;
                return DstCaptureOutcome::Incomplete(active.take_partial());
            }
            _ => {}
        }
    }

    capture
        .map(|mut active| DstCaptureOutcome::Incomplete(active.take_partial()))
        .unwrap_or(DstCaptureOutcome::NoCapture)
}

impl CaptureAccumulator {
    fn new(request_id: &str, capture_bytes: usize) -> Self {
        Self {
            request_id: request_id.to_owned(),
            capture_bytes,
            malformed: false,
            truncated: false,
            raw_valid_count: 0,
            unique_count: 0,
            seen_ids: HashSet::new(),
            players: Vec::new(),
        }
    }

    fn push_row(&mut self, fields: &[&str]) -> bool {
        let Some(sequence) = fields[2].parse::<usize>().ok() else {
            return false;
        };
        if sequence != self.raw_valid_count + 1 || !valid_stable_id(fields[3]) {
            return false;
        }
        let is_admin = match fields[6] {
            "0" => false,
            "1" => true,
            _ => return false,
        };

        self.raw_valid_count += 1;
        let stable_id = fields[3];
        if !self.seen_ids.insert(stable_id.to_owned()) {
            return true;
        }
        self.unique_count += 1;
        if self.players.len() == MAX_PLAYER_ENTRIES {
            self.truncated = true;
            return true;
        }

        let entry = RuntimeLivePlayerEntry {
            player_key: format!("dst:{}:{}", self.request_id, self.unique_count),
            display_name: sanitize_display_field(fields[4]),
            identifiers: vec![RuntimeLivePlayerIdentifier {
                kind: RuntimePlayerIdentityKind::KleiUserId,
                value: stable_id.to_owned(),
                stable: true,
            }],
            available_action_ids: Vec::new(),
            ping_ms: None,
            session_started_at_unix_ms: None,
            role: is_admin.then(|| String::from("admin")),
            attributes: vec![RuntimeLivePlayerAttribute {
                key: String::from("character"),
                value: sanitize_display_field(fields[5]),
            }],
        };
        self.players.push(CollectedLivePlayer {
            entry,
            action_bindings: HashMap::new(),
        });
        true
    }

    fn finish_complete<T: AsRef<str>>(
        mut self,
        validated_action_ids: &[T],
    ) -> CollectedLivePlayers {
        if !self.truncated {
            for player in &mut self.players {
                let stable_id = player.entry.identifiers[0].value.clone();
                for action_id in validated_action_ids {
                    let action_id = action_id.as_ref().to_owned();
                    player.entry.available_action_ids.push(action_id.clone());
                    player.action_bindings.insert(action_id, stable_id.clone());
                }
            }
        }
        CollectedLivePlayers {
            status: RuntimeLivePlayerStatus::Ready,
            complete: true,
            truncated: self.truncated,
            current_players: Some(self.raw_valid_count),
            #[cfg(test)]
            raw_valid_count: self.raw_valid_count,
            #[cfg(test)]
            unique_count: self.unique_count,
            #[cfg(test)]
            stored_count: self.players.len(),
            players: self.players,
        }
    }

    fn take_partial(&mut self) -> PartialLivePlayers {
        PartialLivePlayers {
            #[cfg(test)]
            status: RuntimeLivePlayerStatus::Failed,
            #[cfg(test)]
            complete: false,
            truncated: self.truncated,
            #[cfg(test)]
            current_players: None,
            #[cfg(test)]
            raw_valid_count: self.raw_valid_count,
            #[cfg(test)]
            unique_count: self.unique_count,
            #[cfg(test)]
            stored_count: self.players.len(),
            players: std::mem::take(&mut self.players),
        }
    }
}

fn protocol_fields(line: &str) -> Option<(MarkerKind, Vec<&str>)> {
    let (offset, kind) = [
        (BEGIN_MARKER, MarkerKind::Begin),
        (ROW_MARKER, MarkerKind::Row),
        (END_MARKER, MarkerKind::End),
    ]
    .into_iter()
    .flat_map(|(marker, kind)| {
        line.match_indices(marker)
            .filter(move |(offset, _)| marker_has_token_boundary(line, *offset, marker))
            .map(move |(offset, _)| (offset, kind))
    })
    .min_by_key(|(offset, _)| *offset)?;
    let mut fields: Vec<_> = line[offset..].split('\t').collect();
    let expected_fields = match kind {
        MarkerKind::Begin => 2,
        MarkerKind::Row => 7,
        MarkerKind::End => 3,
    };
    // DST's native print logger appends one tab after the final argument.
    // Remove only that framing byte; missing or additional columns stay invalid.
    if fields.len() == expected_fields + 1 && fields.last() == Some(&"") {
        fields.pop();
    }
    Some((kind, fields))
}

fn marker_has_token_boundary(line: &str, offset: usize, marker: &str) -> bool {
    line[offset + marker.len()..]
        .chars()
        .next()
        .is_none_or(|next| next == '\t')
}

fn valid_request_id(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_stable_id(value: &str) -> bool {
    !value.trim().is_empty()
        && value.chars().count() <= MAX_FIELD_CHARS
        && !value.chars().any(char::is_control)
}

fn sanitize_display_field(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_control())
        .take(MAX_FIELD_CHARS)
        .collect()
}
