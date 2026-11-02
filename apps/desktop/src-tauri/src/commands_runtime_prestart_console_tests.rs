use super::*;

struct Fixture(std::path::PathBuf);

impl Fixture {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "lgsm-prestart-console-{}.log",
            uuid::Uuid::new_v4().simple()
        )))
    }
    fn open(&self) -> PrestartConsole {
        PrestartConsole::open(self.0.to_str().unwrap()).unwrap()
    }
    fn text(&self) -> String {
        std::fs::read_to_string(&self.0).unwrap()
    }
    fn preview(&self) -> Vec<String> {
        app_runtime::with_live_console_preview(&self.0, <[String]>::to_vec).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn raw_streams_keep_duplicates_blank_lines_and_split_unicode_without_interleaving() {
    let fixture = Fixture::new();
    let mut console = fixture.open();
    let name = "玩家".as_bytes();
    console
        .raw(1, InstallOutputSource::Stdout, 0, &name[..4], false)
        .unwrap();
    assert_eq!(fixture.preview(), ["玩"]);
    console
        .raw(2, InstallOutputSource::Stderr, 0, b"same\nsame\n\n", false)
        .unwrap();
    assert_eq!(fixture.text(), "same\nsame\n\n");
    let mut rest = name[4..].to_vec();
    rest.push(b'\n');
    console
        .raw(1, InstallOutputSource::Stdout, 4, &rest, false)
        .unwrap();
    console
        .raw(1, InstallOutputSource::Stdout, 7, &[], true)
        .unwrap();
    console
        .raw(2, InstallOutputSource::Stderr, 11, &[], true)
        .unwrap();
    console.finish().unwrap();
    assert_eq!(fixture.text(), "same\nsame\n\n玩家\n");
    assert!(fixture.preview().is_empty());
}

#[test]
fn raw_partial_is_replaced_until_eof_and_a_long_record_is_not_excerpt_truncated() {
    let fixture = Fixture::new();
    let mut console = fixture.open();
    let bytes = vec![b'x'; 12 * 1024];
    for (index, chunk) in bytes.chunks(4096).enumerate() {
        console
            .raw(
                1,
                InstallOutputSource::Stdout,
                (index * 4096) as u64,
                chunk,
                false,
            )
            .unwrap();
        assert_eq!(fixture.preview()[0].len(), (index + 1) * 4096);
        assert!(fixture.text().is_empty());
    }
    console
        .raw(
            1,
            InstallOutputSource::Stdout,
            bytes.len() as u64,
            &[],
            true,
        )
        .unwrap();
    console.finish().unwrap();
    assert_eq!(fixture.text().as_bytes(), [bytes, vec![b'\n']].concat());
}

#[test]
fn byte_gaps_and_replays_fail_instead_of_text_deduplicating() {
    for wrong_offset in [0, 8] {
        let fixture = Fixture::new();
        let mut console = fixture.open();
        console
            .raw(1, InstallOutputSource::Stdout, 0, b"same\n", false)
            .unwrap();
        assert!(
            console
                .raw(
                    1,
                    InstallOutputSource::Stdout,
                    wrong_offset,
                    b"same\n",
                    false
                )
                .is_err()
        );
        console.finish().unwrap();
        assert_eq!(fixture.text(), "same\n");
    }
}

#[test]
fn crlf_split_and_unterminated_error_are_committed_before_owner_release() {
    let fixture = Fixture::new();
    let mut console = fixture.open();
    console
        .raw(1, InstallOutputSource::Stdout, 0, b"one\r", false)
        .unwrap();
    assert_eq!(fixture.preview(), ["one"]);
    console
        .raw(1, InstallOutputSource::Stdout, 4, b"\ntail", false)
        .unwrap();
    assert_eq!(fixture.text(), "one\r\n");
    assert_eq!(fixture.preview(), ["tail"]);
    console.finish().unwrap();
    assert_eq!(fixture.text(), "one\r\ntail\n");
    assert!(fixture.preview().is_empty());
}

#[test]
fn oversized_unfinished_output_is_an_explicit_error() {
    let fixture = Fixture::new();
    let mut console = fixture.open();
    let error = console
        .raw(
            1,
            InstallOutputSource::Stdout,
            0,
            &vec![b'x'; MAX_PENDING_BYTES + 1],
            false,
        )
        .unwrap_err();
    assert!(error.to_string().contains("unfinished records"));
    // Error cleanup still retains the bytes already received.
    console.finish().unwrap();
    assert_eq!(fixture.text().len(), MAX_PENDING_BYTES + 2);
}

#[test]
fn oversized_chunk_commits_complete_records_and_retains_the_unfinished_tail() {
    for finish_explicitly in [false, true] {
        let fixture = Fixture::new();
        let mut console = fixture.open();
        let mut bytes = b"complete\ncomplete\n".to_vec();
        bytes.extend(vec![b'x'; MAX_PENDING_BYTES + 1]);
        let error = console
            .raw(1, InstallOutputSource::Stdout, 0, &bytes, false)
            .unwrap_err();
        assert!(error.to_string().contains("unfinished records"));
        assert_eq!(fixture.text(), "complete\ncomplete\n");
        if finish_explicitly {
            console.finish().unwrap();
        } else {
            drop(console);
        }
        bytes.push(b'\n');
        assert_eq!(std::fs::read(&fixture.0).unwrap(), bytes);
    }
}

#[test]
fn rejected_preview_retains_uncommitted_records_for_finish() {
    let fixture = Fixture::new();
    let mut console = fixture.open();
    console.progress_preview = Some("invalid\npreview".into());
    assert!(console.publish(b"complete\ncomplete\n".to_vec()).is_err());
    assert!(fixture.text().is_empty());
    assert_eq!(console.uncommitted, b"complete\ncomplete\n");
    console.finish().unwrap();
    assert_eq!(fixture.text(), "complete\ncomplete\n");
}

#[test]
fn dropping_an_interrupted_owner_commits_its_received_tail_and_releases_preview() {
    let fixture = Fixture::new();
    let mut console = fixture.open();
    console
        .raw(
            1,
            InstallOutputSource::Stderr,
            0,
            "最后诊断".as_bytes(),
            false,
        )
        .unwrap();
    assert_eq!(fixture.preview(), ["最后诊断"]);
    drop(console);
    assert_eq!(fixture.text(), "最后诊断\n");
    assert!(fixture.preview().is_empty());
}

#[test]
fn parsed_pipe_status_does_not_duplicate_the_real_partial_or_committed_record() {
    let fixture = Fixture::new();
    let mut console = fixture.open();
    console
        .raw(1, InstallOutputSource::Stdout, 0, b"Waiting...", false)
        .unwrap();
    let mut status = InstallProgressUpdate::stage(app_core::InstallPhase::Preparing, "Waiting...");
    status.console_output = Some(InstallConsoleOutput::Progress {
        retain_boundaries: false,
        preview: false,
    });
    console.update(&status).unwrap();
    assert_eq!(fixture.preview(), ["Waiting..."]);
    console
        .raw(1, InstallOutputSource::Stdout, 10, b"\n", true)
        .unwrap();
    console.update(&status).unwrap();
    assert!(fixture.preview().is_empty());
    console.finish().unwrap();
    assert_eq!(fixture.text(), "Waiting...\n");
}
