#![cfg(test)]

use super::*;
use crate::commands::commands_install_progress::verification_install_progress;

fn package_update(bytes: u64, files: u64) -> InstallProgressUpdate {
    verification_install_progress(InstallVerificationProgress {
        stage: InstallVerificationStage::PackageFiles,
        completed_bytes: bytes,
        total_bytes: 4 * 1024 * 1024 * 1024,
        completed_files: files,
        total_files: 8,
    })
}

#[test]
fn verification_console_uses_measured_bytes_and_files_instead_of_repeating_the_stage() {
    let mut log = PrestartProgressLog::default();
    let update = package_update(1024 * 1024 * 1024, 1);
    let line = log.update(&update).durable.remove(0);
    assert_eq!(
        line,
        "正在确认原版程序清单… 25.0% · 1.0 GiB / 4.0 GiB · 1 / 8 个文件"
    );
}

#[test]
fn verification_replaces_each_measurement_without_appending_progress_history() {
    let mut log = PrestartProgressLog::default();
    assert_eq!(log.update(&package_update(0, 0)).durable.len(), 1);
    for millis in [250, 500, 750] {
        let output = log.update(&package_update(millis * 1024 * 1024, 1));
        assert!(output.durable.is_empty());
        assert!(output.preview.is_some());
    }
    let current = package_update(1024 * 1024 * 1024, 2);
    let line = log.update(&current).preview.unwrap();
    assert!(line.contains("25.0% · 1.0 GiB / 4.0 GiB · 2 / 8 个文件"));
    let repeat = log.update(&current);
    assert!(repeat.durable.is_empty());
    assert_eq!(repeat.preview.as_deref(), Some(line.as_str()));
    assert_eq!(log.finish().durable, vec![line]);
}

#[test]
fn final_byte_chunk_does_not_commit_until_the_last_file_and_completion_is_durable() {
    let mut log = PrestartProgressLog::default();
    assert_eq!(log.update(&package_update(0, 0)).durable.len(), 1);
    let incomplete = log.update(&package_update(4 * 1024 * 1024 * 1024, 7));
    assert!(incomplete.durable.is_empty());
    assert!(incomplete.preview.unwrap().ends_with("7 / 8 个文件"));
    let complete = package_update(4 * 1024 * 1024 * 1024, 8);
    assert!(
        log.update(&complete)
            .durable
            .remove(0)
            .ends_with("100.0% · 4.0 GiB / 4.0 GiB · 8 / 8 个文件")
    );
    assert!(log.update(&complete).durable.is_empty());
    assert!(log.finish().durable.is_empty());
}

#[test]
fn stage_changes_are_immediate_and_saving_does_not_reuse_verification_percentage() {
    let mut log = PrestartProgressLog::default();
    let mut snapshot = package_update(1024 * 1024 * 1024, 1)
        .install_progress
        .unwrap()
        .verification
        .unwrap();
    snapshot.stage = InstallVerificationStage::SteamFiles;
    let steam = verification_install_progress(snapshot.clone());
    assert!(log.update(&steam).durable.remove(0).contains("25.0%"));
    assert_eq!(log.update(&package_update(0, 0)).durable.len(), 1);
    assert!(
        log.update(&package_update(1024 * 1024 * 1024, 1))
            .preview
            .is_some()
    );
    snapshot.stage = InstallVerificationStage::RecordingBaseline;
    let saving = verification_install_progress(snapshot);
    let output = log.update(&saving);
    assert_eq!(output.durable.len(), 2);
    assert!(output.durable[0].contains("25.0%"));
    let line = &output.durable[1];
    assert_eq!(line, "正在保存原版程序清单… · 1 / 8 个文件");
    assert!(!line.contains('%'));
    assert!(!line.contains("GiB"));
}

#[test]
fn unknown_inventory_waits_for_real_totals_and_empty_files_use_file_progress() {
    let mut log = PrestartProgressLog::default();
    let mut snapshot = InstallVerificationProgress {
        stage: InstallVerificationStage::PackageFiles,
        completed_bytes: 0,
        total_bytes: 0,
        completed_files: 0,
        total_files: 0,
    };
    let unknown = verification_install_progress(snapshot.clone());
    assert_eq!(log.update(&unknown).durable, ["正在确认原版程序清单…"]);
    assert!(log.update(&unknown).durable.is_empty());
    snapshot.total_files = 4;
    snapshot.completed_files = 1;
    let measured = verification_install_progress(snapshot.clone());
    assert_eq!(
        log.update(&measured).durable.remove(0),
        "正在确认原版程序清单… 25.0% · 1 / 4 个文件"
    );
    snapshot.completed_files = 4;
    assert!(
        log.update(&verification_install_progress(snapshot))
            .durable
            .remove(0)
            .ends_with("100.0% · 4 / 4 个文件")
    );
}

#[test]
fn ordinary_installer_diagnostics_are_preserved_without_throttling_or_estimated_progress() {
    let mut log = PrestartProgressLog::default();
    assert_eq!(log.update(&package_update(0, 0)).durable.len(), 1);
    for detail in [
        "Connecting to Steam...",
        "Downloading...",
        "Retrying installation...",
    ] {
        let mut update = InstallProgressUpdate::stage(InstallPhase::Preparing, detail);
        update.progress_percent = 90.0;
        assert_eq!(log.update(&update).durable, [detail]);
        assert_eq!(log.update(&update).durable, [detail]);
    }
}

#[test]
fn native_projection_does_not_duplicate_raw_records_in_durable_history() {
    let mut log = PrestartProgressLog::default();
    let mut update =
        InstallProgressUpdate::stage(InstallPhase::Preparing, "Waiting for network...");
    update.console_output = Some(InstallConsoleOutput::Progress {
        retain_boundaries: false,
        preview: true,
    });
    for _ in 0..3 {
        let output = log.update(&update);
        assert!(output.durable.is_empty());
        assert_eq!(output.preview.as_deref(), Some("Waiting for network..."));
    }
    assert!(log.finish().durable.is_empty());
}

#[test]
fn measured_progress_reaches_the_startup_console_file_with_its_origin_prefix() {
    let path = std::env::temp_dir().join(format!(
        "lgsm-prestart-progress-{}.log",
        uuid::Uuid::new_v4().simple()
    ));
    let mut console = super::super::console::PrestartConsole::open(path.to_str().unwrap()).unwrap();
    for (bytes, files) in [(0, 0), (1024 * 1024 * 1024, 1), (1024 * 1024 * 1024, 1)] {
        let update = package_update(bytes, files);
        console.update(&update).unwrap();
    }
    assert_eq!(std::fs::read_to_string(&path).unwrap().lines().count(), 1);
    assert!(
        app_runtime::with_live_console_preview(&path, |rows| rows[0].contains("25.0%")).unwrap()
    );
    console.finish().unwrap();
    assert!(app_runtime::with_live_console_preview(&path, |rows| rows.is_empty()).unwrap());
    let content = std::fs::read_to_string(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert_eq!(content.lines().count(), 2, "{content}");
    assert!(
        content
            .lines()
            .all(|line| line.starts_with("[LanGame startup update] ")),
        "{content}"
    );
    assert!(content.contains("25.0% · 1.0 GiB / 4.0 GiB · 1 / 8 个文件"));
}
