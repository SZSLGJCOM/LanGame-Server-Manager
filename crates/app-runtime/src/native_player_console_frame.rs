use super::{MAX_RESPONSE_BYTES, NativePlayerConsoleError as Error};

/// Full physical rows through the cursor's row; the final row is still being
/// written and cannot be accepted as a complete response delimiter.
pub(super) struct Screen {
    pub width: usize,
    pub height: usize,
    pub cursor_x: usize,
    pub rows: Vec<Vec<u16>>,
}

pub(super) struct Capture {
    begin: String,
    end: String,
    width: usize,
    height: usize,
    prefix: Vec<String>,
    started: bool,
}

impl Capture {
    pub fn new(nonce: &str, screen: &Screen) -> Result<Self, Error> {
        let result = Self {
            begin: format!("Unknown command \"LGM_PLAYER_QUERY_BEGIN_{nonce}\"!"),
            end: format!("Unknown command \"LGM_PLAYER_QUERY_END_{nonce}\"!"),
            width: screen.width,
            height: screen.height,
            prefix: Vec::new(),
            started: false,
        };
        if screen.width < 80 || screen.width > 512 || screen.height < 16 {
            return Err(Error::CaptureLimit);
        }
        if screen
            .rows
            .iter()
            .any(|row| String::from_utf16_lossy(row).contains(nonce))
        {
            return Err(Error::Incomplete);
        }
        Ok(result)
    }

    pub fn observe(&mut self, screen: &Screen) -> Result<Option<String>, Error> {
        if screen.width != self.width
            || screen.height != self.height
            || screen.rows.is_empty()
            || screen.cursor_x >= screen.width
        {
            return Err(Error::Incomplete);
        }
        let mut lines = Vec::with_capacity(screen.rows.len());
        for row in screen.rows.iter().take(screen.rows.len() - 1) {
            if row.len() != screen.width {
                return Err(Error::Incomplete);
            }
            lines.push(
                String::from_utf16(row)
                    .map_err(|_| Error::Incomplete)?
                    .trim_end_matches(' ')
                    .to_owned(),
            );
        }
        let matches: Vec<_> = lines
            .iter()
            .enumerate()
            .filter(|(_, line)| *line == &self.begin)
            .map(|(index, _)| index)
            .collect();
        let start = match matches.as_slice() {
            [] if !self.started => return Ok(None),
            [start] => *start,
            _ => return Err(Error::Incomplete),
        };
        self.started = true;
        let candidate = &lines[start..];
        if !candidate.starts_with(&self.prefix) {
            return Err(Error::Incomplete);
        }
        let end = candidate.iter().position(|line| line == &self.end);
        let observed = &candidate[..end.map_or(candidate.len(), |end| end + 1)];
        // A full physical line may have wrapped. Moria has no escaping or
        // continuation marker, so accepting it could split one identity into two.
        if observed
            .iter()
            .any(|line| line.encode_utf16().count() >= self.width)
        {
            return Err(Error::Incomplete);
        }
        let size = observed.iter().map(|line| line.len() + 1).sum::<usize>();
        if size > MAX_RESPONSE_BYTES || observed.len() > 256 {
            return Err(Error::CaptureLimit);
        }
        self.prefix = observed.to_vec();
        Ok(end.map(|_| format!("{}\n", observed.join("\n"))))
    }
}
