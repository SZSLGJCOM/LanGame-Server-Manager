use super::*;

// Model ordinary console writes/newlines over a full buffer independently of
// the receipt alignment algorithm, including scrolling one row at the bottom.
struct Console {
    rows: Vec<String>,
    cursor: usize,
}

impl Console {
    fn at(height: usize, cursor: usize) -> Self {
        let mut rows = (0..height)
            .map(|row| format!("earlier output {row}"))
            .collect::<Vec<_>>();
        rows[cursor] = "> ".into();
        Self { rows, cursor }
    }

    fn newline(&mut self) {
        if self.cursor + 1 < self.rows.len() {
            self.cursor += 1;
        } else {
            self.rows.remove(0);
            self.rows.push(String::new());
        }
        self.rows[self.cursor].clear();
    }

    fn command_response(&mut self, command: &str, response: &str) {
        self.rows[self.cursor] = format!("> {command}");
        self.newline();
        self.rows[self.cursor] = response.into();
        self.newline();
        self.rows[self.cursor] = "> ".into();
    }

    fn empty_read(&mut self) {
        self.newline();
        self.rows[self.cursor] = "> ".into();
    }

    fn snapshot(&self) -> Prompt {
        let start = (self.cursor + 1).saturating_sub(TAIL_ROWS);
        let rows = self.rows[start..=self.cursor]
            .iter()
            .map(|line| {
                let mut row = line.encode_utf16().collect::<Vec<_>>();
                assert!(row.len() <= 120);
                row.resize(120, u16::from(b' '));
                row
            })
            .collect();
        Prompt {
            size: Coord {
                x: 120,
                y: self.rows.len() as i16,
            },
            cursor: Coord {
                x: 2,
                y: self.cursor as i16,
            },
            input_mode: LINE_AND_ECHO_INPUT,
            rows,
        }
    }
}

#[test]
fn receipt_requires_this_exit_echo_before_its_native_response() {
    for response in EXIT_RESPONSES {
        let mut console = Console::at(128, 30);
        let before = console.snapshot();
        console.command_response("exit", response);
        let executed = console.snapshot();
        assert!(executed.has_exit_response_after(&before));
        assert!(!executed.follows_empty_read(&before));
        console.empty_read();
        assert!(console.snapshot().follows_empty_read(&executed));
    }
    for (command, response) in [
        ("players", EXIT_RESPONSES[0]),
        ("exit", "Error: Stopping the server..."),
        ("exit", ""),
    ] {
        let mut console = Console::at(128, 30);
        let before = console.snapshot();
        console.command_response(command, response);
        assert!(!console.snapshot().has_exit_response_after(&before));
    }
    let mut console = Console::at(128, 30);
    let before = console.snapshot();
    console.rows[30] = EXIT_RESPONSES[0].into();
    console.newline();
    console.rows[31] = EXIT_ECHO.into();
    console.empty_read();
    assert!(!console.snapshot().has_exit_response_after(&before));
}

#[test]
fn receipt_aligns_exit_when_the_initial_prompt_is_overwritten_then_scrolled() {
    for cursor in [6, 7] {
        let mut console = Console::at(8, cursor);
        let before = console.snapshot();
        console.command_response("exit", EXIT_RESPONSES[0]);
        let executed = console.snapshot();
        assert_eq!(executed.cursor.y, 7);
        assert!(executed.has_exit_response_after(&before));
        console.empty_read();
        assert!(console.snapshot().follows_empty_read(&executed));
    }
}

#[test]
fn receipt_tracks_all_32_empty_reads_at_the_buffer_bottom() {
    let mut console = Console::at(9001, 9000);
    let before = console.snapshot();
    assert_eq!(before.rows.len(), TAIL_ROWS);
    console.command_response("exit", EXIT_RESPONSES[0]);
    let mut previous = console.snapshot();
    assert!(previous.has_exit_response_after(&before));
    for _ in 0..32 {
        console.empty_read();
        let current = console.snapshot();
        assert_eq!(current.cursor.y, previous.cursor.y);
        assert!(current.follows_empty_read(&previous));
        assert!(!previous.follows_empty_read(&current));
        assert!(!current.follows_empty_read(&current));
        previous = current;
    }
}

#[test]
fn receipt_rejects_stale_responses_and_unobserved_extra_reads() {
    let mut console = Console::at(128, 30);
    console.command_response("exit", EXIT_RESPONSES[0]);
    let before = console.snapshot();
    assert!(!before.has_exit_response_after(&before));
    assert!(!before.follows_empty_read(&before));
    console.empty_read();
    assert!(!console.snapshot().has_exit_response_after(&before));
    console.empty_read();
    assert!(!console.snapshot().follows_empty_read(&before));
}

#[test]
fn receipt_rejects_ambiguous_scrolled_prompt_and_command_histories() {
    let mut console = Console::at(16, 15);
    console.rows.fill("> ".into());
    console.rows[0] = "last distinct row".into();
    let before = console.snapshot();
    console.empty_read();
    let after = console.snapshot();
    assert!(after != before);
    assert!(!after.follows_empty_read(&before));

    let mut console = Console::at(13, 12);
    for row in 0..12 {
        console.rows[row] = if row % 2 == 0 {
            EXIT_ECHO
        } else {
            EXIT_RESPONSES[0]
        }
        .into();
    }
    console.rows[0] = "last distinct row".into();
    let before = console.snapshot();
    console.command_response("exit", EXIT_RESPONSES[0]);
    let after = console.snapshot();
    assert!(after != before);
    assert!(!after.has_exit_response_after(&before));
}

#[test]
fn receipt_rejects_nonempty_prompt_changed_geometry_and_missing_echo() {
    let mut console = Console::at(128, 30);
    let before = console.snapshot();
    console.command_response("exit", EXIT_RESPONSES[0]);
    let complete = console.snapshot();
    for line in ["x ", "> pending", ">  hidden"] {
        console.rows[console.cursor] = line.into();
        let nonempty = console.snapshot();
        assert!(!nonempty.valid_empty());
        assert!(!nonempty.has_exit_response_after(&before));
    }
    let mut resized = complete.clone();
    resized.size.y += 1;
    assert!(!resized.has_exit_response_after(&before));
    let mut no_echo = complete.clone();
    no_echo.input_mode &= !0x0004;
    assert!(!no_echo.valid_empty());
    assert!(!no_echo.has_exit_response_after(&before));
    let mut typing = complete;
    typing.cursor.x = 3;
    assert!(!typing.valid_empty());
    assert!(!typing.has_exit_response_after(&before));
}

#[test]
fn receipt_rejects_progress_after_all_alignment_evidence_is_lost() {
    let mut console = Console::at(2, 1);
    let before = console.snapshot();
    console.command_response("exit", EXIT_RESPONSES[0]);
    assert!(!console.snapshot().has_exit_response_after(&before));

    let mut console = Console::at(200, 199);
    let before = console.snapshot();
    console.rows[199] = EXIT_ECHO.into();
    for _ in 0..TAIL_ROWS {
        console.newline();
    }
    console.rows[199] = EXIT_RESPONSES[0].into();
    console.empty_read();
    assert!(!console.snapshot().has_exit_response_after(&before));
}
