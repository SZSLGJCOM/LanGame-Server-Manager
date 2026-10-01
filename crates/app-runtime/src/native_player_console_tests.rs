use super::frame::{Capture, Screen};
use super::*;

const NONCE: &str = "1234567890abcdef1234567890abcdef";

fn screen(lines: &[&str]) -> Screen {
    Screen {
        width: 120,
        height: 9001,
        cursor_x: 2,
        rows: lines
            .iter()
            .map(|line| {
                let mut row: Vec<_> = line.encode_utf16().collect();
                row.resize(120, u16::from(b' '));
                row
            })
            .collect(),
    }
}

fn captured_lines() -> Vec<String> {
    vec![
        format!("Unknown command \"LGM_PLAYER_QUERY_BEGIN_{NONCE}\"!"),
        "".into(),
        "> players".into(),
        "Players: 0/8".into(),
        "".into(),
        format!("> LGM_PLAYER_QUERY_END_{NONCE}"),
        format!("Unknown command \"LGM_PLAYER_QUERY_END_{NONCE}\"!"),
        "> ".into(),
    ]
}

fn as_screen(lines: &[String]) -> Screen {
    screen(&lines.iter().map(String::as_str).collect::<Vec<_>>())
}

#[test]
fn native_console_accepts_complete_fresh_frame_only() {
    let mut capture = Capture::new(NONCE, &screen(&["> "])).unwrap();
    let lines = captured_lines();
    assert!(capture.observe(&as_screen(&lines[..4])).unwrap().is_none());
    let text = capture.observe(&as_screen(&lines)).unwrap().unwrap();
    assert!(text.starts_with("Unknown command"));
    assert!(text.ends_with(&format!("END_{NONCE}\"!\n")));
}

#[test]
fn native_console_end_marker_on_active_cursor_row_is_incomplete() {
    let mut capture = Capture::new(NONCE, &screen(&["> "])).unwrap();
    let mut lines = captured_lines();
    lines.pop();
    assert!(capture.observe(&as_screen(&lines)).unwrap().is_none());
}

#[test]
fn native_console_rejects_replayed_nonce_before_input() {
    assert!(Capture::new(NONCE, &as_screen(&captured_lines())).is_err());
}

#[test]
fn native_console_refuses_wrapped_rows_resizes_and_invalid_utf16() {
    for variant in 0..3 {
        let mut capture = Capture::new(NONCE, &screen(&["> "])).unwrap();
        let mut current = as_screen(&captured_lines());
        match variant {
            0 => current.rows[3] = vec![u16::from(b'x'); 120],
            1 => current.width = 121,
            _ => current.rows[3][0] = 0xd800,
        }
        assert_eq!(
            capture.observe(&current),
            Err(NativePlayerConsoleError::Incomplete)
        );
    }
}

#[test]
fn native_console_preserves_prefix_across_physical_scrollback_shift() {
    let mut capture = Capture::new(NONCE, &screen(&["> "])).unwrap();
    let lines = captured_lines();
    let mut shifted = vec!["old history".into()];
    shifted.extend_from_slice(&lines[..4]);
    assert!(capture.observe(&as_screen(&shifted)).unwrap().is_none());
    assert!(capture.observe(&as_screen(&lines)).unwrap().is_some());
}

#[test]
fn native_console_rejects_rewrites_rollback_and_truncated_begin() {
    for variant in 0..3 {
        let mut capture = Capture::new(NONCE, &screen(&["> "])).unwrap();
        let mut lines = captured_lines();
        assert!(capture.observe(&as_screen(&lines[..5])).unwrap().is_none());
        match variant {
            0 => lines[3] = "Players: 1/8".into(),
            1 => lines.truncate(3),
            _ => {
                lines.remove(0);
            }
        }
        assert_eq!(
            capture.observe(&as_screen(&lines)),
            Err(NativePlayerConsoleError::Incomplete)
        );
    }
}

#[test]
fn native_console_rejects_arbitrary_commands_processes_and_invalid_identity() {
    let mut identity = ProcessIdentity {
        creation_time: 123,
        image_path: "C:/owned/MoriaServer-Win64-Shipping.exe".into(),
    };
    assert!(valid_request(123, &identity, NONCE));
    assert!(!valid_request(0, &identity, NONCE));
    assert!(!valid_request(123, &identity, "nonce\rexit"));
    identity.creation_time = 0;
    assert!(!valid_request(123, &identity, NONCE));
    identity.creation_time = 123;
    identity.image_path = "C:/Windows/cmd.exe".into();
    assert!(!valid_request(123, &identity, NONCE));
}
