use super::*;
use crate::ark_clusters::{ArkClusterIssue, ArkClusterMember};
use app_core::{InstanceSummary, PortBinding};

#[cfg(windows)]
#[path = "ark_cluster_backups_concurrency_tests.rs"]
mod concurrency;

fn report() -> ArkClusterReport {
    let identity = ArkClusterIdentity {
        module_id: "arksurvivalevolved".to_owned(),
        cluster_id: "friends".to_owned(),
        directory_key: "cluster-root".to_owned(),
        member_ids: vec!["center".to_owned(), "island".to_owned()],
    };
    ArkClusterReport {
        instance_id: "island".to_owned(),
        identity: Some(identity),
        cluster_directory: Some("cluster-root".to_owned()),
        members: ["center", "island"]
            .into_iter()
            .map(|id| ArkClusterMember {
                summary: InstanceSummary {
                    id: id.to_owned(),
                    name: id.to_owned(),
                    module_id: "arksurvivalevolved".to_owned(),
                    status: InstanceStatus::Stopped,
                    active_process_count: 0,
                    autostart: false,
                    bind_ip: "0.0.0.0".to_owned(),
                    port_count: 1,
                },
                map_name: "TheIsland".to_owned(),
                cluster_id: "friends".to_owned(),
                cluster_directory: Some("cluster-root".to_owned()),
                explicit_shared_directory: true,
                config_file_path: format!("instances/{id}/config/instance.json"),
                saves_path: format!("instances/{id}/runtime/ShooterGame/Saved/{id}"),
                ports: vec![PortBinding {
                    name: "game".to_owned(),
                    protocol: "udp".to_owned(),
                    port: 7777,
                }],
            })
            .collect(),
        related_instances: Vec::new(),
        issues: Vec::new(),
        start_blocked: false,
    }
}

#[cfg(windows)]
struct Fixture {
    root: PathBuf,
    plan: Plan,
}

#[cfg(windows)]
impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("lgsm-cluster-snapshot-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let mut report = report();
        let transfer = root.join("transfer");
        fs::create_dir(&transfer).unwrap();
        report.cluster_directory = Some(transfer.to_string_lossy().into_owned());
        report.identity.as_mut().unwrap().directory_key = path_key(&transfer);
        let mut scopes = Vec::new();
        for (index, member) in report.members.iter_mut().enumerate() {
            let instance = root.join("instances").join(&member.summary.id);
            let config = instance.join("config");
            let saved = instance.join("runtime/ShooterGame/Saved");
            fs::create_dir_all(&config).unwrap();
            fs::create_dir_all(&saved).unwrap();
            member.config_file_path = config.join("instance.json").to_string_lossy().into_owned();
            member.saves_path = saved.join("world").to_string_lossy().into_owned();
            member.cluster_directory = report.cluster_directory.clone();
            let value = serde_json::json!({
                "instance_id": member.summary.id, "module_id": member.summary.module_id,
                "instance_name": member.summary.name, "autostart": member.summary.autostart,
                "ports": member.ports,
                "settings": { "bind_ip": member.summary.bind_ip, "cluster_id": "friends", "cluster_directory": transfer, "map_name": "TheIsland" }
            });
            fs::write(
                config.join("instance.json"),
                serde_json::to_vec(&value).unwrap(),
            )
            .unwrap();
            for (kind, target) in [("config", config), ("saved", saved)] {
                fs::write(target.join("data"), format!("original-{index}-{kind}")).unwrap();
                scopes.push(Scope {
                    key: format!("member-{index:03}-{kind}"),
                    target,
                    existed: false,
                    entries: Vec::new(),
                });
            }
        }
        fs::write(transfer.join("upload"), b"original-transfer").unwrap();
        scopes.push(Scope {
            key: "transfer".to_owned(),
            target: transfer,
            existed: false,
            entries: Vec::new(),
        });
        let backups =
            backup_root(&fixture_paths(&root), report.identity.as_ref().unwrap()).unwrap();
        fs::create_dir_all(&backups).unwrap();
        Self {
            root,
            plan: Plan {
                root: backups,
                report,
                scopes,
            },
        }
    }

    fn change(&self) {
        for (index, scope) in self.plan.scopes.iter().enumerate() {
            fs::write(scope.target.join("changed"), format!("current-{index}")).unwrap();
        }
    }

    fn inventory(&self) -> Vec<Vec<files::Entry>> {
        self.plan
            .scopes
            .iter()
            .map(|scope| files::inventory(&scope.target).unwrap())
            .collect()
    }

    fn records(&self) -> Vec<crate::StoredInstanceRecord> {
        self.plan
            .report
            .members
            .iter()
            .map(|member| crate::StoredInstanceRecord {
                summary: member.summary.clone(),
                config_dir: Path::new(&member.config_file_path)
                    .parent()
                    .unwrap()
                    .to_owned(),
                saves_dir: PathBuf::from(&member.saves_path),
                runtime_mode: String::from("independent"),
                program_install_root: Path::new(&member.config_file_path)
                    .parent()
                    .and_then(Path::parent)
                    .map(|root| root.join("runtime")),
                auto_backup_on_stop: false,
                backup_retention_count: 5,
            })
            .collect()
    }
}

#[cfg(windows)]
fn fixture_paths(root: &Path) -> StoragePaths {
    StoragePaths {
        app_data_root: root.join("app-data"),
        settings_path: root.join("app-data/settings.json"),
        database_path: root.join("app-data/database.sqlite"),
        logs_root: root.join("logs"),
        modules_root: root.join("modules"),
        migrations_root: root.join("migrations"),
        steamcmd_root: root.join("steamcmd"),
        games_root: root.join("games"),
        instances_root: root.join("instances"),
        archives_root: root.join("instances").join(".trash"),
    }
}

#[cfg(windows)]
impl Drop for Fixture {
    fn drop(&mut self) {
        let canonical = fs::canonicalize(&self.root).unwrap();
        assert_eq!(
            canonical.parent(),
            Some(fs::canonicalize(std::env::temp_dir()).unwrap().as_path())
        );
        fs::remove_dir_all(canonical).unwrap();
    }
}

#[cfg(windows)]
#[test]
fn cluster_restore_rolls_back_every_scope_after_mid_group_failure() {
    let fixture = Fixture::new();
    let backup = transaction::snapshot(&fixture.plan, "manual").unwrap();
    fixture.change();
    let before = fixture.inventory();
    let error =
        restore::restore_with_hook(&fixture.plan, &backup.summary.backup_id, |phase, index| {
            if phase == restore::Phase::Published && index == 2 {
                Err(invalid(
                    Path::new("fixture"),
                    "injected publication failure",
                ))
            } else {
                Ok(())
            }
        })
        .unwrap_err();
    assert!(error.to_string().contains("injected publication failure"));
    assert_eq!(fixture.inventory(), before);
    assert!(transaction::ensure_ready(&fixture.plan.root).is_ok());
    let listed = transaction::list(
        &fixture.plan.root,
        fixture.plan.report.identity.as_ref().unwrap(),
    )
    .unwrap();
    assert_eq!(listed.len(), 2);
    assert!(
        listed
            .iter()
            .any(|backup| backup.backup_kind == "pre_restore")
    );
}

#[cfg(windows)]
#[test]
fn cluster_restore_success_restores_all_members_and_transfer_and_preserves_safeguard() {
    let fixture = Fixture::new();
    let original = fixture.inventory();
    let backup = transaction::snapshot(&fixture.plan, "manual").unwrap();
    fixture.change();
    let before = fixture.inventory();
    let result = restore::restore(&fixture.plan, &backup.summary.backup_id).unwrap();
    assert_eq!(fixture.inventory(), original);
    let safeguard =
        transaction::load(&fixture.plan.root, &result.safeguard_backup.backup_id).unwrap();
    assert_eq!(
        safeguard
            .scopes
            .iter()
            .map(|scope| scope.entries.clone())
            .collect::<Vec<_>>(),
        before
    );
    assert!(result.cleanup_warnings.is_empty());
}

#[cfg(windows)]
#[test]
fn cluster_restore_refuses_tampered_files_members_and_targets_before_live_mutation() {
    let fixture = Fixture::new();
    let mut backup = transaction::snapshot(&fixture.plan, "manual").unwrap();
    fixture.change();
    let before = fixture.inventory();
    let source = fixture.plan.root.join(&backup.summary.backup_id);
    fs::write(source.join("member-000-saved/data"), b"tampered").unwrap();
    assert!(restore::restore(&fixture.plan, &backup.summary.backup_id).is_err());
    assert_eq!(fixture.inventory(), before);
    backup.scopes[0].target = fixture.root.join("unrelated");
    transaction::write_json(&source.join("manifest.json"), &backup).unwrap();
    assert!(restore::restore(&fixture.plan, &backup.summary.backup_id).is_err());
    assert_eq!(fixture.inventory(), before);
    assert!(restore::restore(&fixture.plan, "../outside").is_err());
    assert_eq!(fixture.inventory(), before);
}

#[cfg(windows)]
#[test]
fn cluster_restore_recovers_process_interruption_between_config_renames() {
    let fixture = Fixture::new();
    let backup = transaction::snapshot(&fixture.plan, "manual").unwrap();
    fixture.change();
    let before = fixture.inventory();
    let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        restore::restore_with_hook(&fixture.plan, &backup.summary.backup_id, |phase, index| {
            if phase == restore::Phase::OriginalMoved && index == 2 {
                panic!("simulate process interruption");
            }
            Ok(())
        })
    }));
    assert!(interrupted.is_err());
    assert!(!fixture.plan.scopes[2].target.exists());
    assert!(transaction::ensure_ready(&fixture.plan.root).is_err());
    let paths = fixture_paths(&fixture.root);
    let records = fixture.records();
    let pending = recovery::read_pending(&paths, &records[1])
        .unwrap()
        .unwrap();
    let ports = fixture
        .plan
        .report
        .members
        .iter()
        .map(|member| crate::InstancePortProjection {
            instance_id: member.summary.id.clone(),
            module_id: member.summary.module_id.clone(),
            bind_ip: member.summary.bind_ip.clone(),
            ports: member.ports.clone(),
        })
        .collect::<Vec<_>>();
    let reconstructed = recovery::report_for_recovery(
        &paths,
        &records,
        &ports,
        &pending.identity,
        &pending.backup_id,
    )
    .unwrap();
    let forbidden = records
        .iter()
        .flat_map(|record| [record.config_dir.clone(), record.saves_dir.clone()])
        .collect::<Vec<_>>();
    let plan = build_plan(&paths, reconstructed, &forbidden).unwrap();
    let result = recovery::recover(&plan, &pending.backup_id).unwrap();
    assert_eq!(result.outcome, "rolled_back");
    assert_eq!(fixture.inventory(), before);
    assert!(transaction::ensure_ready(&fixture.plan.root).is_ok());
    assert!(
        recovery::read_pending(&paths, &records[1])
            .unwrap()
            .is_none()
    );
}

#[cfg(windows)]
#[test]
fn cluster_restore_partial_pointer_failure_cleans_every_marker_without_changing_live_data() {
    let fixture = Fixture::new();
    let backup = transaction::snapshot(&fixture.plan, "manual").unwrap();
    fixture.change();
    let before = fixture.inventory();
    let second_pointer = fixture.plan.scopes[2]
        .target
        .parent()
        .unwrap()
        .join(recovery::POINTER_NAME);
    crate::atomic_file::fail_next_atomic_write_for_test(&second_pointer);
    assert!(restore::restore(&fixture.plan, &backup.summary.backup_id).is_err());
    assert_eq!(fixture.inventory(), before);
    assert!(!second_pointer.exists());
    assert!(
        recovery::read_pending_root(
            &fixture.plan.root,
            fixture.plan.report.identity.as_ref().unwrap()
        )
        .unwrap()
        .is_none()
    );
    assert_eq!(fixture.inventory(), before);
    assert!(transaction::ensure_ready(&fixture.plan.root).is_ok());
}

#[cfg(windows)]
#[test]
fn cluster_restore_committed_interruption_finishes_cleanup_without_undoing_data() {
    let fixture = Fixture::new();
    let original = fixture.inventory();
    let backup = transaction::snapshot(&fixture.plan, "manual").unwrap();
    fixture.change();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            restore::restore_with_hook(&fixture.plan, &backup.summary.backup_id, |phase, _| {
                if phase == restore::Phase::Committed {
                    panic!("simulate interruption after commit");
                }
                Ok(())
            })
        }))
        .is_err()
    );
    let result = recovery::recover(&fixture.plan, &backup.summary.backup_id).unwrap();
    assert_eq!(result.outcome, "completed");
    assert_eq!(fixture.inventory(), original);
    assert!(transaction::ensure_ready(&fixture.plan.root).is_ok());
}

#[cfg(windows)]
#[test]
fn cluster_restore_recovery_rejects_changed_registration_without_disposing_recovery_data() {
    for interruption in [restore::Phase::OriginalMoved, restore::Phase::Committed] {
        let mut fixture = Fixture::new();
        let original = fixture.inventory();
        let backup = transaction::snapshot(&fixture.plan, "manual").unwrap();
        fixture.change();
        let changed = fixture.inventory();
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                restore::restore_with_hook(
                    &fixture.plan,
                    &backup.summary.backup_id,
                    |phase, index| {
                        if phase == interruption
                            && (phase == restore::Phase::Committed || index == 2)
                        {
                            panic!("simulate interruption before changing registration");
                        }
                        Ok(())
                    },
                )
            }))
            .is_err()
        );
        let interrupted = fixture.inventory();
        fixture.plan.report.members[0].summary.autostart = true;
        fixture.plan.report.members[0].ports[0].port += 1000;
        let error = recovery::recover(&fixture.plan, &backup.summary.backup_id).unwrap_err();
        assert!(error.to_string().contains("Snapshot registration differs"));
        assert_eq!(fixture.inventory(), interrupted);
        assert!(transaction::ensure_ready(&fixture.plan.root).is_err());
        fixture.plan.report.members[0].summary.autostart = false;
        fixture.plan.report.members[0].ports[0].port -= 1000;
        let result = recovery::recover(&fixture.plan, &backup.summary.backup_id).unwrap();
        assert!(result.cleanup_warnings.is_empty());
        assert_eq!(
            fixture.inventory(),
            if interruption == restore::Phase::Committed {
                original
            } else {
                changed
            }
        );
        assert!(transaction::ensure_ready(&fixture.plan.root).is_ok());
    }
}

#[cfg(windows)]
#[test]
fn cluster_recovery_refuses_a_forged_workspace_outside_the_member_parent() {
    let fixture = Fixture::new();
    let backup = transaction::snapshot(&fixture.plan, "manual").unwrap();
    fixture.change();
    let before = fixture.inventory();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            restore::restore_with_hook(&fixture.plan, &backup.summary.backup_id, |phase, index| {
                if phase == restore::Phase::OriginalMoved && index == 0 {
                    panic!("simulate interruption");
                }
                Ok(())
            })
        }))
        .is_err()
    );
    let path = fixture.plan.root.join(recovery::JOURNAL_NAME);
    let original: recovery::Journal = transaction::read_json(&path, 2 * 1024 * 1024).unwrap();
    let mut forged = original.clone();
    forged.workspaces[0].path = fixture.root.join(".langame-ark-restore-0123456789ab");
    fs::create_dir(&forged.workspaces[0].path).unwrap();
    fs::write(forged.workspaces[0].path.join("unrelated"), b"protected").unwrap();
    transaction::write_json(&path, &forged).unwrap();
    assert!(recovery::recover(&fixture.plan, &backup.summary.backup_id).is_err());
    assert_eq!(
        fs::read(forged.workspaces[0].path.join("unrelated")).unwrap(),
        b"protected"
    );
    transaction::write_json(&path, &original).unwrap();
    recovery::recover(&fixture.plan, &backup.summary.backup_id).unwrap();
    assert_eq!(fixture.inventory(), before);
}

#[test]
fn cluster_snapshot_copy_rejects_parent_traversal_before_creating_a_file() {
    let root = std::env::temp_dir().join(format!(
        "lgsm-cluster-snapshot-path-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir(&root).unwrap();
    let entry = files::Entry {
        path: PathBuf::from("../outside"),
        bytes: Some(0),
        sha256: Some(String::new()),
    };
    assert!(files::copy_verified(&root, &root.join("stage"), &[entry]).is_err());
    assert!(!root.join("outside").exists());
    assert_eq!(
        fs::canonicalize(&root).unwrap().parent(),
        Some(fs::canonicalize(std::env::temp_dir()).unwrap().as_path())
    );
    fs::remove_dir_all(root).unwrap();
}

#[cfg(windows)]
#[test]
fn cluster_snapshot_rejects_junction_ancestors_and_preserves_external_data() {
    use std::os::windows::process::CommandExt;
    let fixture = Fixture::new();
    let link = fixture.plan.scopes[1].target.join("linked-transfer");
    let target = &fixture.plan.scopes[4].target;
    let native_link = fs::canonicalize(link.parent().unwrap())
        .unwrap()
        .join(link.file_name().unwrap());
    let native_target = fs::canonicalize(target).unwrap();
    let output = std::process::Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(native_link.to_string_lossy().replace('/', "\\"))
        .arg(native_target.to_string_lossy().replace('/', "\\"))
        .creation_flags(0x0800_0000)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "junction fixture creation failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let inventory_result = files::inventory(&fixture.plan.scopes[1].target);
    let descendant_result = files::inventory(&link.join("not-created"));
    fs::remove_dir(&link).unwrap();
    assert!(inventory_result.is_err());
    assert!(descendant_result.is_err());
    assert_eq!(
        fs::read(target.join("upload")).unwrap(),
        b"original-transfer"
    );
}

#[cfg(windows)]
#[test]
fn cluster_snapshot_capacity_accepts_extended_length_directories() {
    let fixture = Fixture::new();
    let long = fixture.root.join("capacity-path-".repeat(15));
    fs::create_dir(&long).unwrap();
    assert!(
        long.as_os_str().len() > 260,
        "fixture must exercise the Win32 long-path boundary"
    );
    files::require_capacity(&long, 0).unwrap();
}

#[cfg(windows)]
#[test]
fn cluster_snapshot_canonical_and_missing_paths_keep_the_same_windows_boundary() {
    let fixture = Fixture::new();
    let canonical = fs::canonicalize(&fixture.root).unwrap();
    assert!(overlaps(
        &canonical,
        &fixture.root.join("not-created/deeper")
    ));
    assert!(overlaps(
        &fixture.root,
        &canonical.join("not-created/deeper")
    ));
    assert!(!overlaps(
        &fixture.root.join("transfer"),
        &fixture.root.join("transfer-neighbor")
    ));
}

#[test]
fn cluster_snapshot_rejects_member_changes_and_every_active_transition() {
    let baseline = report();
    let expected = baseline.identity.clone().unwrap();
    assert!(validate_report(&baseline, &expected).is_ok());
    for status in [
        InstanceStatus::Starting,
        InstanceStatus::Running,
        InstanceStatus::Stopping,
        InstanceStatus::Error,
    ] {
        let mut current = baseline.clone();
        current.members[1].summary.status = status;
        assert!(validate_report(&current, &expected).is_err());
    }
    let mut current = baseline.clone();
    current.members[1].summary.active_process_count = 1;
    assert!(validate_report(&current, &expected).is_err());
    let mut current = baseline.clone();
    current
        .identity
        .as_mut()
        .unwrap()
        .member_ids
        .push("another-map".to_owned());
    assert!(validate_report(&current, &expected).is_err());
}

#[test]
fn cluster_snapshot_rejects_implicit_roots_and_unclassified_peers() {
    let mut current = report();
    let expected = current.identity.clone().unwrap();
    current.members[0].explicit_shared_directory = false;
    assert!(validate_report(&current, &expected).is_err());
    current.members[0].explicit_shared_directory = true;
    current.issues.push(ArkClusterIssue {
        code: "peer_inspection_incomplete".to_owned(),
        severity: "warning".to_owned(),
        instance_id: "unknown".to_owned(),
        instance_name: "unknown".to_owned(),
        message: "unclassified".to_owned(),
        path: None,
    });
    assert!(validate_report(&current, &expected).is_err());
}

#[test]
fn cluster_snapshot_protects_parent_and_child_directory_boundaries() {
    assert!(overlaps(
        Path::new("C:/clusters/friends"),
        Path::new("C:/clusters")
    ));
    assert!(overlaps(
        Path::new("C:/clusters/friends"),
        Path::new("C:/clusters/friends/other")
    ));
    assert!(!overlaps(
        Path::new("C:/clusters/friends"),
        Path::new("C:/clusters/friends-next")
    ));
}
