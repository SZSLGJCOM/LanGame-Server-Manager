use serde::Serialize;
use std::collections::VecDeque;
use std::sync::Mutex;

const MAX_EVENTS: usize = 256;
const MAX_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AssistantProgressEvent {
    cursor: u64,
    kind: String,
    text: String,
    tool_name: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AssistantProgressSnapshot {
    revision: u64,
    cursor: u64,
    reset: bool,
    text: String,
    events: Vec<AssistantProgressEvent>,
}

#[derive(Debug, Default)]
struct ProgressState {
    revision: u64,
    cursor: u64,
    cancelled: bool,
    text: String,
    bytes: usize,
    events: VecDeque<AssistantProgressEvent>,
}

/// Transient observation only: progress never authorizes work or becomes the
/// durable conversation transcript. Revision ownership rejects stale producers.
#[derive(Debug, Default)]
pub(crate) struct AssistantSessionProgress(Mutex<ProgressState>);

impl AssistantSessionProgress {
    pub(crate) fn begin(&self, revision: u64) -> Result<(), String> {
        let mut state = self
            .0
            .lock()
            .map_err(|_| "Assistant progress is unavailable.")?;
        state.revision = revision;
        state.cancelled = false;
        state.text.clear();
        state.bytes = 0;
        state.events.clear();
        Ok(())
    }

    pub(crate) fn cancel(&self) {
        // Cancellation is fail-closed even if a prior producer poisoned the lock.
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .cancelled = true;
    }

    pub(crate) fn record(
        &self,
        revision: u64,
        kind: &str,
        text: &str,
        tool_name: Option<&str>,
    ) -> Result<(), String> {
        if !matches!(
            kind,
            "model_start"
                | "text_delta"
                | "phase"
                | "tool_started"
                | "tool_completed"
                | "tool_failed"
        ) || text.len() > MAX_BYTES
            || tool_name.is_some_and(|name| {
                name.len() > 64
                    || !name
                        .bytes()
                        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'_' | b'-'))
            })
        {
            return Err("Assistant progress exceeded its observation boundary.".into());
        }
        let mut state = self
            .0
            .lock()
            .map_err(|_| "Assistant progress is unavailable.")?;
        if state.revision != revision || state.cancelled {
            return Err("Assistant progress belongs to an inactive turn.".into());
        }
        if kind == "model_start" {
            state.text.clear();
        } else if kind == "text_delta" {
            if state.text.len().saturating_add(text.len()) > MAX_BYTES {
                return Err("Assistant public reply exceeded its progress text limit.".into());
            }
            state.text.push_str(text);
        }
        state.cursor = state
            .cursor
            .checked_add(1)
            .ok_or("Assistant progress cursor exhausted.")?;
        let event = AssistantProgressEvent {
            cursor: state.cursor,
            kind: kind.into(),
            text: text.into(),
            tool_name: tool_name.map(str::to_owned),
        };
        state.bytes +=
            event.text.len() + event.kind.len() + event.tool_name.as_ref().map_or(0, String::len);
        state.events.push_back(event);
        while state.events.len() > MAX_EVENTS || state.bytes > MAX_BYTES {
            if let Some(event) = state.events.pop_front() {
                state.bytes -= event.text.len()
                    + event.kind.len()
                    + event.tool_name.as_ref().map_or(0, String::len);
            }
        }
        Ok(())
    }

    pub(crate) fn snapshot(
        &self,
        after_cursor: Option<u64>,
    ) -> Result<AssistantProgressSnapshot, String> {
        let state = self
            .0
            .lock()
            .map_err(|_| "Assistant progress is unavailable.")?;
        let floor = state
            .events
            .front()
            .map_or(state.cursor, |event| event.cursor.saturating_sub(1));
        let reset = after_cursor.is_none_or(|cursor| cursor < floor || cursor > state.cursor);
        Ok(AssistantProgressSnapshot {
            revision: state.revision,
            cursor: state.cursor,
            reset,
            text: state.text.clone(),
            events: state
                .events
                .iter()
                .filter(|event| reset || after_cursor.is_none_or(|cursor| event.cursor > cursor))
                .cloned()
                .collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overwritten_cursors_recover_text_and_cancelled_producers_cannot_append() {
        let progress = AssistantSessionProgress::default();
        progress.begin(1).unwrap();
        for _ in 0..MAX_EVENTS + 10 {
            progress.record(1, "text_delta", "界", None).unwrap();
        }
        let snapshot = progress.snapshot(Some(1)).unwrap();
        assert!(snapshot.reset);
        assert_eq!(snapshot.events.len(), MAX_EVENTS);
        assert_eq!(snapshot.text, "界".repeat(MAX_EVENTS + 10));
        progress.cancel();
        assert!(progress.record(1, "text_delta", "late", None).is_err());
        progress.begin(2).unwrap();
        assert!(progress.record(1, "text_delta", "old turn", None).is_err());
        progress.record(2, "model_start", "", None).unwrap();
        progress.record(2, "text_delta", "new", None).unwrap();
        assert_eq!(progress.snapshot(None).unwrap().text, "new");
    }
}
