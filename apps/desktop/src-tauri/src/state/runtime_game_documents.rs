use super::{RuntimeLogStreamLease, RuntimeLogStreams, stream_key};
use crate::runtime_log_stream::document_patch::{
    NativeLogDocument, RuntimeLogDocumentPatch, document_patch, same_document,
};

impl RuntimeLogStreams {
    pub(crate) fn begin_native_document(
        &mut self,
        instance_id: &str,
        log_path: &str,
        lease: RuntimeLogStreamLease,
        document: NativeLogDocument,
    ) {
        if let Some(entry) = self.active.get_mut(&stream_key(instance_id, log_path))
            && entry.lease == lease
            && entry.native_document.is_none()
        {
            entry.native_document = Some(document);
        }
    }

    pub(crate) fn publish_native_document(
        &mut self,
        instance_id: &str,
        log_path: &str,
        lease: RuntimeLogStreamLease,
        document: NativeLogDocument,
        reset: bool,
    ) -> Option<RuntimeLogDocumentPatch> {
        let entry = self.active.get_mut(&stream_key(instance_id, log_path))?;
        if entry.lease != lease {
            return None;
        }
        let previous = entry.native_document.as_ref()?;
        if previous.run_id != document.run_id || document.revision <= previous.revision {
            return None;
        }
        if !reset && same_document(&previous.snapshot, &document.snapshot) {
            return None;
        }
        let patch = document_patch(previous, &document, reset);
        entry.native_document = Some(document);
        Some(patch)
    }

    /// File reads may allocate unrelated revisions. Readback must return the
    /// exact last publication used as the next patch's acknowledged base.
    /// The returned flag identifies a completed lease with no future publisher.
    pub(crate) fn native_document(
        &self,
        instance_id: &str,
        log_path: &str,
        run_id: i64,
    ) -> Option<(NativeLogDocument, bool)> {
        let key = stream_key(instance_id, log_path);
        if let Some(entry) = self.active.get(&key) {
            return entry
                .native_document
                .as_ref()
                .filter(|document| document.run_id == run_id)
                .map(|document| (document.clone(), false));
        }
        self.completed_native_documents
            .iter()
            .rev()
            .find(|(stored_key, document)| *stored_key == key && document.run_id == run_id)
            .map(|(_, document)| (document.clone(), true))
    }

    pub(super) fn retain_native_document(
        &mut self,
        key: (String, String),
        document: NativeLogDocument,
    ) {
        self.completed_native_documents
            .retain(|(stored_key, _)| *stored_key != key);
        self.completed_native_documents.push_back((key, document));
        let document_bytes = |document: &NativeLogDocument| {
            document
                .snapshot
                .lines
                .iter()
                .map(String::len)
                .sum::<usize>()
                + document.snapshot.read_error.as_ref().map_or(0, String::len)
        };
        let mut bytes: usize = self
            .completed_native_documents
            .iter()
            .map(|(_, document)| document_bytes(document))
            .sum();
        while self.completed_native_documents.len() > 128 || bytes > 32 * 1024 * 1024 {
            if let Some((_, document)) = self.completed_native_documents.pop_front() {
                bytes -= document_bytes(&document);
            }
        }
    }
}

#[cfg(test)]
#[path = "runtime_game_documents_tests.rs"]
mod tests;
