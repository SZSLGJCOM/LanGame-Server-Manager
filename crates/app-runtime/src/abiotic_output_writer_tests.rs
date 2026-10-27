use super::*;
use std::sync::{Arc, Mutex};

const TOKEN: &str = "synthetic-modio-token-12345";

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn captured(capture: &Capture) -> String {
    String::from_utf8(capture.0.lock().unwrap().clone()).unwrap()
}

#[test]
fn every_token_split_is_hidden_and_readiness_and_save_bytes_are_unchanged() {
    for split in 1..TOKEN.len() {
        let capture = Capture::default();
        {
            let mut writer = ModioTokenWriter::new(capture.clone(), vec![TOKEN.into()]);
            writer.write_all(b"Session creation completed\n").unwrap();
            assert_eq!(captured(&capture), "Session creation completed\n");
            writer.write_all(&TOKEN.as_bytes()[..split]).unwrap();
            writer.flush().unwrap();
            assert_eq!(captured(&capture), "Session creation completed\n");
            writer.write_all(&TOKEN.as_bytes()[split..]).unwrap();
            writer.write_all(b"\nSaved world successfully\n").unwrap();
        }
        assert_eq!(
            captured(&capture),
            "Session creation completed\n[REDACTED]\nSaved world successfully\n"
        );
    }
}

#[test]
fn byte_sized_writes_and_multiple_credentials_remain_hidden() {
    let capture = Capture::default();
    {
        let mut writer = ModioTokenWriter::new(
            capture.clone(),
            vec![TOKEN.into(), format!("{TOKEN}-longer")],
        );
        let input = format!("玩家: {TOKEN}-longer\n-ModioToken={TOKEN}\n");
        for byte in input.as_bytes() {
            writer.write_all(&[*byte]).unwrap();
        }
    }
    assert_eq!(
        captured(&capture),
        "玩家: [REDACTED]\n-ModioToken=[REDACTED]\n"
    );
}

#[test]
fn flush_keeps_a_possible_prefix_private_and_drop_masks_it_at_eof() {
    let capture = Capture::default();
    {
        let mut writer = ModioTokenWriter::new(capture.clone(), vec![TOKEN.into()]);
        writer.write_all(b"ordinary output\nsynthetic-mod").unwrap();
        writer.flush().unwrap();
        assert_eq!(captured(&capture), "ordinary output\n");
    }
    assert_eq!(captured(&capture), "ordinary output\n[REDACTED]");
}

struct BrokenSink {
    fail_on_flush: bool,
}

impl Write for BrokenSink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.fail_on_flush {
            Ok(bytes.len())
        } else {
            Err(io::Error::other("synthetic persistence failure"))
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::other("synthetic flush failure"))
    }
}

#[test]
fn write_and_flush_failures_are_returned_and_raw_retries_are_rejected() {
    let mut writer = ModioTokenWriter::new(
        BrokenSink {
            fail_on_flush: false,
        },
        vec![TOKEN.into()],
    );
    assert!(writer.write_all(TOKEN.as_bytes()).is_err());
    assert!(writer.write_all(TOKEN.as_bytes()).is_err());
    assert!(writer.flush().is_err());
    let mut writer = ModioTokenWriter::new(
        BrokenSink {
            fail_on_flush: true,
        },
        vec![TOKEN.into()],
    );
    writer.write_all(b"ordinary output").unwrap();
    assert!(writer.flush().is_err());
    assert!(writer.write_all(TOKEN.as_bytes()).is_err());
}
