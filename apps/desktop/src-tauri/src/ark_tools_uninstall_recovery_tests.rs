use super::*;

fn interrupted(fixture: &Fixture, wanted: &BTreeMap<String, String>, index: usize) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        remove_with(&fixture.0, &ASE, wanted, |current| {
            if current == index {
                panic!("simulated process interruption after removal");
            }
            Ok(())
        })
        .unwrap();
    }));
    assert!(result.is_err());
    assert!(fixture.bin().join(PENDING).is_dir());
}

#[test]
fn ark_tools_uninstall_recovers_interruption_before_install_or_another_removal() {
    for retry in ["install", "remove"] {
        let fixture = Fixture::new();
        let content = installed(&fixture);
        let wanted = hashes(&content);
        interrupted(&fixture, &wanted, 1);
        assert!(
            ensure_can_start(&fixture.0)
                .unwrap_err()
                .contains("interrupted")
        );
        let status = inspect(&fixture.0, ASE.module_id, embedded()).unwrap();
        assert!(!status.ready);
        assert!(status.can_uninstall);
        if retry == "install" {
            publish(&fixture.0, &ASE, content.clone(), &wanted).unwrap();
            assert_content(&fixture, &content);
        } else {
            remove(&fixture.0, &ASE, &wanted).unwrap();
            for path in wanted.keys() {
                assert!(!fixture.bin().join(path).exists());
            }
        }
        assert!(!pending(&fixture.bin()).unwrap());
        ensure_can_start(&fixture.0).unwrap();
        fixture.finish();
    }
}

#[test]
fn ark_tools_uninstall_recovers_an_interrupted_rollback_idempotently() {
    let fixture = Fixture::new();
    let content = installed(&fixture);
    let wanted = hashes(&content);
    interrupted(&fixture, &wanted, 1);
    let first = wanted.keys().next().unwrap();
    fs::hard_link(
        fixture.bin().join(PENDING).join("0.old"),
        fixture.bin().join(first),
    )
    .unwrap();
    recovery::recover(&fixture.bin(), &ASE, &wanted).unwrap();
    recovery::recover(&fixture.bin(), &ASE, &wanted).unwrap();
    assert_content(&fixture, &content);
    assert!(!pending(&fixture.bin()).unwrap());
    fixture.finish();
}

#[test]
fn ark_tools_uninstall_preserves_incomplete_preparation_without_blocking_retries() {
    for interruption in [
        "missing-backup",
        "truncated-journal",
        "truncated-backup",
        "write-failure",
    ] {
        let fixture = Fixture::new();
        let content = installed(&fixture);
        let wanted = hashes(&content);
        let owner = fs::read(fixture.bin().join(OWNER)).unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            remove_with_writes(
                &fixture.0,
                &ASE,
                &wanted,
                |_| Ok(()),
                |path, bytes| {
                    let interrupted_name = if interruption == "truncated-journal" {
                        JOURNAL
                    } else {
                        "0.old"
                    };
                    if path
                        .file_name()
                        .is_some_and(|name| name == interrupted_name)
                    {
                        if interruption != "missing-backup" {
                            recovery::write_new(path, &bytes[..bytes.len() / 2]).unwrap();
                        }
                        if interruption == "write-failure" {
                            return Err("injected partial backup write failure".into());
                        }
                        panic!("simulated process interruption while writing {interrupted_name}");
                    }
                    recovery::write_new(path, bytes)
                },
            )
        }));
        let staged: Vec<_> = fs::read_dir(fixture.bin())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(STAGING)
            })
            .collect();
        assert_eq!(staged.len(), 1);
        let directory = &staged[0];
        assert_ne!(directory.file_name().unwrap(), STAGING);
        if interruption == "write-failure" {
            let error = result.unwrap().unwrap_err();
            assert!(error.contains("preparation retained at"));
            assert!(error.contains(&directory.display().to_string()));
        } else {
            assert!(result.is_err());
        }
        let journal_bytes = fs::read(directory.join(JOURNAL)).unwrap();
        let backup_bytes = files::read(directory, "0.old").unwrap();
        if interruption == "truncated-journal" {
            assert!(serde_json::from_slice::<Journal>(&journal_bytes).is_err());
        } else if interruption != "missing-backup" {
            assert_eq!(
                backup_bytes.as_ref().unwrap().len(),
                content[wanted.keys().next().unwrap()].len() / 2
            );
        }
        // Preparation never mutated the runtime, even if the process stopped
        // in write_all. Its bytes remain diagnostic data, not recovery input.
        recovery::recover(&fixture.bin(), &ASE, &wanted).unwrap();
        assert_content(&fixture, &content);
        assert_eq!(fs::read(fixture.bin().join(OWNER)).unwrap(), owner);
        assert!(!pending(&fixture.bin()).unwrap());
        ensure_can_start(&fixture.0).unwrap();
        publish(&fixture.0, &ASE, content.clone(), &wanted).unwrap();
        assert_content(&fixture, &content);
        assert!(!remove(&fixture.0, &ASE, &wanted).unwrap());
        for path in wanted.keys() {
            assert!(!fixture.bin().join(path).exists());
        }
        assert_eq!(fs::read(directory.join(JOURNAL)).unwrap(), journal_bytes);
        assert_eq!(files::read(directory, "0.old").unwrap(), backup_bytes);
        assert!(!pending(&fixture.bin()).unwrap());
        ensure_can_start(&fixture.0).unwrap();
        fixture.finish();
    }
}

#[test]
fn ark_tools_uninstall_cleanup_resumes_after_partial_backup_or_journal_cleanup() {
    for state in [COMMITTED, ROLLED_BACK] {
        let fixture = Fixture::new();
        let content = installed(&fixture);
        let wanted = hashes(&content);
        interrupted(&fixture, &wanted, wanted.len() - 1);
        let pending = fixture.bin().join(PENDING);
        if state == ROLLED_BACK {
            for (index, path) in wanted.keys().enumerate() {
                fs::hard_link(
                    pending.join(format!("{index}.old")),
                    fixture.bin().join(path),
                )
                .unwrap();
            }
        }
        let directory = fixture.bin().join(state);
        fs::rename(pending, &directory).unwrap();
        fs::remove_file(directory.join("0.old")).unwrap();
        recovery::recover(&fixture.bin(), &ASE, &wanted).unwrap();
        assert!(!directory.exists());
        if state == ROLLED_BACK {
            assert_content(&fixture, &content);
        } else {
            for path in wanted.keys() {
                assert!(!fixture.bin().join(path).exists());
            }
        }
        fs::create_dir(&directory).unwrap();
        recovery::recover(&fixture.bin(), &ASE, &wanted).unwrap();
        assert!(!directory.exists());
        fixture.finish();
    }
}

#[test]
fn ark_tools_uninstall_recovery_preserves_changed_files_without_partial_restoration() {
    for changed in ["runtime", "backup", "unknown", "owner"] {
        let fixture = Fixture::new();
        let content = installed(&fixture);
        let wanted = hashes(&content);
        interrupted(&fixture, &wanted, 1);
        let directory = fixture.bin().join(PENDING);
        let path = match changed {
            "runtime" => fixture.bin().join(PLUGIN),
            "backup" => directory.join("0.old"),
            "owner" => fixture.bin().join(OWNER),
            _ => directory.join("operator-notes.txt"),
        };
        fs::write(&path, b"external modifications").unwrap();
        let before: Vec<_> = wanted
            .keys()
            .map(|path| files::read(&fixture.bin(), path).unwrap())
            .collect();
        assert!(
            recovery::recover(&fixture.bin(), &ASE, &wanted).is_err(),
            "{changed}"
        );
        let after: Vec<_> = wanted
            .keys()
            .map(|path| files::read(&fixture.bin(), path).unwrap())
            .collect();
        assert_eq!(before, after, "{changed}");
        assert_eq!(fs::read(path).unwrap(), b"external modifications");
        assert!(directory.join(JOURNAL).is_file());
        fixture.finish();
    }
}

#[test]
fn ark_tools_uninstall_recovery_rejects_changed_journal_paths_and_backup_ownership() {
    for changed in ["path", "hash", "profile"] {
        let fixture = Fixture::new();
        let content = installed(&fixture);
        let wanted = hashes(&content);
        interrupted(&fixture, &wanted, 1);
        let path = fixture.bin().join(PENDING).join(JOURNAL);
        let mut journal: Journal = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        match changed {
            "path" => {
                journal.before.insert("../operator.txt".into(), None);
            }
            "hash" => {
                journal
                    .before
                    .insert(PLUGIN.into(), Some(digest(b"unowned")));
            }
            _ => {
                let mut owner: Ownership = serde_json::from_slice(&journal.owner).unwrap();
                owner.module_id = ASA.module_id.into();
                journal.owner = serde_json::to_vec(&owner).unwrap();
            }
        }
        fs::write(&path, serde_json::to_vec(&journal).unwrap()).unwrap();
        assert!(
            recovery::recover(&fixture.bin(), &ASE, &wanted).is_err(),
            "{changed}"
        );
        assert!(!fixture.bin().join(PLUGIN).exists());
        assert!(path.is_file());
        fixture.finish();
    }
}

#[test]
fn ark_tools_uninstall_start_gate_blocks_every_state_and_recovery_rejects_conflicts() {
    let fixture = Fixture::new();
    let content = installed(&fixture);
    let wanted = hashes(&content);
    for state in [PENDING, COMMITTED, ROLLED_BACK] {
        let path = fixture.bin().join(state);
        fs::create_dir(&path).unwrap();
        assert!(ensure_can_start(&fixture.0).is_err());
        fs::remove_dir(&path).unwrap();
        fs::write(&path, b"invalid marker").unwrap();
        assert!(ensure_can_start(&fixture.0).is_err());
        assert!(recovery::recover(&fixture.bin(), &ASE, &wanted).is_err());
        fs::remove_file(path).unwrap();
    }
    fs::create_dir(fixture.bin().join(PENDING)).unwrap();
    fs::create_dir(fixture.bin().join(".langame-ark-tools-staging")).unwrap();
    assert!(
        recover_all(&fixture.bin(), &ASE, &wanted)
            .unwrap_err()
            .contains("Conflicting")
    );
    assert_content(&fixture, &content);
    fixture.finish();
}

#[cfg(windows)]
#[test]
fn ark_tools_uninstall_recovery_rejects_junction_transaction_directories() {
    use std::os::windows::process::CommandExt;
    let fixture = Fixture::new();
    let content = installed(&fixture);
    let wanted = hashes(&content);
    interrupted(&fixture, &wanted, 1);
    let link = fixture.bin().join(PENDING);
    let target = fixture.0.join("retained-removal-transaction");
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
    let result = recovery::recover(&fixture.bin(), &ASE, &wanted);
    fs::remove_dir(link).unwrap();
    assert!(result.is_err());
    assert_eq!(fs::read(target.join(JOURNAL)).unwrap(), journal);
    assert!(!fixture.bin().join(PLUGIN).exists());
    fixture.finish();
}
