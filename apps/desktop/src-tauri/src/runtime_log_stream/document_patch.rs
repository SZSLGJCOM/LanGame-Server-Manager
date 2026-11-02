use app_core::LogTailSnapshot;
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct RuntimeLogDocumentPatch {
    pub previous_revision: u64,
    pub revision: u64,
    pub start: usize,
    pub delete_count: usize,
    pub lines: Vec<String>,
    pub total_lines: usize,
    pub truncated: bool,
    pub read_error: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct NativeLogDocument {
    pub snapshot: LogTailSnapshot,
    pub revision: u64,
    pub run_id: i64,
}

pub(crate) fn same_document(previous: &LogTailSnapshot, current: &LogTailSnapshot) -> bool {
    previous.source_path == current.source_path
        && previous.lines == current.lines
        && previous.total_lines == current.total_lines
        && previous.truncated == current.truncated
        && previous.read_error == current.read_error
}

pub(crate) fn document_patch(
    previous: &NativeLogDocument,
    current: &NativeLogDocument,
    reset: bool,
) -> RuntimeLogDocumentPatch {
    let old = &previous.snapshot.lines;
    let new = &current.snapshot.lines;
    // These are exact positions in one acknowledged document. Equal text at
    // different positions is never used to infer a cursor or remove repeats.
    let prefix = if reset {
        0
    } else {
        old.iter().zip(new).take_while(|(a, b)| a == b).count()
    };
    let suffix = if reset {
        0
    } else {
        old[prefix..]
            .iter()
            .rev()
            .zip(new[prefix..].iter().rev())
            .take_while(|(a, b)| a == b)
            .count()
    };
    RuntimeLogDocumentPatch {
        previous_revision: previous.revision,
        revision: current.revision,
        start: prefix,
        delete_count: old.len() - prefix - suffix,
        lines: new[prefix..new.len() - suffix].to_vec(),
        total_lines: current.snapshot.total_lines,
        truncated: current.snapshot.truncated,
        read_error: current.snapshot.read_error.clone(),
    }
}

#[cfg(test)]
#[path = "document_patch_tests.rs"]
mod tests;
