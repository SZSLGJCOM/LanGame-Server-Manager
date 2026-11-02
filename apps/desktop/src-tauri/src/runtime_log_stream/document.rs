use serde::Serialize;

const MAX_LINES: usize = 400;
const MAX_BYTES: usize = 512 * 1024;

/// A cursor describes a published document, not a filesystem read offset. A
/// partial row can change without committing a line or advancing that offset.
#[derive(Clone, Debug, Serialize)]
pub struct ConsoleStreamUpdate {
    pub stream_id: String,
    pub started_at_unix_ms: u128,
    pub previous_revision: u64,
    pub revision: u64,
    pub reset: bool,
    pub partial_lines: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ConsoleSourceDocument {
    pub log_path: String,
    pub process_key: Option<String>,
    pub display_name: Option<String>,
    pub run_id: Option<i64>,
    pub started_at_unix_ms: u128,
    pub stream_id: String,
    pub revision: u64,
    pub lines: Vec<String>,
    pub partial_lines: Vec<String>,
    pub total_lines: u64,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_error: Option<String>,
    #[serde(skip)]
    pub(crate) requires_readback: bool,
    #[serde(skip)]
    pub(crate) reader_byte_offset: u64,
}

impl ConsoleSourceDocument {
    pub fn new(
        log_path: &str,
        process_key: Option<&str>,
        display_name: Option<&str>,
        run_id: Option<i64>,
        started_at_unix_ms: u128,
    ) -> Self {
        Self {
            log_path: log_path.into(),
            process_key: process_key.map(str::to_owned),
            display_name: display_name.map(str::to_owned),
            run_id,
            started_at_unix_ms,
            stream_id: uuid::Uuid::new_v4().to_string(),
            revision: 0,
            lines: Vec::new(),
            partial_lines: Vec::new(),
            total_lines: 0,
            truncated: false,
            read_error: None,
            requires_readback: false,
            reader_byte_offset: 0,
        }
    }

    pub fn publish(
        &mut self,
        lines: &[String],
        partial_lines: Vec<String>,
        reset: bool,
        epoch: u128,
    ) -> ConsoleStreamUpdate {
        if reset && self.revision != 0 {
            self.stream_id = uuid::Uuid::new_v4().to_string();
            self.started_at_unix_ms = epoch;
            self.revision = 0;
            self.lines.clear();
            self.total_lines = 0;
            self.truncated = false;
        }
        let previous_revision = self.revision;
        // At 60 updates/second, exhausting u64 requires billions of years.
        self.revision += 1;
        self.total_lines += lines.len() as u64;
        self.lines.extend_from_slice(lines);
        self.partial_lines = partial_lines;
        self.truncated |= bound_lines(&mut self.lines);
        ConsoleStreamUpdate {
            stream_id: self.stream_id.clone(),
            started_at_unix_ms: self.started_at_unix_ms,
            previous_revision,
            revision: self.revision,
            reset,
            partial_lines: self.partial_lines.clone(),
        }
    }

    pub(crate) fn refresh_readback(&mut self) {
        if !self.requires_readback {
            return;
        }
        // The producer may continue after its observer failed. Keep this flag
        // until a new lease so every recovery read observes the retained file.
        let snapshot = app_storage::read_log_path_snapshot(self.log_path.clone(), MAX_LINES);
        let mut lines = snapshot.lines;
        let truncated = snapshot.truncated | bound_lines(&mut lines);
        let total_lines = snapshot.total_lines as u64;
        if self.lines != lines
            || !self.partial_lines.is_empty()
            || self.total_lines != total_lines
            || self.truncated != truncated
            || self.read_error != snapshot.read_error
        {
            self.revision += 1;
            self.lines = lines;
            self.partial_lines.clear();
            self.total_lines = total_lines;
            self.truncated = truncated;
            self.read_error = snapshot.read_error;
        }
    }
}

fn bound_lines(lines: &mut Vec<String>) -> bool {
    let mut bytes = lines.iter().map(String::len).sum::<usize>();
    let mut discard = 0;
    while lines.len() - discard > MAX_LINES || bytes > MAX_BYTES {
        bytes -= lines[discard].len();
        discard += 1;
    }
    if discard > 0 {
        lines.drain(..discard);
    }
    discard > 0
}

#[cfg(test)]
#[path = "document_tests.rs"]
mod tests;
