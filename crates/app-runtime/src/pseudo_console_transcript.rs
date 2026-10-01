use std::io;

/// Converts completed terminal rows into an append-only transcript. Cursor edits
/// are applied before a row is emitted; escape sequences never reach log parsers.
pub(super) struct TerminalTranscript {
    screen: Vec<Vec<char>>,
    row: usize,
    column: usize,
    saved_cursor: (usize, usize),
    escape: Escape,
    utf8: Vec<u8>,
    dirty: Vec<bool>,
}

enum Escape {
    None,
    Start,
    Csi(Vec<u8>),
    Osc { length: usize, escape: bool },
}

impl TerminalTranscript {
    pub(super) fn new(width: usize, height: usize) -> Self {
        Self {
            screen: vec![vec![' '; width]; height],
            row: 0,
            column: 0,
            saved_cursor: (0, 0),
            escape: Escape::None,
            utf8: Vec::with_capacity(4),
            dirty: vec![false; height],
        }
    }

    pub(super) fn push(&mut self, bytes: &[u8], output: &mut String) -> io::Result<()> {
        for &byte in bytes {
            if output.len() > 1024 * 1024 {
                return Err(invalid_terminal_sequence());
            }
            let state = std::mem::replace(&mut self.escape, Escape::None);
            match state {
                Escape::Start => match byte {
                    b'[' => self.escape = Escape::Csi(Vec::new()),
                    b']' => {
                        self.escape = Escape::Osc {
                            length: 0,
                            escape: false,
                        }
                    }
                    b'7' => self.saved_cursor = (self.row, self.column),
                    b'8' => (self.row, self.column) = self.saved_cursor,
                    b'D' => self.line_feed(output),
                    b'E' => {
                        self.column = 0;
                        self.line_feed(output);
                    }
                    _ => return Err(invalid_terminal_sequence()),
                },
                Escape::Csi(mut parameters) => {
                    if (0x40..=0x7e).contains(&byte) {
                        self.apply_csi(&parameters, byte)?;
                    } else if parameters.len() < 64 && (0x20..=0x3f).contains(&byte) {
                        parameters.push(byte);
                        self.escape = Escape::Csi(parameters);
                    } else {
                        return Err(invalid_terminal_sequence());
                    }
                }
                Escape::Osc { length, escape } => {
                    if byte == 7 || (escape && byte == b'\\') {
                        continue;
                    }
                    if length >= 4096 {
                        return Err(invalid_terminal_sequence());
                    }
                    self.escape = Escape::Osc {
                        length: length + 1,
                        escape: byte == 27,
                    };
                }
                Escape::None => {
                    if !self.utf8.is_empty() || byte >= 128 {
                        self.utf8.push(byte);
                        match std::str::from_utf8(&self.utf8) {
                            Ok(value) => {
                                let character =
                                    value.chars().next().ok_or_else(invalid_terminal_sequence)?;
                                self.utf8.clear();
                                self.put(character, output);
                            }
                            Err(error) if error.error_len().is_none() && self.utf8.len() < 4 => {}
                            Err(_) => return Err(invalid_terminal_sequence()),
                        }
                        continue;
                    }
                    match byte {
                        27 => self.escape = Escape::Start,
                        b'\r' => self.column = 0,
                        b'\n' => self.line_feed(output),
                        8 => self.column = self.column.saturating_sub(1),
                        b'\t' => self.column = ((self.column / 8 + 1) * 8).min(self.width() - 1),
                        0 | 7 => {}
                        32..=126 => self.put(byte as char, output),
                        _ => return Err(invalid_terminal_sequence()),
                    }
                }
            }
        }
        Ok(())
    }

    pub(super) fn finish(&mut self, output: &mut String) -> io::Result<()> {
        // Preserve text decoded before an incomplete final code point or escape.
        // Those bytes are already known; discarding them can hide the crash tail.
        self.drain_pending_rows(output);
        if !self.utf8.is_empty() || !matches!(self.escape, Escape::None) {
            return Err(invalid_terminal_sequence());
        }
        Ok(())
    }

    pub(super) fn drain_pending_rows(&mut self, output: &mut String) {
        for row in 0..self.screen.len() {
            self.emit_row(row, output);
        }
    }

    fn width(&self) -> usize {
        self.screen[0].len()
    }

    fn put(&mut self, character: char, output: &mut String) {
        if self.column >= self.width() {
            self.column = 0;
            self.line_feed(output);
        }
        self.screen[self.row][self.column] = character;
        self.dirty[self.row] = true;
        self.column += 1;
    }

    fn line_feed(&mut self, output: &mut String) {
        self.emit_row(self.row, output);
        if self.row + 1 < self.screen.len() {
            self.row += 1;
        } else {
            self.screen.rotate_left(1);
            self.dirty.rotate_left(1);
            self.dirty[self.row] = false;
            self.screen[self.row].fill(' ');
        }
    }

    fn emit_row(&mut self, row: usize, output: &mut String) {
        if !self.dirty[row] {
            return;
        }
        let line = self.screen[row].iter().collect::<String>();
        let line = line.trim_end_matches(' ');
        if !line.is_empty() {
            output.push_str(line);
            output.push('\n');
        }
        self.dirty[row] = false;
    }

    fn apply_csi(&mut self, bytes: &[u8], command: u8) -> io::Result<()> {
        // Color and cursor visibility do not affect the text transcript.
        if matches!(command, b'm' | b'h' | b'l') {
            return Ok(());
        }
        let text = std::str::from_utf8(bytes).map_err(|_| invalid_terminal_sequence())?;
        let values = text
            .split(';')
            .map(|value| {
                if value.is_empty() {
                    Ok(0)
                } else {
                    value.parse::<usize>()
                }
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| invalid_terminal_sequence())?;
        let first = values.first().copied().unwrap_or(0);
        let distance = first.max(1);
        match command {
            b'H' | b'f' => {
                self.row = distance.saturating_sub(1).min(self.screen.len() - 1);
                self.column = values
                    .get(1)
                    .copied()
                    .unwrap_or(1)
                    .max(1)
                    .saturating_sub(1)
                    .min(self.width() - 1);
            }
            b'G' => self.column = distance.saturating_sub(1).min(self.width() - 1),
            b'd' => self.row = distance.saturating_sub(1).min(self.screen.len() - 1),
            b'A' | b'F' => self.row = self.row.saturating_sub(distance),
            b'B' | b'E' => self.row = self.row.saturating_add(distance).min(self.screen.len() - 1),
            b'C' => self.column = self.column.saturating_add(distance).min(self.width() - 1),
            b'D' => self.column = self.column.saturating_sub(distance),
            b'K' => {
                let column = self.column.min(self.width() - 1);
                match first {
                    0 => self.screen[self.row][column..].fill(' '),
                    1 => self.screen[self.row][..=column].fill(' '),
                    2 => self.screen[self.row].fill(' '),
                    _ => return Err(invalid_terminal_sequence()),
                }
            }
            b'J' if first == 2 || first == 3 => {
                self.screen.iter_mut().for_each(|row| row.fill(' '))
            }
            b'J' if first == 0 => {
                let column = self.column.min(self.width() - 1);
                self.screen[self.row][column..].fill(' ');
                self.screen
                    .iter_mut()
                    .skip(self.row + 1)
                    .for_each(|row| row.fill(' '));
            }
            b'X' => {
                let start = self.column.min(self.width() - 1);
                let end = start.saturating_add(distance).min(self.width());
                self.screen[self.row][start..end].fill(' ');
            }
            b's' => self.saved_cursor = (self.row, self.column),
            b'u' => (self.row, self.column) = self.saved_cursor,
            _ => return Err(invalid_terminal_sequence()),
        }
        if matches!(command, b'E' | b'F') {
            self.column = 0;
        }
        Ok(())
    }
}

fn invalid_terminal_sequence() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "The terminal output contains an unsupported or incomplete control sequence.",
    )
}

#[cfg(test)]
#[path = "pseudo_console_transcript_tests.rs"]
mod tests;
