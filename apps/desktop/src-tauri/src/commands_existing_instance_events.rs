use super::{ExistingClient, evidence};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Default, Serialize)]
pub(super) struct EventEvidence {
    pub read_failed: bool,
    pub resets_after_baseline: usize,
    pub batches: usize,
    pub runs: BTreeMap<i64, RunEvents>,
    pub pending_start_logs: BTreeMap<String, RunEvents>,
    pub streams: BTreeMap<String, StreamEvidence>,
    pub retained_transcripts_match: bool,
}

impl EventEvidence {
    pub(super) fn sound_for(&self, run_ids: &[i64]) -> bool {
        !self.read_failed && self.resets_after_baseline == 0 && !run_ids.is_empty()
            && self.retained_transcripts_match
            // A stream reserved before run publication legitimately retains
            // run_id=None. The start receipt's unique log paths plus complete
            // content equality provide the association, not a retroactive tag.
            && self.runs.values().chain(self.pending_start_logs.values()).all(|run| {
                !run.capture_incomplete && !run.retention_or_stream_limit
            })
    }
}

#[derive(Default, Serialize)]
pub(super) struct StreamEvidence {
    byte_offset: u64,
    transcript: super::transcript::Signature,
}

#[derive(Default, Serialize)]
pub(super) struct RunEvents {
    events: u64,
    lines: u64,
    game_lines: u64,
    last_byte_offset: u64,
    capture_incomplete: bool,
    retention_or_stream_limit: bool,
}

impl RunEvents {
    fn observe_stream_error(&mut self, value: Option<&Value>) -> Result<(), &'static str> {
        let Some(value) = value.filter(|value| !value.is_null()) else {
            return Ok(());
        };
        let message = value.as_str().ok_or("invalid log stream error")?;
        // The explicit error channel is not file content. Matching transcripts
        // must never turn an incomplete capture into a successful receipt.
        self.capture_incomplete = true;
        self.retention_or_stream_limit |= evidence::diagnostic_markers(message).1;
        Ok(())
    }
}

#[derive(Deserialize)]
struct Batch {
    generation: String,
    reset: bool,
    events: Vec<Event>,
}
#[derive(Deserialize)]
struct Event {
    sequence: u64,
    name: String,
    payload: Value,
}

pub(super) struct Observer {
    instance_id: String,
    generation: Option<String>,
    sequence: u64,
    evidence: EventEvidence,
    transcripts: BTreeMap<String, super::transcript::Accumulator>,
}

impl Observer {
    pub(super) fn new(id: &str) -> Self {
        Self {
            instance_id: id.into(),
            generation: None,
            sequence: 0,
            evidence: EventEvidence::default(),
            transcripts: BTreeMap::new(),
        }
    }

    pub(super) async fn establish(&mut self, client: &ExistingClient) -> Result<(), String> {
        // The bridge is bounded by both count and encoded bytes; a short batch
        // can still have successors. At most 256 nonempty pulls plus an empty
        // pull drains a quiescent ring even when only one event fits per batch.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
        for _ in 0..257 {
            let count = tokio::time::timeout_at(deadline, self.poll(client))
                .await
                .map_err(|_| "event baseline exceeded its total deadline")??;
            if count == 0 {
                self.evidence = EventEvidence::default();
                self.transcripts.clear();
                return Ok(());
            }
        }
        Err("event stream did not reach an initial baseline".into())
    }

    async fn poll(&mut self, client: &ExistingClient) -> Result<usize, String> {
        let response = client
            .request(
                "runtime_service_events",
                json!({
                    "after": self.sequence, "generation": self.generation,
                }),
            )
            .await?;
        let batch: Batch =
            serde_json::from_value(response).map_err(|_| "invalid event response")?;
        if uuid::Uuid::parse_str(&batch.generation).is_err() || batch.events.len() > 32 {
            return Err("invalid event generation or batch size".into());
        }
        let changed = self.generation.as_deref() != Some(batch.generation.as_str());
        if self.generation.is_some() && (batch.reset || changed) {
            self.evidence.resets_after_baseline += 1;
        }
        if batch.reset || changed {
            self.sequence = 0;
        }
        let count = batch.events.len();
        for event in batch.events {
            if event.sequence <= self.sequence {
                return Err("invalid event sequence".into());
            }
            self.sequence = event.sequence;
            if event.name != "runtime-log-stream"
                || event.payload["instance_id"].as_str() != Some(self.instance_id.as_str())
            {
                continue;
            }
            let path = event.payload["log_path"]
                .as_str()
                .ok_or("log event has no path")?;
            let key = path_key(Path::new(path));
            if self.transcripts.len() >= 160 && !self.transcripts.contains_key(&key) {
                return Err("event transcript exceeded its path bound".into());
            }
            let transcript = self.transcripts.entry(key.clone()).or_default();
            let run = if let Some(run_id) = event.payload["run_id"].as_i64() {
                if self.evidence.runs.len() >= 128 && !self.evidence.runs.contains_key(&run_id) {
                    return Err("event evidence exceeded its run bound".into());
                }
                self.evidence.runs.entry(run_id).or_default()
            } else {
                // Windrose initialization and other pending-start output can
                // precede publication of the first persisted run ID.
                let path = event.payload["log_path"]
                    .as_str()
                    .ok_or("pending log has no path")?;
                if self.evidence.pending_start_logs.len() >= 32
                    && !self.evidence.pending_start_logs.contains_key(path)
                {
                    return Err("pending log evidence exceeded its bound".into());
                }
                self.evidence
                    .pending_start_logs
                    .entry(path.into())
                    .or_default()
            };
            run.events += 1;
            run.last_byte_offset = event.payload["byte_offset"].as_u64().unwrap_or_default();
            run.observe_stream_error(event.payload.get("stream_error"))?;
            for line in event.payload["lines"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                run.lines += 1;
                run.game_lines += u64::from(evidence::game_line(line));
                transcript.line(line);
                let (incomplete, retention) = evidence::diagnostic_markers(line);
                run.capture_incomplete |= incomplete;
                run.retention_or_stream_limit |= retention;
            }
            self.evidence.streams.insert(
                key,
                StreamEvidence {
                    byte_offset: run.last_byte_offset,
                    transcript: transcript.snapshot(),
                },
            );
        }
        self.generation = Some(batch.generation);
        self.evidence.batches += 1;
        Ok(count)
    }

    pub(super) async fn watch<T>(
        &mut self,
        client: &ExistingClient,
        future: impl Future<Output = Result<T, String>>,
    ) -> Result<T, String> {
        tokio::pin!(future);
        let mut interval = tokio::time::interval(Duration::from_millis(100));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                result = &mut future => return result,
                _ = interval.tick() => {
                    if self.poll(client).await.is_err() { self.evidence.read_failed = true; }
                }
            }
        }
    }

    pub(super) fn pending_paths(&self) -> Vec<PathBuf> {
        self.evidence
            .pending_start_logs
            .keys()
            .map(PathBuf::from)
            .collect()
    }

    pub(super) async fn finish(
        mut self,
        client: &ExistingClient,
        logs: &[evidence::LogEvidence],
    ) -> EventEvidence {
        // The producer has stopped before these immutable file signatures were
        // captured. Matching every line proves final partial delivery and also
        // detects lost middle batches, which an EOF byte offset alone cannot.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            match tokio::time::timeout_at(deadline, self.poll(client)).await {
                Ok(Ok(_)) => {}
                _ => {
                    self.evidence.read_failed = true;
                    break;
                }
            }
            if matches_retained(&self.evidence, logs) {
                self.evidence.retained_transcripts_match = true;
                break;
            }
            if tokio::time::Instant::now() >= deadline {
                break;
            }
            tokio::time::sleep_until(
                (tokio::time::Instant::now() + Duration::from_millis(100)).min(deadline),
            )
            .await;
        }
        self.evidence
    }
}

fn path_key(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase()
}

fn matches_retained(events: &EventEvidence, logs: &[evidence::LogEvidence]) -> bool {
    let mut count = 0;
    for log in logs.iter().filter(|log| log.source == "managed_run") {
        count += 1;
        if !log.sound()
            || !events
                .streams
                .get(&path_key(&log.path))
                .is_some_and(|stream| {
                    stream.byte_offset == log.end_offset && stream.transcript == log.transcript
                })
        {
            return false;
        }
    }
    count > 0
}

#[test]
fn existing_acceptance_event_eof_requires_complete_matching_content() {
    let path = PathBuf::from(r"C:\fixture\run.log");
    let mut file = super::transcript::Accumulator::default();
    file.bytes(b"ready\nfinal partial");
    let signature = file.finish();
    let logs = [evidence::LogEvidence {
        path: path.clone(),
        source: "managed_run".into(),
        end_offset: 19,
        transcript: signature.clone(),
        ..Default::default()
    }];
    let mut events = EventEvidence::default();
    let mut partial = super::transcript::Accumulator::default();
    partial.line("ready");
    events.streams.insert(
        path_key(&path),
        StreamEvidence {
            byte_offset: 19,
            transcript: partial.snapshot(),
        },
    );
    assert!(
        !matches_retained(&events, &logs),
        "matching EOF offset must not hide pending text"
    );
    partial.line("final partial");
    events.streams.insert(
        path_key(&path),
        StreamEvidence {
            byte_offset: 19,
            transcript: partial.snapshot(),
        },
    );
    assert!(matches_retained(&events, &logs));
}

#[test]
fn existing_acceptance_event_stream_error_rejects_matching_transcripts() {
    for message in [
        "",
        "Reader failed: synthetic fixture",
        "[LanGame] Earlier console output expired under the log retention policy.",
    ] {
        let mut run = RunEvents::default();
        run.observe_stream_error(Some(&json!(message))).unwrap();
        let mut events = EventEvidence {
            retained_transcripts_match: true,
            ..Default::default()
        };
        events.runs.insert(1, run);
        assert!(
            !events.sound_for(&[1]),
            "explicit errors must fail even when every retained file line matches"
        );
    }
}

#[test]
fn existing_acceptance_event_stream_error_accepts_absence_and_rejects_invalid_shape() {
    let mut run = RunEvents::default();
    run.observe_stream_error(None).unwrap();
    run.observe_stream_error(Some(&Value::Null)).unwrap();
    assert!(!run.capture_incomplete);
    assert!(
        run.observe_stream_error(Some(&json!({"message": "invalid shape"})))
            .is_err()
    );
}
