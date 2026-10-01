use std::fs;

use app_core::{RuntimeLivePlayerStatus, RuntimePlayerIdentityKind};

use super::dst_client_table::{
    DstCaptureOutcome, MAX_CAPTURE_BYTES, MAX_FIELD_CHARS, MAX_LINE_BYTES, MAX_PLAYER_ENTRIES,
    parse_dst_client_table,
};
use crate::runtime_log_stream::{RuntimeLogTailState, read_runtime_log_delta_bounded};

const REQUEST_ID: &str = "0123456789abcdef0123456789abcdef";
const ACTION_IDS: &[&str] = &["kick_userid"];

fn fixture_lines(contents: &str) -> Vec<String> {
    contents.lines().map(str::to_owned).collect()
}

fn parse(lines: &[String]) -> DstCaptureOutcome {
    parse_dst_client_table(lines, REQUEST_ID, ACTION_IDS)
}

#[test]
fn dst_client_table_accepts_only_correlated_complete_capture_and_maps_public_rows() {
    let lines = fixture_lines(include_str!("../../test-data/live-players/dst/ready.txt"));

    let DstCaptureOutcome::Complete(capture) = parse(&lines) else {
        panic!("matching complete capture must be authoritative");
    };

    assert_eq!(capture.status, RuntimeLivePlayerStatus::Ready);
    assert!(capture.complete);
    assert!(!capture.truncated);
    assert_eq!(capture.current_players, Some(3));
    assert_eq!(capture.raw_valid_count, 3);
    assert_eq!(capture.unique_count, 2);
    assert_eq!(capture.stored_count, 2);
    assert_eq!(capture.players.len(), 2);

    let first = &capture.players[0];
    assert_eq!(first.entry.display_name, "星火");
    assert_eq!(first.entry.role, None);
    assert_eq!(first.entry.attributes[0].key, "character");
    assert_eq!(first.entry.attributes[0].value, "wilson");
    assert_eq!(first.entry.identifiers.len(), 1);
    assert_eq!(
        first.entry.identifiers[0].kind,
        RuntimePlayerIdentityKind::KleiUserId
    );
    assert_eq!(first.entry.identifiers[0].value, "KU_alpha");
    assert!(first.entry.identifiers[0].stable);
    assert_eq!(first.entry.available_action_ids, ["kick_userid"]);
    assert_eq!(
        first.action_bindings.get("kick_userid"),
        Some(&String::from("KU_alpha"))
    );

    let second = &capture.players[1];
    assert_eq!(second.entry.display_name, "玩家 [LGM-DST-PLAYERS-END]");
    assert_eq!(second.entry.role.as_deref(), Some("admin"));
    assert_ne!(first.entry.player_key, second.entry.player_key);
    assert!(!first.entry.player_key.contains("KU_alpha"));

    let public_json = serde_json::to_value(&first.entry).expect("serialize public row");
    let public_text = public_json.to_string();
    assert!(!public_text.contains("action_bindings"));
    assert!(!public_text.contains("TheNet:Kick"));
}

#[test]
fn dst_client_table_accepts_correlated_zero_count_as_ready_empty_capture() {
    let lines = fixture_lines(include_str!("../../test-data/live-players/dst/empty.txt"));

    let DstCaptureOutcome::Complete(capture) = parse(&lines) else {
        panic!("zero-count END must be a complete capture");
    };

    assert_eq!(capture.status, RuntimeLivePlayerStatus::Ready);
    assert!(capture.complete);
    assert_eq!(capture.current_players, Some(0));
    assert!(capture.players.is_empty());
}

#[test]
fn dst_client_table_accepts_native_print_trailing_tab_without_losing_empty_fields() {
    for (rows, count) in [
        (String::new(), 0),
        (
            format!("[00:01:02]: [LGM-DST-PLAYER]\t{REQUEST_ID}\t1\tKU_valid\t\t\t0\t\n"),
            1,
        ),
    ] {
        let lines = fixture_lines(&format!(
            "[00:01:02]: [LGM-DST-PLAYERS-BEGIN]\t{REQUEST_ID}\t\n\
             {rows}[00:01:02]: [LGM-DST-PLAYERS-END]\t{REQUEST_ID}\t{count}\t\n"
        ));
        let DstCaptureOutcome::Complete(capture) = parse(&lines) else {
            panic!("native print framing must preserve a complete capture");
        };
        assert_eq!(capture.current_players, Some(count));
        assert_eq!(capture.players.len(), count);
        if let Some(player) = capture.players.first() {
            assert_eq!(player.entry.display_name, "");
            assert_eq!(player.entry.attributes[0].value, "");
        }
    }
}

#[test]
fn dst_client_table_rejects_extra_columns_after_native_print_framing() {
    for suffix in ["\t\t", "\textra\t"] {
        let lines = fixture_lines(&format!(
            "[LGM-DST-PLAYERS-BEGIN]\t{REQUEST_ID}\t\n\
             [LGM-DST-PLAYER]\t{REQUEST_ID}\t1\tKU_valid\tPlayer\twilson\t0{suffix}\n\
             [LGM-DST-PLAYERS-END]\t{REQUEST_ID}\t1\t\n"
        ));
        assert!(matches!(parse(&lines), DstCaptureOutcome::Incomplete(_)));
    }
}

#[test]
fn dst_client_table_never_turns_partial_malformed_or_mismatched_capture_into_empty_success() {
    let partial = fixture_lines(include_str!("../../test-data/live-players/dst/partial.txt"));
    let malformed = fixture_lines(&format!(
        "[LGM-DST-PLAYERS-BEGIN]\t{REQUEST_ID}\n\
         [LGM-DST-PLAYER]\t{REQUEST_ID}\t1\tKU_bad\tmissing columns\n\
         [LGM-DST-PLAYERS-END]\t{REQUEST_ID}\t0\n"
    ));
    let mismatched = fixture_lines(&format!(
        "[LGM-DST-PLAYERS-BEGIN]\t{REQUEST_ID}\n\
         [LGM-DST-PLAYER]\t{REQUEST_ID}\t1\tKU_valid\tPlayer\twilson\t0\n\
         [LGM-DST-PLAYERS-END]\t{REQUEST_ID}\t0\n"
    ));

    for lines in [&partial, &malformed, &mismatched] {
        let DstCaptureOutcome::Incomplete(capture) = parse(lines) else {
            panic!("invalid capture must remain incomplete");
        };
        assert_eq!(capture.status, RuntimeLivePlayerStatus::Failed);
        assert!(!capture.complete);
        assert_eq!(capture.current_players, None);
        assert!(capture.unique_count >= capture.stored_count);
        assert_eq!(capture.stored_count, capture.players.len());
        assert!(capture.players.iter().all(|player| {
            player.entry.available_action_ids.is_empty() && player.action_bindings.is_empty()
        }));
    }
}

#[test]
fn dst_client_table_rejects_a_second_begin_inside_the_same_correlated_capture() {
    let lines = fixture_lines(&format!(
        "[LGM-DST-PLAYERS-BEGIN]\t{REQUEST_ID}\n\
         [LGM-DST-PLAYER]\t{REQUEST_ID}\t1\tKU_valid\tPlayer\twilson\t0\n\
         [LGM-DST-PLAYERS-BEGIN]\t{REQUEST_ID}\n\
         [LGM-DST-PLAYERS-END]\t{REQUEST_ID}\t0\n"
    ));

    let DstCaptureOutcome::Incomplete(capture) = parse(&lines) else {
        panic!("a nested matching BEGIN must invalidate the capture");
    };
    assert!(!capture.complete);
    assert_eq!(capture.current_players, None);
}

#[test]
fn dst_client_table_sanitizes_display_fields_without_truncating_stable_identifiers() {
    let display = format!("A\u{0007}{}终", "界".repeat(MAX_FIELD_CHARS + 8));
    let valid = fixture_lines(&format!(
        "[LGM-DST-PLAYERS-BEGIN]\t{REQUEST_ID}\n\
         [LGM-DST-PLAYER]\t{REQUEST_ID}\t1\tKU_valid\t{display}\t{}\t0\n\
         [LGM-DST-PLAYERS-END]\t{REQUEST_ID}\t1\n",
        "w".repeat(MAX_FIELD_CHARS + 9)
    ));
    let DstCaptureOutcome::Complete(capture) = parse(&valid) else {
        panic!("overlong display-only fields remain valid");
    };
    assert_eq!(
        capture.players[0].entry.display_name.chars().count(),
        MAX_FIELD_CHARS
    );
    assert!(!capture.players[0].entry.display_name.contains('\u{0007}'));
    assert_eq!(
        capture.players[0].entry.attributes[0].value.chars().count(),
        MAX_FIELD_CHARS
    );

    let overlong_id = format!("KU_{}", "x".repeat(MAX_FIELD_CHARS));
    let invalid = fixture_lines(&format!(
        "[LGM-DST-PLAYERS-BEGIN]\t{REQUEST_ID}\n\
         [LGM-DST-PLAYER]\t{REQUEST_ID}\t1\t{overlong_id}\tPlayer\twilson\t0\n\
         [LGM-DST-PLAYERS-END]\t{REQUEST_ID}\t1\n"
    ));
    let DstCaptureOutcome::Incomplete(capture) = parse(&invalid) else {
        panic!("an overlong canonical ID must invalidate the capture");
    };
    assert_eq!(capture.raw_valid_count, 0);
    assert!(capture.players.is_empty());
}

#[test]
fn dst_client_table_distinguishes_entry_cap_from_byte_and_line_exhaustion() {
    let mut capped = vec![format!("[LGM-DST-PLAYERS-BEGIN]\t{REQUEST_ID}")];
    for index in 1..=(MAX_PLAYER_ENTRIES + 2) {
        capped.push(format!(
            "[LGM-DST-PLAYER]\t{REQUEST_ID}\t{index}\tKU_{index}\tP{index}\twilson\t0"
        ));
    }
    capped.push(format!(
        "[LGM-DST-PLAYERS-END]\t{REQUEST_ID}\t{}",
        MAX_PLAYER_ENTRIES + 2
    ));
    let DstCaptureOutcome::Complete(capture) = parse(&capped) else {
        panic!("entry cap still permits count validation through END");
    };
    assert!(capture.complete);
    assert!(capture.truncated);
    assert_eq!(capture.raw_valid_count, MAX_PLAYER_ENTRIES + 2);
    assert_eq!(capture.unique_count, MAX_PLAYER_ENTRIES + 2);
    assert_eq!(capture.stored_count, MAX_PLAYER_ENTRIES);
    assert!(capture.players.iter().all(|player| {
        player.entry.available_action_ids.is_empty() && player.action_bindings.is_empty()
    }));

    let overlong_line = vec![
        format!("[LGM-DST-PLAYERS-BEGIN]\t{REQUEST_ID}"),
        "x".repeat(MAX_LINE_BYTES + 1),
        format!("[LGM-DST-PLAYERS-END]\t{REQUEST_ID}\t0"),
    ];
    let DstCaptureOutcome::Incomplete(line_limited) = parse(&overlong_line) else {
        panic!("line exhaustion before END must be incomplete");
    };
    assert!(line_limited.truncated);

    let mut byte_limited = vec![format!("[LGM-DST-PLAYERS-BEGIN]\t{REQUEST_ID}")];
    let repeated_row = format!(
        "[LGM-DST-PLAYER]\t{REQUEST_ID}\t1\tKU_same\t{}\twilson\t0",
        "p".repeat(MAX_FIELD_CHARS)
    );
    while byte_limited
        .iter()
        .map(|line| line.len() + 1)
        .sum::<usize>()
        <= MAX_CAPTURE_BYTES
    {
        byte_limited.push(repeated_row.clone());
    }
    byte_limited.push(format!("[LGM-DST-PLAYERS-END]\t{REQUEST_ID}\t1"));
    let DstCaptureOutcome::Incomplete(byte_limited) = parse(&byte_limited) else {
        panic!("byte exhaustion before END must be incomplete");
    };
    assert!(byte_limited.truncated);
    assert_eq!(byte_limited.current_players, None);
}

#[test]
fn dst_client_table_returns_no_capture_without_a_matching_begin_marker() {
    let lines = fixture_lines(
        "[LGM-DST-PLAYERS-BEGIN]\tffffffffffffffffffffffffffffffff\n\
         [LGM-DST-PLAYERS-END]\tffffffffffffffffffffffffffffffff\t0\n\
         name contains [LGM-DST-PLAYERS-BEGIN] but has no exact token layout\n",
    );

    assert!(matches!(parse(&lines), DstCaptureOutcome::NoCapture));
}

#[test]
fn dst_client_table_runtime_log_stream_bounded_reader_honors_budget_and_pending_cap() {
    let root = std::env::temp_dir().join(format!(
        "langame-dst-bounded-reader-{}",
        uuid::Uuid::new_v4().simple()
    ));
    fs::create_dir_all(&root).expect("create temp fixture root");
    let path = root.join("server.log");
    fs::write(&path, "one\ntwo\nthree\n").expect("write log");

    let mut state = RuntimeLogTailState::default();
    let first = read_runtime_log_delta_bounded(&path, &mut state, 8, 16).expect("bounded read");
    assert_eq!(first.lines, ["one", "two"]);
    assert_eq!(first.bytes_read, 8);
    assert_eq!(state.byte_offset, 8);
    assert!(first.limit_exhausted);

    fs::write(&path, "abcdefghijk").expect("replace with long pending line");
    let mut state = RuntimeLogTailState::default();
    let pending =
        read_runtime_log_delta_bounded(&path, &mut state, 8, 5).expect("bounded pending read");
    assert!(state.pending_text.len() <= 5);
    assert!(pending.limit_exhausted);

    fs::write(&path, "12345678").expect("write exact-budget log");
    let mut state = RuntimeLogTailState::default();
    let exact_budget =
        read_runtime_log_delta_bounded(&path, &mut state, 8, 16).expect("read exact caller budget");
    assert!(
        exact_budget.limit_exhausted,
        "consuming the full budget must conservatively report exhaustion"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn dst_client_table_runtime_log_stream_bounded_reader_preserves_utf8_across_reads() {
    let root = std::env::temp_dir().join(format!(
        "langame-dst-utf8-reader-{}",
        uuid::Uuid::new_v4().simple()
    ));
    fs::create_dir_all(&root).expect("create temp fixture root");
    let path = root.join("server.log");
    fs::write(&path, "玩家\n").expect("write UTF-8 log");

    let mut state = RuntimeLogTailState::default();
    let first =
        read_runtime_log_delta_bounded(&path, &mut state, 2, 16).expect("first partial codepoint");
    assert!(first.lines.is_empty());
    let second =
        read_runtime_log_delta_bounded(&path, &mut state, 16, 16).expect("complete codepoint");
    assert_eq!(second.lines, ["玩家"]);

    let _ = fs::remove_dir_all(root);
}
