use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

fn normal() -> Duration {
    Duration::from_secs(180)
}
fn generation() -> Observation {
    Observation {
        generating: true,
        ..Observation::default()
    }
}
fn initialized() -> Observation {
    Observation {
        initialized: true,
        ..Observation::default()
    }
}
fn parse(bytes: &[u8]) -> Observation {
    let mut result = Observation::default();
    ConsoleCursor::default().consume(bytes, &mut result);
    result
}

#[test]
fn pinned_preloader_messages_require_their_source_level_and_complete_line() {
    for line in [
        "[Info   :InteropManager] Detected outdated interop assemblies, will regenerate them now\r\n",
        "[Message:InteropManager] Downloading unity base libraries https://example.invalid/unity.zip\n",
        "[Message:InteropManager] Reading unity base libraries from file file:///unity.zip\n",
        "[Message:InteropManager] Extracting downloaded unity base libraries\n",
        "[Message:InteropManager] Running Cpp2IL to generate dummy assemblies from global-metadata.dat\n",
        "[Info   :InteropManager] Generating interop assemblies\n",
    ] {
        assert_eq!(parse(line.as_bytes()), generation());
    }
    for line in [
        "[Message:OtherPlugin] Running Cpp2IL to generate dummy assemblies from global-metadata.dat\n",
        "[Warning:InteropManager] Generating interop assemblies\n",
        "[Info:Cpp2IL] Generating interop assemblies\n",
        "[Info:InteropManager] Generating interop assemblies",
        "[Info:InteropManager] Generating interop assemblies (not really)\n",
        "[LGSM] V Rising BepInEx is generating IL2CPP interop assemblies for this run.\n",
    ] {
        assert_eq!(parse(line.as_bytes()), Observation::default());
    }
}

#[test]
fn complete_chainloader_is_only_initialization_evidence_and_failures_are_explicit() {
    assert_eq!(
        parse(b"[Message: BepInEx] Chainloader initialized\n"),
        initialized()
    );
    assert_eq!(
        parse(b"[Message: BepInEx] Chainloader startup complete\n"),
        initialized()
    );
    assert_eq!(
        parse(b"[Message: Other] Chainloader initialized\n"),
        Observation::default()
    );
    let failed = parse(
        b"[Error:InteropManager] Failed to generate Il2Cpp interop assemblies: network failure\n",
    );
    assert!(failed.failed);
    assert!(
        PhaseClock::new(normal())
            .observe(&failed, Duration::from_secs(2))
            .unwrap_err()
            .contains("check disk space")
    );
}

#[test]
fn silent_preloader_and_old_hot_start_keep_normal_deadline() {
    let mut clock = PhaseClock::new(normal());
    clock
        .observe(&initialized(), Duration::from_secs(1))
        .unwrap();
    clock
        .observe(&generation(), Duration::from_secs(2))
        .unwrap();
    assert_eq!(clock.deadline(), normal());
    assert!(clock.check(normal()).is_err());
    let mut clock = PhaseClock::new(normal());
    assert!(clock.observe(&generation(), normal()).is_err());
    assert!(
        !clock.generating,
        "late generation cannot revive an expired start"
    );
}

#[test]
fn current_generation_gets_a_fixed_bound_without_sliding_on_repeated_output() {
    let mut clock = PhaseClock::new(normal());
    clock
        .observe(&generation(), Duration::from_secs(2))
        .unwrap();
    assert_eq!(clock.deadline(), INITIALIZATION_LIMIT);
    clock
        .observe(&generation(), Duration::from_secs(800))
        .unwrap();
    assert_eq!(clock.deadline(), INITIALIZATION_LIMIT);
    assert!(clock.check(Duration::from_secs(899)).is_ok());
    assert!(clock.observe(&initialized(), INITIALIZATION_LIMIT).is_err());
    assert!(!clock.initialized);
}

#[test]
fn initialization_restores_binding_budget_without_claiming_listener_ready() {
    let mut clock = PhaseClock::new(normal());
    clock
        .observe(&generation(), Duration::from_secs(2))
        .unwrap();
    clock
        .observe(&initialized(), Duration::from_secs(800))
        .unwrap();
    assert_eq!(clock.deadline(), Duration::from_secs(980));
    clock
        .observe(&initialized(), Duration::from_secs(900))
        .unwrap();
    assert_eq!(clock.deadline(), Duration::from_secs(980));
    assert!(
        clock
            .check(Duration::from_secs(980))
            .unwrap_err()
            .contains("owned game listener")
    );
    assert!(clock.deadline() <= INITIALIZATION_LIMIT + normal());
}

#[test]
fn split_rows_are_assembled_and_overlong_rows_cannot_inject_a_marker_suffix() {
    let mut cursor = ConsoleCursor::default();
    let mut result = Observation::default();
    cursor.consume(
        b"[Message:InteropManager] Running Cpp2IL to generate ",
        &mut result,
    );
    assert_eq!(result, Observation::default());
    cursor.consume(b"dummy assemblies from metadata\n", &mut result);
    assert_eq!(result, generation());
    result = Observation::default();
    cursor.consume(&vec![b'x'; MAX_LINE_BYTES + 1], &mut result);
    assert!(cursor.pending.len() <= MAX_LINE_BYTES);
    cursor.consume(
        b"[Info:InteropManager] Generating interop assemblies\n",
        &mut result,
    );
    assert_eq!(result, Observation::default());
    cursor.consume(
        b"[Info:InteropManager] Generating interop assemblies\n",
        &mut result,
    );
    assert_eq!(result, generation());
}

struct LogFixture(PathBuf);
impl LogFixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "lgsm-vrising-startup-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("managed-console")).unwrap();
        Self(root)
    }
    fn path(&self) -> PathBuf {
        self.0.join("managed-console/run-current.log")
    }
}
impl Drop for LogFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn only_appends_after_pre_spawn_baseline_are_current_run_evidence() {
    let fixture = LogFixture::new();
    let path = fixture.path();
    let writer = ManagedConsoleLog::open(&path).unwrap();
    writer.write_all(b"[Info:InteropManager] Generating interop assemblies\n[Message:BepInEx] Chainloader initialized\n").unwrap();
    let parts = open_log_segments(&path).unwrap().unwrap();
    let part = parts.last().unwrap();
    let mut cursor = ConsoleCursor {
        offset: part.start_offset + part.file.metadata().unwrap().len(),
        ..ConsoleCursor::default()
    };
    assert_eq!(cursor.read(&path).unwrap(), Observation::default());
    writer.write_all(b"[Message:InteropManager] Running Cpp2IL to generate dummy assemblies from current-metadata\n").unwrap();
    assert_eq!(cursor.read(&path).unwrap(), generation());
    assert_eq!(cursor.read(&path).unwrap(), Observation::default());
    // BepInEx's previous-run disk log is deliberately never opened.
    fs::write(
        fixture.0.join("LogOutput.log"),
        b"[Message:BepInEx] Chainloader initialized\n",
    )
    .unwrap();
    assert_eq!(cursor.read(&path).unwrap(), Observation::default());
    drop(parts);
    drop(writer);
}

#[test]
fn normal_console_rotation_preserves_the_absolute_evidence_cursor() {
    let fixture = LogFixture::new();
    let path = fixture.path();
    let writer = ManagedConsoleLog::open(&path).unwrap();
    writer.write_all(&vec![b'\n'; 8 * 1024 * 1024 - 8]).unwrap();
    let mut cursor = ConsoleCursor {
        offset: 8 * 1024 * 1024 - 8,
        ..ConsoleCursor::default()
    };
    writer
        .write_all(b"\n\n\n\n\n\n\n\n[Info:InteropManager] Generating interop assemblies\n")
        .unwrap();
    assert_eq!(cursor.read(&path).unwrap(), generation());
    drop(writer);
}
