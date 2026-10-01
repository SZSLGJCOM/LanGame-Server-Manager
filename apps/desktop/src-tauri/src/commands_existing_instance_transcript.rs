//! Bounded comparison of retained bytes and delivered lines. Console contents
//! stay in memory; receipts contain only aggregate counts and digests.
use serde::Serialize;
use sha2::{Digest, Sha256};

const MAX_LINE_BYTES: usize = 1024 * 1024;

#[derive(Clone, Default, Debug, PartialEq, Eq, Serialize)]
pub(super) struct Signature {
    pub sha256: String,
    pub lines: u64,
    pub game_lines: u64,
    pub line_limit_exceeded: bool,
}

#[derive(Default)]
pub(super) struct Accumulator {
    digest: Sha256,
    lines: u64,
    game_lines: u64,
    pending: Vec<u8>,
    skip_lf: bool,
    line_limit_exceeded: bool,
}

impl Accumulator {
    pub(super) fn bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            if self.skip_lf {
                self.skip_lf = false;
                if *byte == b'\n' {
                    continue;
                }
            }
            if matches!(*byte, b'\r' | b'\n') {
                self.end_line();
                self.skip_lf = *byte == b'\r';
            } else if self.pending.len() < MAX_LINE_BYTES {
                self.pending.push(*byte);
            } else {
                self.line_limit_exceeded = true;
            }
        }
    }

    pub(super) fn line(&mut self, line: &str) {
        self.digest.update((line.len() as u64).to_le_bytes());
        self.digest.update(line.as_bytes());
        self.lines += 1;
        self.game_lines += u64::from(super::evidence::game_line(line));
    }

    fn end_line(&mut self) {
        let bytes = std::mem::take(&mut self.pending);
        self.line(&String::from_utf8_lossy(&bytes));
        self.pending = bytes;
        self.pending.clear();
    }

    pub(super) fn snapshot(&self) -> Signature {
        Signature {
            sha256: self
                .digest
                .clone()
                .finalize()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
            lines: self.lines,
            game_lines: self.game_lines,
            line_limit_exceeded: self.line_limit_exceeded,
        }
    }

    pub(super) fn finish(mut self) -> Signature {
        if !self.pending.is_empty() {
            self.end_line();
        }
        self.snapshot()
    }
}

#[test]
fn existing_acceptance_transcript_requires_middle_lines_and_final_partial() {
    let mut file = Accumulator::default();
    file.bytes(b"first\r\nmiddle\rfinal ");
    file.bytes("尾行".as_bytes());
    let expected = file.finish();
    let mut delivered = Accumulator::default();
    delivered.line("first");
    assert_ne!(
        delivered.snapshot(),
        expected,
        "an EOF byte offset alone proves nothing"
    );
    delivered.line("middle");
    assert_ne!(
        delivered.snapshot(),
        expected,
        "the unterminated final line is required"
    );
    delivered.line("final 尾行");
    assert_eq!(delivered.snapshot(), expected);
    let mut dropped = Accumulator::default();
    dropped.line("first");
    dropped.line("final 尾行");
    assert_ne!(
        dropped.snapshot(),
        expected,
        "a later offset cannot hide a dropped middle event"
    );
}

#[test]
fn existing_acceptance_transcript_normalizes_split_crlf_and_invalid_utf8() {
    let mut file = Accumulator::default();
    file.bytes(b"\r");
    file.bytes(b"\n\xfftail");
    let mut delivered = Accumulator::default();
    delivered.line("");
    delivered.line("\u{fffd}tail");
    assert_eq!(file.finish(), delivered.snapshot());
}
