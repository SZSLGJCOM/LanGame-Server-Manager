use super::*;
use std::io::{Seek, SeekFrom, Write};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("langame-native-handoff-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn path(&self) -> PathBuf {
        self.0.join("native.log")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn process() -> InstanceProcessState {
    InstanceProcessState {
        run_id: 1,
        session_id: Some("test".into()),
        process_key: "main".into(),
        display_name: "Main".into(),
        pid: Some(123),
        process_identity: Some(app_core::ProcessIdentity {
            creation_time: 134_353_044_000_000_000,
            image_path: "synthetic.exe".into(),
        }),
        status: "running".into(),
        started_at: None,
        stopped_at: None,
        exit_code: None,
        crash_flag: false,
        log_path: None,
        is_primary: true,
    }
}

const STARTUP: &str = "[2026.10.01-05.00.01:000][0] startup complete\n";

#[test]
fn cr_lf_split_at_handoff_drops_only_the_matching_lf() {
    let fixture = Fixture::new();
    let path = fixture.path();
    let startup = format!("{STARTUP}progress\r");
    fs::write(&path, &startup).unwrap();
    let mut file = fs::File::open(&path).unwrap();
    let boundary = NativeStartupSource::capture(
        "main".into(),
        &path,
        process().process_identity.unwrap().creation_time,
        &mut file,
        startup.len() as u64,
    )
    .unwrap();
    fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"\n\nactual\n")
        .unwrap();
    assert_eq!(read(&path, &boundary, &process()).lines, ["", "actual"]);
    drop(file);
}

fn checkpoint(path: &Path) -> NativeStartupSource {
    let mut file = fs::File::open(path).unwrap();
    file.seek(SeekFrom::Start(3)).unwrap();
    let boundary = NativeStartupSource::capture(
        "main".into(),
        path,
        process().process_identity.unwrap().creation_time,
        &mut file,
        STARTUP.len() as u64,
    )
    .unwrap();
    assert_eq!(
        file.stream_position().unwrap(),
        3,
        "Capturing a checkpoint must not alter the reader cursor"
    );
    boundary
}

fn read(
    path: &Path,
    boundary: &NativeStartupSource,
    process: &InstanceProcessState,
) -> LogTailSnapshot {
    read_game_log_snapshot_after_startup(
        path.to_string_lossy().into_owned(),
        process,
        400,
        Some(boundary),
    )
    .snapshot
}

#[test]
fn exact_native_handoff_preserves_undated_followup_and_real_repeated_rows() {
    let fixture = Fixture::new();
    let path = fixture.path();
    fs::write(&path, STARTUP).unwrap();
    let boundary = checkpoint(&path);
    fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"same\nsame\nundated followup\npartial")
        .unwrap();
    let snapshot = read(&path, &boundary, &process());
    assert!(snapshot.read_error.is_none());
    assert_eq!(
        snapshot.lines,
        ["same", "same", "undated followup", "partial"]
    );
    assert_eq!(snapshot.total_lines, 4);
    fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b" completed\n")
        .unwrap();
    assert_eq!(
        read(&path, &boundary, &process()).lines,
        ["same", "same", "undated followup", "partial completed"]
    );
}

#[test]
fn an_identical_replacement_file_does_not_inherit_the_previous_files_checkpoint() {
    let fixture = Fixture::new();
    let path = fixture.path();
    let content = format!("{STARTUP}same\nsame\n");
    fs::write(&path, &content).unwrap();
    let boundary = checkpoint(&path);
    fs::rename(&path, fixture.0.join("previous.log")).unwrap();
    fs::write(&path, &content).unwrap();
    let snapshot = read(&path, &boundary, &process());
    assert_eq!(snapshot.lines, [STARTUP.trim_end(), "same", "same"]);
}

#[test]
fn in_place_rewrite_and_a_different_process_cannot_reuse_the_startup_offset() {
    let fixture = Fixture::new();
    let path = fixture.path();
    fs::write(&path, format!("{STARTUP}after\n")).unwrap();
    let boundary = checkpoint(&path);
    let mut wrong_process = process();
    wrong_process
        .process_identity
        .as_mut()
        .unwrap()
        .creation_time += 1;
    assert_eq!(
        read(&path, &boundary, &wrong_process).lines,
        [STARTUP.trim_end(), "after"]
    );
    let rewritten = STARTUP.replace("startup complete", "rewrite complete");
    assert_eq!(rewritten.len(), STARTUP.len());
    fs::write(&path, format!("{rewritten}after\n")).unwrap();
    assert_eq!(
        read(&path, &boundary, &process()).lines,
        [rewritten.trim_end(), "after"]
    );
    fs::write(&path, "short\n").unwrap();
    let mut file = fs::File::open(&path).unwrap();
    assert!(
        boundary
            .validated_offset(
                &path,
                &mut file,
                process().process_identity.unwrap().creation_time
            )
            .unwrap()
            .is_none()
    );
}
