use super::super::tests::{Fixture, hashes, payload};
use super::*;

fn interrupted(fixture: &Fixture, bytes: &[u8], index: usize) -> BTreeMap<String, String> {
    let mut next = payload(bytes);
    let wanted = hashes(&next);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        publish(&fixture.0, &ASE, &mut next, &wanted, |current| {
            if current == index {
                panic!("simulated process interruption before publication");
            }
            Ok(())
        })
        .unwrap();
    }));
    assert!(result.is_err());
    assert!(fixture.bin().join(PENDING).is_dir());
    wanted
}

#[test]
fn ark_tools_install_recovers_first_install_after_process_interruption() {
    let fixture = Fixture::new();
    let wanted = interrupted(&fixture, b"first plugin", 2);
    let error = ensure_can_start(&fixture.0).unwrap_err();
    assert!(error.contains("ARK") && error.contains("interrupted"));
    recover(&fixture.bin(), &ASE, &wanted).unwrap();
    recover(&fixture.bin(), &ASE, &wanted).unwrap();
    assert_eq!(fs::read_dir(fixture.bin()).unwrap().count(), 1);
    let mut next = payload(b"first plugin");
    publish(&fixture.0, &ASE, &mut next, &wanted, |_| Ok(())).unwrap();
    ensure_can_start(&fixture.0).unwrap();
    fixture.finish();
}

#[test]
fn ark_tools_install_recovers_upgrade_and_an_interrupted_rollback() {
    let fixture = Fixture::new();
    let mut initial = payload(b"old plugin");
    let old = hashes(&initial);
    publish(&fixture.0, &ASE, &mut initial, &old, |_| Ok(())).unwrap();
    let old_manifest = fs::read(fixture.bin().join(OWNER)).unwrap();
    let wanted = interrupted(&fixture, b"new plugin", 1);
    let directory = fixture.bin().join(PENDING);
    let ValidatedJournal {
        journal,
        expected_hashes: after,
        ..
    } = load(&directory, &ASE, &wanted).unwrap();
    // The prior process had already restored this file before it exited. A
    // second recovery must accept old bytes as well as the new/missing state.
    let index = order(&after).iter().position(|p| p == PLUGIN).unwrap();
    files::rollback_file(
        &fixture.bin().join(PLUGIN),
        &directory.join(format!("{index}.old")),
        Some(b"old plugin"),
        b"new plugin",
    )
    .unwrap();
    assert!(journal.old_owner.is_some());
    recover(&fixture.bin(), &ASE, &wanted).unwrap();
    assert_eq!(fs::read(fixture.bin().join(PLUGIN)).unwrap(), b"old plugin");
    assert_eq!(fs::read(fixture.bin().join(OWNER)).unwrap(), old_manifest);
    let mut next = payload(b"new plugin");
    publish(&fixture.0, &ASE, &mut next, &wanted, |_| Ok(())).unwrap();
    assert_eq!(fs::read(fixture.bin().join(PLUGIN)).unwrap(), b"new plugin");
    fixture.finish();
}

#[test]
fn ark_tools_install_recovers_partial_staging_without_runtime_changes() {
    let fixture = Fixture::new();
    let wanted = interrupted(&fixture, b"first plugin", 0);
    fs::rename(fixture.bin().join(PENDING), fixture.bin().join(STAGING)).unwrap();
    fs::remove_file(fixture.bin().join(STAGING).join("1.new")).unwrap();
    recover(&fixture.bin(), &ASE, &wanted).unwrap();
    assert_eq!(fs::read_dir(fixture.bin()).unwrap().count(), 1);
    fixture.finish();
}

#[test]
fn ark_tools_install_recovery_preserves_changed_runtime_and_transaction_files() {
    for change in ["runtime", "backup", "unexpected"] {
        let fixture = Fixture::new();
        let wanted = interrupted(&fixture, b"first plugin", 2);
        let directory = fixture.bin().join(PENDING);
        let path = match change {
            "runtime" => fixture.bin().join("config.json"),
            "backup" => directory.join("2.new"),
            _ => directory.join("operator-notes.txt"),
        };
        fs::write(&path, b"external changes must survive").unwrap();
        assert!(recover(&fixture.bin(), &ASE, &wanted).is_err(), "{change}");
        assert_eq!(fs::read(path).unwrap(), b"external changes must survive");
        assert!(directory.join(JOURNAL).is_file());
        fixture.finish();
    }
}

#[test]
fn ark_tools_install_recovery_rejects_changed_file_sets_and_backup_ownership() {
    for change in ["extra", "hash", "directory"] {
        let fixture = Fixture::new();
        let wanted = interrupted(&fixture, b"first plugin", 0);
        let path = fixture.bin().join(PENDING).join(JOURNAL);
        let mut journal: Journal = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        match change {
            "extra" => {
                journal.before.insert("operator.txt".into(), None);
            }
            "hash" => {
                journal
                    .before
                    .insert(PLUGIN.into(), Some(digest(b"unowned")));
            }
            _ => journal.created_directories.push("../outside".into()),
        }
        fs::write(&path, serde_json::to_vec(&journal).unwrap()).unwrap();
        assert!(recover(&fixture.bin(), &ASE, &wanted).is_err(), "{change}");
        assert!(!fixture.bin().join(OWNER).exists());
        assert!(path.is_file());
        fixture.finish();
    }
}

#[test]
fn ark_tools_install_cleanup_is_reentrant_after_backup_and_journal_removal() {
    for state in [COMMITTED, ROLLED_BACK] {
        let fixture = Fixture::new();
        let wanted = interrupted(&fixture, b"first plugin", 0);
        let directory = fixture.bin().join(PENDING);
        let ValidatedJournal {
            journal,
            serialized_bytes: bytes,
            expected_hashes: after,
        } = load(&directory, &ASE, &wanted).unwrap();
        if state == COMMITTED {
            for (index, path) in order(&after).iter().enumerate() {
                let mut pins = Vec::new();
                let mut created = Vec::new();
                files::prepare_parent(&fixture.bin(), path, &mut pins, &mut created).unwrap();
                fs::hard_link(
                    directory.join(format!("{index}.new")),
                    fixture.bin().join(path),
                )
                .unwrap();
            }
        }
        let cleanup_directory = fixture.bin().join(state);
        fs::rename(directory, &cleanup_directory).unwrap();
        // The previous cleanup deleted one numbered file, then the process
        // exited. Remaining files still have to satisfy the journal hashes.
        let expected = transaction_files(&journal, &after, &bytes);
        files::remove_hash(&cleanup_directory.join("0.new"), &expected["0.new"]).unwrap();
        recover(&fixture.bin(), &ASE, &wanted).unwrap();
        assert!(!cleanup_directory.exists());
        fs::create_dir(&cleanup_directory).unwrap();
        recover(&fixture.bin(), &ASE, &wanted).unwrap();
        assert!(!cleanup_directory.exists());
        fixture.finish();
    }
}

#[test]
fn ark_tools_install_start_gate_accepts_unextended_runtime_and_blocks_all_transaction_states() {
    let fixture = Fixture::new();
    ensure_can_start(&fixture.0).unwrap();
    for name in [STAGING, PENDING, COMMITTED, ROLLED_BACK] {
        let path = fixture.bin().join(name);
        fs::create_dir(&path).unwrap();
        assert!(
            ensure_can_start(&fixture.0)
                .unwrap_err()
                .contains("interrupted")
        );
        fs::remove_dir(&path).unwrap();
        fs::write(&path, b"invalid marker type").unwrap();
        assert!(ensure_can_start(&fixture.0).is_err());
        fs::remove_file(path).unwrap();
    }
    fixture.finish();
}

#[cfg(windows)]
#[test]
fn ark_tools_install_verified_deletion_keeps_the_same_exclusive_windows_handle() {
    let fixture = Fixture::new();
    let path = fixture.bin().join("owned.dll");
    fs::write(&path, b"verified bytes").unwrap();
    let file = files::lock_regular(&path, true).unwrap().unwrap();
    assert_eq!(file.bytes, b"verified bytes");
    assert!(fs::write(&path, b"racing writer").is_err());
    assert!(fs::rename(&path, fixture.bin().join("swapped.dll")).is_err());
    file.remove().unwrap();
    assert!(!path.exists());
    fixture.finish();
}

#[cfg(windows)]
#[test]
fn ark_tools_install_recovery_rejects_a_junction_transaction_without_touching_target() {
    use std::os::windows::process::CommandExt;
    let fixture = Fixture::new();
    let wanted = interrupted(&fixture, b"first plugin", 0);
    let link = fixture.bin().join(PENDING);
    let target = fixture.0.join("retained-transaction");
    fs::rename(&link, &target).unwrap();
    let journal = fs::read(target.join(JOURNAL)).unwrap();
    let result = std::process::Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(link.to_string_lossy().replace('/', "\\"))
        .arg(target.to_string_lossy().replace('/', "\\"))
        .creation_flags(0x0800_0000)
        .output()
        .unwrap();
    assert!(result.status.success(), "{result:?}");
    let result = recover(&fixture.bin(), &ASE, &wanted);
    fs::remove_dir(&link).unwrap();
    assert!(result.is_err());
    assert_eq!(fs::read(target.join(JOURNAL)).unwrap(), journal);
    assert!(!fixture.bin().join(OWNER).exists());
    fixture.finish();
}
