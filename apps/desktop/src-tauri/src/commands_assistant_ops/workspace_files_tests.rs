use super::*;

fn document(file: &str, content: &str) -> app_storage::InstanceWorkspaceDocument {
    app_storage::InstanceWorkspaceDocument {
        entry: app_storage::InstanceWorkspaceEntry {
            file: file.into(),
            byte_length: content.len() as u64,
            editable: true,
            protection_reason: None,
        },
        source_sha256: "a".repeat(64),
        content: content.into(),
    }
}

fn page(files: Vec<app_storage::InstanceWorkspaceEntry>) -> app_storage::InstanceWorkspacePage {
    app_storage::InstanceWorkspacePage {
        files,
        directories: Vec::new(),
        next_offset: None,
        next_file_offset: None,
        next_directory_offset: None,
        listing_sha256: "b".repeat(64),
        scan_truncated: false,
        scanned_entries: 1,
    }
}

#[test]
fn workspace_read_redacts_before_paging_and_keeps_raw_hash() {
    let text = format!(
        "{}\npassword=synthetic-test-only\n你好，世界",
        "prefix".repeat(685)
    );
    let source = document("data/plugins/example/settings.txt", &text);
    let first = assistant_workspace_document_evidence(source, 0).unwrap();
    assert_eq!(first["sourceSha256"], "a".repeat(64));
    assert_eq!(first["redacted"], true);
    let second = assistant_workspace_document_evidence(
        document("data/plugins/example/settings.txt", &text),
        first["nextOffsetBytes"].as_u64().unwrap() as usize,
    )
    .unwrap();
    assert!(!first.to_string().contains("synthetic-test-only"));
    assert!(!second.to_string().contains("synthetic-test-only"));
    assert!(
        assistant_workspace_document_evidence(document("data/scripts/hello.lua", "你好"), 1)
            .is_err()
    );
}

#[test]
fn workspace_search_is_literal_redacted_and_returns_source_identity() {
    let text = "password=synthetic-test-only\nlocal result = call.*(value)\nreturn result";
    for (query, count) in [("synthetic-test-only", 0), ("call.*(", 1), ("CALL", 0)] {
        let source = document("data/scripts/test.lua", text);
        let result =
            assistant_workspace_search_results(page(vec![source.entry]), query, None, |file, _| {
                Ok(document(file, text))
            })
            .unwrap();
        assert_eq!(result["matches"].as_array().unwrap().len(), count);
        assert!(!result.to_string().contains("synthetic-test-only"));
        if count == 1 {
            assert_eq!(result["matches"][0]["line"], 2);
            assert_eq!(result["matches"][0]["sourceSha256"], "a".repeat(64));
        }
    }
}

#[test]
fn workspace_search_marks_read_failures_and_scan_limits_as_evidence_gaps() {
    let source = document("data/plugins/test.txt", "needle");
    let result =
        assistant_workspace_search_results(page(vec![source.entry]), "needle", None, |_, _| {
            Err("unreadable".into())
        })
        .unwrap();
    assert_eq!(result["scanTruncated"], true);
    assert_eq!(result["unreadableFiles"], json!(["data/plugins/test.txt"]));
    let content = "needle\n".repeat(129);
    let source = document("data/scripts/test.lua", &content);
    let result =
        assistant_workspace_search_results(page(vec![source.entry]), "needle", None, |file, _| {
            Ok(document(file, &content))
        })
        .unwrap();
    assert_eq!(
        result["matches"].as_array().unwrap().len(),
        ASSISTANT_WORKSPACE_SEARCH_PAGE
    );
    assert_eq!(
        result["nextCursor"]["lineOffset"],
        ASSISTANT_WORKSPACE_SEARCH_PAGE
    );
    assert_eq!(result["nextCursor"]["fileOffset"], 0);
    assert_eq!(result["scanTruncated"], false);
    assert_eq!(result["hasMore"], true);
    for query in ["", " ", "line\nbreak", &"x".repeat(129)] {
        assert!(validate_assistant_workspace_search(query, None).is_err());
    }
}

#[test]
fn workspace_search_failed_reads_cannot_bypass_budget_with_stale_lengths() {
    let entries = (0..ASSISTANT_WORKSPACE_SEARCH_FILES)
        .map(|index| document(&format!("data/scripts/{index}.txt"), "").entry)
        .collect();
    let mut possible_read_bytes = 0;
    let mut reads = 0;
    let result = assistant_workspace_search_results(page(entries), "needle", None, |_, maximum| {
        // A file grew after listing and failed decoding after consuming its
        // bounded buffer. Failure cannot reveal how many bytes were consumed.
        reads += 1;
        possible_read_bytes += maximum + 1;
        Err("invalid UTF-8 after reading the bounded buffer".into())
    })
    .unwrap();
    assert_eq!(reads, 4);
    assert_eq!(possible_read_bytes, ASSISTANT_WORKSPACE_SEARCH_BYTES);
    assert_eq!(result["scannedBytes"], ASSISTANT_WORKSPACE_SEARCH_BYTES);
    assert_eq!(result["scanTruncated"], true);
    assert_eq!(result["unreadableFiles"].as_array().unwrap().len(), reads);
    assert_eq!(result["nextCursor"]["fileOffset"], reads);
}

fn next_cursor(result: &Value) -> Option<AssistantWorkspaceSearchCursor> {
    serde_json::from_value(result["nextCursor"].clone()).unwrap()
}

#[test]
fn workspace_search_resumes_beyond_the_first_64_flat_files() {
    let mut cursor: Option<AssistantWorkspaceSearchCursor> = None;
    let mut found = Vec::new();
    for request in 0..3 {
        let offset = cursor.as_ref().map_or(0, |cursor| cursor.file_offset);
        let end = (offset + ASSISTANT_WORKSPACE_SEARCH_FILES).min(100);
        let mut page = page(
            (offset..end)
                .map(|index| document(&format!("data/scripts/{index:03}.lua"), "").entry)
                .collect(),
        );
        page.next_file_offset = (end < 100).then_some(end);
        let result = assistant_workspace_search_results(page, "needle", cursor, |file, _| {
            Ok(document(
                file,
                if file.ends_with("095.lua") {
                    "needle"
                } else {
                    "absent"
                },
            ))
        })
        .unwrap();
        found.extend(result["matches"].as_array().unwrap().iter().cloned());
        if request == 0 {
            assert!(found.is_empty());
            assert_eq!(result["nextCursor"]["fileOffset"], 64);
            assert_eq!(result["hasMore"], true);
        }
        cursor = next_cursor(&result);
        if cursor.is_none() {
            break;
        }
    }
    assert!(cursor.is_none());
    assert_eq!(found.len(), 1);
    assert_eq!(found[0]["file"], "data/scripts/095.lua");
}

#[test]
fn workspace_search_resumes_every_match_in_a_large_single_file() {
    let content = "needle\n".repeat(129);
    let mut cursor = None;
    let mut lines = Vec::new();
    for _ in 0..10 {
        let source = document("data/scripts/test.lua", &content);
        let result = assistant_workspace_search_results(
            page(vec![source.entry]),
            "needle",
            cursor,
            |file, _| Ok(document(file, &content)),
        )
        .unwrap();
        assert_eq!(result["scanTruncated"], false);
        lines.extend(
            result["matches"]
                .as_array()
                .unwrap()
                .iter()
                .map(|found| found["line"].as_u64().unwrap()),
        );
        cursor = next_cursor(&result);
        if cursor.is_none() {
            break;
        }
    }
    assert!(cursor.is_none());
    assert_eq!(lines, (1..=129).collect::<Vec<_>>());
}

#[test]
fn workspace_search_rejects_changed_query_listing_and_resumed_content() {
    let content = "needle\n".repeat(30);
    let entry = document("data/scripts/test.lua", &content).entry;
    let first =
        assistant_workspace_search_results(page(vec![entry.clone()]), "needle", None, |file, _| {
            Ok(document(file, &content))
        })
        .unwrap();
    let cursor = next_cursor(&first).unwrap();
    assert!(validate_assistant_workspace_search("changed", Some(&cursor)).is_err());
    let mut listing = page(vec![entry.clone()]);
    listing.listing_sha256 = "c".repeat(64);
    assert!(
        assistant_workspace_search_results(listing, "needle", Some(cursor.clone()), |_, _| {
            panic!("Changed listing must be rejected before reading a file")
        })
        .unwrap_err()
        .contains("listing changed")
    );
    assert!(
        assistant_workspace_search_results(
            page(vec![entry]),
            "needle",
            Some(cursor.clone()),
            |file, _| {
                let mut changed = document(file, &content);
                changed.source_sha256 = "d".repeat(64);
                Ok(changed)
            }
        )
        .unwrap_err()
        .contains("file changed")
    );
    let mut invalid = cursor;
    invalid.source_sha256 = None;
    assert!(validate_assistant_workspace_search("needle", Some(&invalid)).is_err());
}

#[test]
fn workspace_read_pages_preserve_redacted_utf8_and_bound_json_expansion() {
    for text in [
        format!(
            "{}\npassword=synthetic-test-only\n{}",
            "你好🦀".repeat(800),
            "尾部".repeat(900)
        ),
        "\u{01}".repeat(6000),
    ] {
        let file = "data/scripts/test.txt";
        let expected = redact_assistant_file_content(Path::new(file), &text).unwrap();
        let mut combined = String::new();
        let mut offset = 0;
        for _ in 0..32 {
            let result =
                assistant_workspace_document_evidence(document(file, &text), offset).unwrap();
            assert!(result.to_string().len() < ASSISTANT_TOOL_RESULT_BYTES);
            assert!(!result.to_string().contains("synthetic-test-only"));
            combined.push_str(result["content"].as_str().unwrap());
            match result["nextOffsetBytes"].as_u64() {
                Some(next) => {
                    assert!(next as usize > offset);
                    offset = next as usize;
                }
                None => break,
            }
        }
        assert_eq!(combined, expected);
    }
}

#[test]
fn workspace_search_pages_long_paths_and_centers_unicode_match_snippets() {
    let files = (0..40)
        .map(|index| format!("data/scripts/{}/{index:03}.txt", "x".repeat(800)))
        .collect::<Vec<_>>();
    let content = format!("{}needle{}", "界".repeat(800), "尾".repeat(800));
    let mut cursor: Option<AssistantWorkspaceSearchCursor> = None;
    let mut found = Vec::new();
    for _ in 0..40 {
        let offset = cursor.as_ref().map_or(0, |cursor| cursor.file_offset);
        let entries = files
            .iter()
            .skip(offset)
            .map(|file| document(file, &content).entry)
            .collect();
        let result =
            assistant_workspace_search_results(page(entries), "needle", cursor, |file, _| {
                Ok(document(file, &content))
            })
            .unwrap();
        assert!(result.to_string().len() < ASSISTANT_TOOL_RESULT_BYTES);
        for item in result["matches"].as_array().unwrap() {
            assert!(item["text"].as_str().unwrap().contains("needle"));
            assert_eq!(item["textTruncated"], true);
            assert!(item["snippetOffsetBytes"].as_u64().unwrap() > 0);
            found.push(item["file"].as_str().unwrap().to_owned());
        }
        cursor = next_cursor(&result);
        if cursor.is_none() {
            break;
        }
    }
    assert!(cursor.is_none());
    assert_eq!(found, files);
}

#[test]
fn workspace_search_oversized_file_does_not_hide_later_readable_file() {
    let mut large = document("data/scripts/000.txt", "").entry;
    large.byte_length = 2 * ASSISTANT_WORKSPACE_SEARCH_BYTES as u64;
    let small = document("data/scripts/001.txt", "needle").entry;
    let result = assistant_workspace_search_results(
        page(vec![large, small]),
        "needle",
        None,
        |file, maximum| {
            assert!(maximum <= 256 * 1024);
            if file.ends_with("000.txt") {
                Err("file too large".into())
            } else {
                Ok(document(file, "needle"))
            }
        },
    )
    .unwrap();
    assert_eq!(result["scanTruncated"], true);
    assert_eq!(result["matches"][0]["file"], "data/scripts/001.txt");
    assert!(result["nextCursor"].is_null());
}
