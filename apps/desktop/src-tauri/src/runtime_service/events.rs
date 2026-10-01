use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::sync::Mutex;
use tauri::{Listener, Manager};

const CAPACITY: usize = 256;
const BATCH_SIZE: usize = 32;
// A 256 KiB read plus a 64 KiB pending row can expand sixfold in JSON.
// Bound the complete encoded events, retained queue and response separately.
const MAX_EVENT_BYTES: usize = 2 * 1024 * 1024;
const MAX_RETAINED_BYTES: usize = 32 * 1024 * 1024;
const MAX_BATCH_BYTES: usize = 4 * 1024 * 1024;
pub(super) const RESET_EVENT: &str = "runtime-service-events-reset";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct Event {
    pub sequence: u64,
    pub name: String,
    pub payload: Value,
}

#[derive(Debug, Deserialize, Serialize)]
struct Batch {
    generation: String,
    reset: bool,
    events: Vec<Event>,
}

pub(super) struct Events {
    state: Mutex<EventState>,
}

struct QueuedEvent {
    event: Event,
    bytes: usize,
}

struct EventState {
    generation: String,
    sequence: u64,
    retained_bytes: usize,
    events: VecDeque<QueuedEvent>,
}

impl EventState {
    fn invalidate(&mut self) {
        // A missing final event has no later sequence to expose its gap. Rotate
        // the generation even for an empty queue so every existing cursor asks
        // for file readback once; retaining earlier events would replay them.
        self.generation = uuid::Uuid::new_v4().to_string();
        self.sequence = 0;
        self.retained_bytes = 0;
        self.events.clear();
    }
}

impl Default for Events {
    fn default() -> Self {
        Self {
            state: Mutex::new(EventState {
                generation: uuid::Uuid::new_v4().to_string(),
                sequence: 0,
                retained_bytes: 0,
                events: VecDeque::new(),
            }),
        }
    }
}

impl Events {
    fn push(&self, name: &str, payload: &str) {
        let payload = (payload.len() <= MAX_EVENT_BYTES)
            .then(|| serde_json::from_str(payload).ok())
            .flatten();
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let Some(payload) = payload else {
            state.invalidate();
            return;
        };
        if state.sequence == u64::MAX {
            state.invalidate();
        }
        let event = Event {
            sequence: state.sequence + 1,
            name: name.into(),
            payload,
        };
        let bytes = json_size(&event).unwrap_or(usize::MAX);
        if bytes > MAX_EVENT_BYTES {
            state.invalidate();
            return;
        }
        state.sequence = event.sequence;
        state.retained_bytes += bytes;
        state.events.push_back(QueuedEvent { event, bytes });
        while state.events.len() > CAPACITY || state.retained_bytes > MAX_RETAINED_BYTES {
            if let Some(expired) = state.events.pop_front() {
                state.retained_bytes -= expired.bytes;
            }
        }
    }

    pub(super) fn after(&self, sequence: u64, generation: Option<&str>) -> Result<Value, String> {
        let state = self
            .state
            .lock()
            .map_err(|_| "Runtime service events unavailable")?;
        let reset = generation != Some(state.generation.as_str())
            || sequence > state.sequence
            || state
                .events
                .front()
                .is_some_and(|item| item.event.sequence > sequence.saturating_add(1));
        let after = if reset { 0 } else { sequence };
        // A gap or a service restart invalidates the cursor. File-based console
        // readback is authoritative; the retained tail can still be delivered.
        let mut batch = Batch {
            generation: state.generation.clone(),
            reset,
            events: Vec::new(),
        };
        let mut bytes = json_size(&batch).map_err(|error| error.to_string())?;
        for item in state
            .events
            .iter()
            .filter(|item| item.event.sequence > after)
        {
            let next_bytes = bytes + item.bytes + usize::from(!batch.events.is_empty());
            if batch.events.len() == BATCH_SIZE || next_bytes > MAX_BATCH_BYTES {
                break;
            }
            batch.events.push(item.event.clone());
            bytes = next_bytes;
        }
        drop(state);
        serde_json::to_value(batch).map_err(|error| error.to_string())
    }
}

fn json_size(value: &impl Serialize) -> Result<usize, serde_json::Error> {
    struct Counter(usize);
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_add(bytes.len())
                .ok_or_else(|| std::io::Error::other("Runtime event size overflow"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter(0);
    serde_json::to_writer(&mut counter, value)?;
    Ok(counter.0)
}

#[derive(Default)]
pub(super) struct Cursor {
    generation: Option<String>,
    sequence: u64,
}

impl Cursor {
    pub(super) fn request_args(&self) -> Value {
        json!({ "after": self.sequence, "generation": self.generation })
    }

    pub(super) fn accept(&mut self, response: Value) -> Result<(bool, Vec<Event>), String> {
        let batch: Batch = serde_json::from_value(response).map_err(|error| error.to_string())?;
        if uuid::Uuid::parse_str(&batch.generation).is_err() || batch.events.len() > BATCH_SIZE {
            return Err("Runtime event batch has an invalid generation or size".into());
        }
        let mut previous = 0;
        for event in &batch.events {
            if event.sequence <= previous {
                return Err(
                    "Runtime event sequences must be positive and strictly increasing".into(),
                );
            }
            previous = event.sequence;
        }
        let changed = self.generation.as_deref() != Some(batch.generation.as_str());
        let reset = batch.reset || changed;
        let readback = self.generation.is_some() && reset;
        let after = if reset { 0 } else { self.sequence };
        let events: Vec<_> = batch
            .events
            .into_iter()
            .filter(|event| event.sequence > after)
            .collect();
        self.sequence = events.last().map_or(after, |event| event.sequence);
        self.generation = Some(batch.generation);
        Ok((readback, events))
    }
}

pub(super) fn install(app: &tauri::AppHandle) {
    app.manage(Events::default());
    for name in [
        crate::runtime_log_stream::RUNTIME_LOG_STREAM_EVENT,
        "app-shutdown-failed",
    ] {
        let handle = app.clone();
        app.listen_any(name, move |event| {
            handle.state::<Events>().push(name, event.payload());
        });
    }
}

#[cfg(test)]
#[path = "events_budget_tests.rs"]
mod budget_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn pull(events: &Events, cursor: &mut Cursor) -> (bool, Vec<Event>) {
        let request = cursor.request_args();
        cursor
            .accept(
                events
                    .after(
                        request["after"].as_u64().unwrap(),
                        request["generation"].as_str(),
                    )
                    .unwrap(),
            )
            .unwrap()
    }

    #[test]
    fn service_restart_delivers_new_low_sequences_and_requests_readback() {
        let old = Events::default();
        let mut cursor = Cursor::default();
        for _ in 0..20 {
            old.push("runtime-log-stream", "null");
        }
        assert!(!pull(&old, &mut cursor).0);
        let restarted = Events::default();
        restarted.push("runtime-log-stream", "\"new output\"");
        let (readback, events) = pull(&restarted, &mut cursor);
        assert!(readback);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].sequence, 1);
        assert_eq!(events[0].payload, "new output");
        assert!(!pull(&restarted, &mut cursor).0);
    }

    #[test]
    fn empty_restart_resets_cursor_before_any_new_events_exist() {
        let old = Events::default();
        let mut cursor = Cursor::default();
        old.push("runtime-log-stream", "null");
        pull(&old, &mut cursor);
        let restarted = Events::default();
        let (readback, events) = pull(&restarted, &mut cursor);
        assert!(readback);
        assert!(events.is_empty());
        assert_eq!(cursor.sequence, 0);
        restarted.push("runtime-log-stream", "null");
        assert_eq!(pull(&restarted, &mut cursor).1[0].sequence, 1);
    }

    #[test]
    fn retention_gap_resets_once_then_drains_bounded_batches_without_replay() {
        let source = Events::default();
        let mut cursor = Cursor::default();
        pull(&source, &mut cursor);
        for _ in 0..CAPACITY + 4 {
            source.push("runtime-log-stream", "null");
        }
        let (readback, first) = pull(&source, &mut cursor);
        assert!(readback);
        assert_eq!(first.len(), BATCH_SIZE);
        assert_eq!(first[0].sequence, 5);
        let mut sequences: Vec<_> = first.into_iter().map(|event| event.sequence).collect();
        loop {
            let (readback, events) = pull(&source, &mut cursor);
            assert!(!readback);
            if events.is_empty() {
                break;
            }
            assert!(events.len() <= BATCH_SIZE);
            sequences.extend(events.into_iter().map(|event| event.sequence));
        }
        assert_eq!(sequences, (5..=260).collect::<Vec<_>>());
    }

    #[test]
    fn ahead_cursor_requests_readback_and_invalid_response_does_not_advance() {
        let source = Events::default();
        let mut cursor = Cursor::default();
        source.push("runtime-log-stream", "null");
        pull(&source, &mut cursor);
        cursor.sequence = 100;
        let (readback, events) = pull(&source, &mut cursor);
        assert!(readback);
        assert_eq!(events[0].sequence, 1);
        let before = cursor.request_args();
        let invalid = json!({ "generation": source.state.lock().unwrap().generation.clone(), "reset": false, "events": [
            {"sequence": 3, "name": "runtime-log-stream", "payload": null},
            {"sequence": 2, "name": "runtime-log-stream", "payload": null}
        ] });
        assert!(cursor.accept(invalid).is_err());
        assert_eq!(cursor.request_args(), before);
    }
}
