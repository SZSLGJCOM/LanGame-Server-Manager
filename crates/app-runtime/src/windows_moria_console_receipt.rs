//! Fresh native command response, separate from input delivery and process exit.
use super::*;

const TAIL_ROWS: usize = 96;
const MAX_PROGRESS_ROWS: usize = 64;
const LINE_AND_ECHO_INPUT: u32 = 0x0002 | 0x0004;
const EXIT_ECHO: &str = "> exit";
const EXIT_RESPONSES: [&str; 2] = [
    "Stopping the session (this might take a while)...",
    "Stopping the server...",
];

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetConsoleMode(handle: *mut c_void, mode: *mut u32) -> i32;
}

// Never format this bounded snapshot: earlier native output can contain join
// credentials. Rows stay local to this helper and are used only for alignment.
#[derive(Clone, PartialEq, Eq)]
pub(super) struct Prompt {
    size: Coord,
    cursor: Coord,
    input_mode: u32,
    rows: Vec<Vec<u16>>,
}

impl Prompt {
    pub(super) fn follows_empty_read(&self, previous: &Self) -> bool {
        self.unique_progress(previous, |rows| rows.iter().all(|row| row_is(row, "> "))) == Some(1)
    }

    fn has_exit_response_after(&self, previous: &Self) -> bool {
        self.unique_progress(previous, |rows| {
            rows.first().is_some_and(|row| row_is(row, EXIT_ECHO))
                && rows
                    .iter()
                    .skip(1)
                    .any(|row| EXIT_RESPONSES.iter().any(|response| row_is(row, response)))
        })
        .is_some()
    }

    fn valid_empty(&self) -> bool {
        (8..=512).contains(&self.size.x)
            && self.size.y > 0
            && self.cursor.x == 2
            && (0..self.size.y).contains(&self.cursor.y)
            && self.input_mode & LINE_AND_ECHO_INPUT == LINE_AND_ECHO_INPUT
            && self.rows.len() == (self.cursor.y as usize + 1).min(TAIL_ROWS)
            && self
                .rows
                .iter()
                .all(|row| row.len() == self.size.x as usize)
            && self.rows.last().is_some_and(|row| row_is(row, "> "))
    }

    fn first_row(&self) -> i32 {
        i32::from(self.cursor.y) + 1 - self.rows.len() as i32
    }

    fn unique_progress(
        &self,
        previous: &Self,
        accepts: impl Fn(&[Vec<u16>]) -> bool,
    ) -> Option<usize> {
        if !self.valid_empty()
            || !previous.valid_empty()
            || self == previous
            || self.size != previous.size
            || self.input_mode != previous.input_mode
        {
            return None;
        }
        let mut progress = None;
        for advance in 1..=MAX_PROGRESS_ROWS {
            let Some(start) = self.aligned_output_start(previous, advance) else {
                continue;
            };
            if accepts(&self.rows[start..self.rows.len() - 1]) {
                if progress.is_some() {
                    return None;
                }
                progress = Some(advance);
            }
        }
        progress
    }

    fn aligned_output_start(&self, previous: &Self, advance: usize) -> Option<usize> {
        let moved = i32::from(self.cursor.y) - i32::from(previous.cursor.y);
        let scrolled = advance as i32 - moved;
        if moved < 0 || scrolled < 0 || (scrolled > 0 && self.cursor.y != self.size.y - 1) {
            return None;
        }
        let first_output = i32::from(self.cursor.y) - advance as i32;
        if first_output < self.first_row() {
            return None;
        }
        let overlap_start = previous.first_row().max(self.first_row() + scrolled);
        // A scroll needs a retained, unchanged prefix. Losing it, or finding
        // multiple valid alignments in repeated output, is not fresh evidence.
        if scrolled > 0 && overlap_start >= i32::from(previous.cursor.y) {
            return None;
        }
        // The old prompt itself is excluded: typing exit overwrites that row.
        for row in overlap_start..i32::from(previous.cursor.y) {
            let old = &previous.rows[(row - previous.first_row()) as usize];
            let new = &self.rows[(row - scrolled - self.first_row()) as usize];
            if old != new {
                return None;
            }
        }
        Some((first_output - self.first_row()) as usize)
    }
}

fn row_is(row: &[u16], expected: &str) -> bool {
    row.len() >= expected.len()
        && row[..expected.len()]
            .iter()
            .copied()
            .eq(expected.bytes().map(u16::from))
        && row[expected.len()..]
            .iter()
            .all(|cell| *cell == u16::from(b' '))
}

pub(super) fn require_empty_prompt(
    pid: u32,
    input: &Handle,
    output: &Handle,
) -> Result<Prompt, RuntimeProcessError> {
    stable_prompt(pid, input, output)?.ok_or_else(|| {
        error(
            pid,
            "Moria console has pending input or no stable empty prompt",
        )
    })
}

pub(super) fn exit_response_received(
    pid: u32,
    input: &Handle,
    output: &Handle,
    before: &Prompt,
) -> Result<Option<Prompt>, RuntimeProcessError> {
    Ok(stable_prompt(pid, input, output)?.filter(|prompt| prompt.has_exit_response_after(before)))
}

pub(super) fn stable_prompt(
    pid: u32,
    input: &Handle,
    output: &Handle,
) -> Result<Option<Prompt>, RuntimeProcessError> {
    let Some(first) = capture_prompt(pid, input, output)? else {
        return Ok(None);
    };
    // At the buffer bottom a scroll leaves the cursor unchanged. Require two
    // identical bounded reads as well as stable geometry and an empty queue.
    Ok(capture_prompt(pid, input, output)?.filter(|second| second == &first))
}

fn capture_prompt(
    pid: u32,
    input: &Handle,
    output: &Handle,
) -> Result<Option<Prompt>, RuntimeProcessError> {
    let mut pending = 0;
    let mut input_mode = 0;
    let mut before = BufferInfo::default();
    if unsafe { GetNumberOfConsoleInputEvents(input.0, &mut pending) } == 0
        || unsafe { GetConsoleMode(input.0, &mut input_mode) } == 0
        || unsafe { GetConsoleScreenBufferInfo(output.0, &mut before) } == 0
    {
        return Err(observation_error(pid));
    }
    if pending != 0
        || input_mode & LINE_AND_ECHO_INPUT != LINE_AND_ECHO_INPUT
        || !(8..=512).contains(&before.size.x)
        || before.size.y <= 0
        || before.cursor.x != 2
        || !(0..before.size.y).contains(&before.cursor.y)
    {
        return Ok(None);
    }
    let count = (before.cursor.y as usize + 1).min(TAIL_ROWS);
    let width = before.size.x as usize;
    let mut cells = vec![0_u16; count * width];
    let mut read = 0;
    if unsafe {
        ReadConsoleOutputCharacterW(
            output.0,
            cells.as_mut_ptr(),
            cells.len() as u32,
            Coord {
                x: 0,
                y: before.cursor.y + 1 - count as i16,
            },
            &mut read,
        )
    } == 0
    {
        return Err(observation_error(pid));
    }
    if read as usize != cells.len() {
        return Err(error(pid, "Moria console prompt snapshot is incomplete"));
    }
    let mut after_mode = 0;
    let mut after = BufferInfo::default();
    if unsafe { GetNumberOfConsoleInputEvents(input.0, &mut pending) } == 0
        || unsafe { GetConsoleMode(input.0, &mut after_mode) } == 0
        || unsafe { GetConsoleScreenBufferInfo(output.0, &mut after) } == 0
    {
        return Err(observation_error(pid));
    }
    if pending != 0
        || input_mode != after_mode
        || before.cursor != after.cursor
        || before.size != after.size
    {
        return Ok(None);
    }
    let prompt = Prompt {
        size: before.size,
        cursor: before.cursor,
        input_mode,
        rows: cells.chunks_exact(width).map(<[u16]>::to_vec).collect(),
    };
    Ok(prompt.valid_empty().then_some(prompt))
}

fn observation_error(pid: u32) -> RuntimeProcessError {
    error(
        pid,
        format!(
            "Moria console prompt observation failed: {}",
            std::io::Error::last_os_error()
        ),
    )
}

#[cfg(test)]
#[path = "windows_moria_console_receipt_tests.rs"]
mod tests;
