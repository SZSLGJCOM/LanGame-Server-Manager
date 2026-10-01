use super::*;

fn push(terminal: &mut TerminalTranscript, bytes: &[u8]) -> io::Result<String> {
    let mut output = String::new();
    terminal.push(bytes, &mut output)?;
    Ok(output)
}

fn finish(terminal: &mut TerminalTranscript) -> io::Result<String> {
    let mut output = String::new();
    terminal.finish(&mut output)?;
    Ok(output)
}

#[test]
fn barotrauma_empty_clientlist_has_explicit_boundaries_after_redraw() {
    // Sanitized control sequence shape from Barotrauma 1.13.4.0 on ConPTY.
    let mut terminal = TerminalTranscript::new(240, 80);
    let text = push(&mut terminal, b"\x1b[?9001h\x1b[?1004h\x1b[2J\x1b[m\x1b[H\x1b]0;server\x07\x1b[38;5;15mclientlist\x1b[229X\x1b[38;5;14m\r\n***************\x1b[224X\r\n***************\x1b[224X\x1b[38;5;8m\r\n                  \x1b[4;1H").unwrap();
    assert_eq!(text, "clientlist\n***************\n***************\n");
    assert_eq!(
        push(
            &mut terminal,
            b"\x1b[38;5;15mclientlist\x1b[K\r\n***************\r\n***************\r\n"
        )
        .unwrap(),
        text
    );
}

#[test]
fn unicode_split_across_reads_is_preserved_without_replacement() {
    let mut terminal = TerminalTranscript::new(240, 80);
    let bytes = "- 1: 玩家😀, endpoint, Steam_123, ping 10 ms\r\n".as_bytes();
    let mut output = String::new();
    for byte in bytes {
        output.push_str(&push(&mut terminal, &[*byte]).unwrap());
    }
    assert_eq!(output, "- 1: 玩家😀, endpoint, Steam_123, ping 10 ms\n");
    assert!(push(&mut terminal, &[0xff]).is_err());
}

#[test]
fn erase_and_cursor_edits_do_not_leak_autocomplete_or_reemit_untouched_history() {
    let mut terminal = TerminalTranscript::new(40, 8);
    assert_eq!(
        push(
            &mut terminal,
            b"clientlist autocomplete\rclientlist\x1b[K\r\n"
        )
        .unwrap(),
        "clientlist\n"
    );
    assert_eq!(push(&mut terminal, b"\x1b[1;1H\n").unwrap(), "");
    assert_eq!(
        push(&mut terminal, b"\x1b[1;1Hdone\x1b[K\r\n").unwrap(),
        "done\n"
    );
}

#[test]
fn malformed_and_unbounded_terminal_sequences_fail_closed() {
    for bytes in [
        b"\x1b[999999999999999999999999999H".as_slice(),
        b"\x1b[1q",
        b"\x1bZ",
        b"\xc0\xaf",
    ] {
        assert!(push(&mut TerminalTranscript::new(40, 8), bytes).is_err());
    }
    let mut osc = b"\x1b]".to_vec();
    osc.extend(vec![b'x'; 4097]);
    assert!(push(&mut TerminalTranscript::new(40, 8), &osc).is_err());
}

#[test]
fn final_transcript_emits_pending_edits_once_without_repeating_completed_rows() {
    let mut terminal = TerminalTranscript::new(40, 8);
    assert_eq!(push(&mut terminal, b"first\r\npartial").unwrap(), "first\n");
    push(&mut terminal, b"\rfinal\x1b[K").unwrap();
    assert_eq!(finish(&mut terminal).unwrap(), "final\n");
    assert!(finish(&mut terminal).unwrap().is_empty());
}

#[test]
fn final_transcript_rejects_incomplete_utf8_and_control_sequences() {
    for bytes in [b"\xe7\x8e".as_slice(), b"\x1b[", b"\x1b]title"] {
        let mut terminal = TerminalTranscript::new(40, 8);
        push(&mut terminal, bytes).unwrap();
        assert_eq!(
            finish(&mut terminal).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
}
