const ASSISTANT_WORKSPACE_PAGE_BYTES: usize = 4096;
const ASSISTANT_WORKSPACE_SEARCH_BYTES: usize = 1024 * 1024;
const ASSISTANT_WORKSPACE_SEARCH_FILES: usize = 64;
const ASSISTANT_WORKSPACE_SEARCH_PAGE: usize = 24;
const ASSISTANT_WORKSPACE_SEARCH_PAYLOAD_BYTES: usize = 7 * 1024;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct AssistantWorkspaceSearchCursor {
    pub(super) file_offset: usize,
    pub(super) line_offset: usize,
    pub(super) source_sha256: Option<String>,
    pub(super) listing_sha256: String,
    pub(super) query: String,
}

pub(super) async fn list_assistant_workspace_files(
    state: &DesktopState,
    storage: &StorageBootstrap,
    instance: &InstanceDetails,
    directory: &str,
    offset: usize,
) -> Result<Value, String> {
    let _lease = state.begin_storage_context_operation("assistant workspace listing")?;
    ensure_storage_context_snapshot_current(state, storage, "assistant workspace listing")?;
    let workspace = app_storage::open_instance_workspace(&storage.paths, &instance.summary.id)
        .await
        .map_err(|error| redact_assistant_provider_text(&error.to_string()))?;
    let directory = directory.to_owned();
    run_assistant_evidence_read(state, move || {
        workspace
            .list_files(&directory, offset)
            .map_err(|error| redact_assistant_provider_text(&error.to_string()))
            .and_then(|page| serde_json::to_value(page).map_err(|error| error.to_string()))
    })
    .await
}

pub(super) async fn read_assistant_workspace_file(
    state: &DesktopState,
    storage: &StorageBootstrap,
    instance: &InstanceDetails,
    file: &str,
    offset: usize,
) -> Result<Value, String> {
    let _lease = state.begin_storage_context_operation("assistant workspace read")?;
    ensure_storage_context_snapshot_current(state, storage, "assistant workspace read")?;
    let workspace = app_storage::open_instance_workspace(&storage.paths, &instance.summary.id)
        .await
        .map_err(|error| redact_assistant_provider_text(&error.to_string()))?;
    let file = file.to_owned();
    run_assistant_evidence_read(state, move || {
        let document = workspace
            .read_file(&file)
            .map_err(|error| redact_assistant_provider_text(&error.to_string()))?;
        assistant_workspace_document_evidence(document, offset)
    })
    .await
}

fn assistant_workspace_document_evidence(
    document: app_storage::InstanceWorkspaceDocument,
    offset: usize,
) -> Result<Value, String> {
    let content =
        redact_assistant_file_content(Path::new(&document.entry.file), &document.content)?;
    if offset > content.len() || !content.is_char_boundary(offset) {
        return Err(String::from(
            "Use a UTF-8 offset from nextOffsetBytes in this redacted document.",
        ));
    }
    let mut end = offset;
    let mut encoded_bytes = 0;
    for ch in content[offset..].chars() {
        let needed = match ch {
            '"' | '\\' | '\u{08}' | '\t' | '\n' | '\u{0c}' | '\r' => 2,
            ch if ch < ' ' => 6,
            ch => ch.len_utf8(),
        };
        if end + ch.len_utf8() - offset > ASSISTANT_WORKSPACE_PAGE_BYTES
            || encoded_bytes + needed > 6 * 1024
        {
            break;
        }
        end += ch.len_utf8();
        encoded_bytes += needed;
    }
    Ok(json!({
        "file": document.entry.file, "sourceSha256": document.source_sha256,
        "byteLength": document.entry.byte_length, "editable": document.entry.editable,
        "redactedByteLength": content.len(), "offsetBasis": "redacted_utf8_bytes",
        "protectionReason": document.entry.protection_reason,
        "content": &content[offset..end], "offsetBytes": offset,
        "nextOffsetBytes": (end < content.len()).then_some(end),
        "redacted": content != document.content, "truncated": end < content.len(),
    }))
}

pub(super) async fn search_assistant_workspace_files(
    state: &DesktopState,
    storage: &StorageBootstrap,
    instance: &InstanceDetails,
    directory: &str,
    query: &str,
    cursor: Option<AssistantWorkspaceSearchCursor>,
) -> Result<Value, String> {
    validate_assistant_workspace_search(query, cursor.as_ref())?;
    let _lease = state.begin_storage_context_operation("assistant workspace search")?;
    ensure_storage_context_snapshot_current(state, storage, "assistant workspace search")?;
    let workspace = app_storage::open_instance_workspace(&storage.paths, &instance.summary.id)
        .await
        .map_err(|error| redact_assistant_provider_text(&error.to_string()))?;
    let directory = directory.to_owned();
    let query = query.to_owned();
    run_assistant_evidence_read(state, move || {
        let page = workspace
            .list_files(
                &directory,
                cursor.as_ref().map_or(0, |cursor| cursor.file_offset),
            )
            .map_err(|error| redact_assistant_provider_text(&error.to_string()))?;
        assistant_workspace_search_results(page, &query, cursor, |file, maximum| {
            workspace
                .read_file_with_limit(file, maximum)
                .map_err(|error| redact_assistant_provider_text(&error.to_string()))
        })
    })
    .await
}

fn validate_assistant_workspace_search(
    query: &str,
    cursor: Option<&AssistantWorkspaceSearchCursor>,
) -> Result<(), String> {
    if query.trim().is_empty() || query.len() > 128 || query.chars().any(char::is_control) {
        return Err(String::from(
            "Use 1 to 128 UTF-8 bytes of literal search text.",
        ));
    }
    if let Some(cursor) = cursor {
        let hash_valid = |hash: &str| {
            hash.len() == 64
                && hash
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        };
        if cursor.file_offset > 2048
            || cursor.line_offset > 256 * 1024
            || (cursor.line_offset > 0 && cursor.source_sha256.is_none())
            || !hash_valid(&cursor.listing_sha256)
            || cursor
                .source_sha256
                .as_deref()
                .is_some_and(|hash| !hash_valid(hash))
            || cursor.query != query
        {
            return Err(String::from(
                "Use nextCursor unchanged with the same directory and query; restart with null after changing the search.",
            ));
        }
    }
    Ok(())
}

fn assistant_workspace_search_results(
    page: app_storage::InstanceWorkspacePage,
    query: &str,
    cursor: Option<AssistantWorkspaceSearchCursor>,
    mut read: impl FnMut(&str, usize) -> Result<app_storage::InstanceWorkspaceDocument, String>,
) -> Result<Value, String> {
    validate_assistant_workspace_search(query, cursor.as_ref())?;
    if cursor
        .as_ref()
        .is_some_and(|cursor| cursor.listing_sha256 != page.listing_sha256)
    {
        return Err(String::from(
            "Workspace listing changed during search; restart with cursor null to avoid skipped or repeated files.",
        ));
    }
    if cursor.is_some() && page.files.is_empty() {
        return Err(String::from(
            "Search cursor is outside this workspace listing; restart with cursor null.",
        ));
    }
    let offset = cursor.as_ref().map_or(0, |cursor| cursor.file_offset);
    let make_cursor = |file_offset, line_offset, source_sha256| {
        (file_offset < offset + page.files.len() || page.next_file_offset.is_some()).then(|| {
            AssistantWorkspaceSearchCursor {
                file_offset,
                line_offset,
                source_sha256,
                listing_sha256: page.listing_sha256.clone(),
                query: query.to_owned(),
            }
        })
    };
    let mut next_cursor = page
        .next_file_offset
        .and_then(|next| make_cursor(next, 0, None));
    let mut matches = Vec::new();
    let mut unreadable = Vec::new();
    let mut payload_bytes = 0;
    let mut scanned_bytes = 0_usize;
    let mut scanned_files = 0_usize;
    let mut truncated = page.scan_truncated;
    'files: for (file_index, entry) in page
        .files
        .iter()
        .take(ASSISTANT_WORKSPACE_SEARCH_FILES)
        .enumerate()
    {
        let file_offset = offset + file_index;
        let resumed = cursor.as_ref().filter(|_| file_index == 0);
        let line_offset = resumed.map_or(0, |cursor| cursor.line_offset);
        let source_sha256 = resumed.and_then(|cursor| cursor.source_sha256.clone());
        let remaining = ASSISTANT_WORKSPACE_SEARCH_BYTES.saturating_sub(scanned_bytes);
        let path_bytes = serde_json::to_vec(&entry.file)
            .map_err(|error| error.to_string())?
            .len()
            + 1;
        if remaining == 0
            || (entry.byte_length <= 256 * 1024 && entry.byte_length >= remaining as u64)
            || payload_bytes + path_bytes > ASSISTANT_WORKSPACE_SEARCH_PAYLOAD_BYTES
        {
            next_cursor = make_cursor(file_offset, line_offset, source_sha256);
            break;
        }
        scanned_files += 1;
        // Failed reads may have consumed the entire bounded buffer. Reserve it
        // before opening, independent of stale directory metadata, and include
        // the overflow-detection byte. Refund only a successful, measured read.
        let reserved = remaining.min(256 * 1024 + 1);
        let maximum = reserved - 1;
        scanned_bytes += reserved;
        match read(&entry.file, maximum) {
            Ok(document) => {
                if document.content.len() > maximum {
                    return Err(String::from(
                        "Workspace reader exceeded the reserved byte budget.",
                    ));
                }
                scanned_bytes -= reserved - document.content.len();
                if source_sha256
                    .as_ref()
                    .is_some_and(|expected| *expected != document.source_sha256)
                {
                    return Err(String::from(
                        "Workspace file changed during search; restart with cursor null to avoid skipped or repeated lines.",
                    ));
                }
                let content = redact_assistant_file_content(
                    Path::new(&document.entry.file),
                    &document.content,
                )?;
                let line_count = content.lines().count();
                if line_offset > line_count {
                    return Err(String::from(
                        "Search cursor is beyond the redacted document; restart with cursor null.",
                    ));
                }
                for (line_index, line) in content.lines().enumerate().skip(line_offset) {
                    if line.contains(query) {
                        let (start, end) = assistant_workspace_match_window(line, query);
                        let found = json!({
                            "file": document.entry.file, "line": line_index + 1,
                            "text": &line[start..end], "snippetOffsetBytes": start,
                            "textTruncated": start > 0 || end < line.len(),
                            "sourceSha256": document.source_sha256, "editable": document.entry.editable,
                        });
                        let needed = found.to_string().len() + 1;
                        if payload_bytes + needed > ASSISTANT_WORKSPACE_SEARCH_PAYLOAD_BYTES {
                            next_cursor = make_cursor(
                                file_offset,
                                line_index,
                                Some(document.source_sha256.clone()),
                            );
                            break 'files;
                        }
                        payload_bytes += needed;
                        matches.push(found);
                        if matches.len() == ASSISTANT_WORKSPACE_SEARCH_PAGE {
                            next_cursor = if line_index + 1 < line_count {
                                make_cursor(
                                    file_offset,
                                    line_index + 1,
                                    Some(document.source_sha256.clone()),
                                )
                            } else {
                                make_cursor(file_offset + 1, 0, None)
                            };
                            break 'files;
                        }
                    }
                }
            }
            Err(_) => {
                if source_sha256.is_some() {
                    return Err(String::from(
                        "The resumed workspace file is unreadable; restart with cursor null and inspect the evidence gap.",
                    ));
                }
                truncated = true;
                unreadable.push(entry.file.clone());
                payload_bytes += path_bytes;
            }
        }
    }
    Ok(json!({
        "matches": matches, "nextCursor": next_cursor,
        "scanTruncated": truncated, "hasMore": next_cursor.is_some(),
        "lineBasis": "redacted_utf8", "listingSha256": page.listing_sha256,
        "scannedFiles": scanned_files, "scannedBytes": scanned_bytes, "unreadableFiles": unreadable,
        "scannedBytesIsUpperBound": !unreadable.is_empty(),
        "guidance": "Literal, case-sensitive search of redacted UTF-8 text. Pass nextCursor unchanged with the same directory/query to continue. scanTruncated or unreadableFiles is an evidence gap, even after the final page. The catalog covers at most 2048 entries and 12 directory levels; narrow directory if truncated. Zero matches on an unfinished page do not prove absence.",
    }))
}

fn assistant_workspace_match_window(line: &str, query: &str) -> (usize, usize) {
    let position = line.find(query).unwrap_or_default();
    let mut start = position.saturating_sub(96);
    while !line.is_char_boundary(start) {
        start += 1;
    }
    let mut end = (start + 320).min(line.len());
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    (start, end)
}

#[cfg(test)]
#[path = "workspace_files_tests.rs"]
mod workspace_files_tests;
