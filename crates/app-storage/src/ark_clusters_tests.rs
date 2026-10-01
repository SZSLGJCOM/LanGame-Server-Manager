use super::*;
use app_core::InstanceStatus;

fn member(id: &str, edition: &str, cluster: &str, directory: Option<&str>) -> Projection {
    Projection {
        member: ArkClusterMember {
            summary: InstanceSummary {
                id: id.to_owned(),
                name: format!("Server {id}"),
                module_id: edition.to_owned(),
                status: InstanceStatus::Stopped,
                active_process_count: 0,
                autostart: false,
                bind_ip: "0.0.0.0".to_owned(),
                port_count: 1,
            },
            map_name: "TheIsland".to_owned(),
            cluster_id: cluster.to_owned(),
            cluster_directory: directory.map(str::to_owned),
            explicit_shared_directory: true,
            config_file_path: format!("/instances/{id}/config/instance.json"),
            saves_path: format!("/instances/{id}/saves"),
            ports: vec![PortBinding {
                name: "game".to_owned(),
                protocol: "udp".to_owned(),
                port: 7777,
            }],
        },
        directory_key: directory.map(str::to_owned),
        issues: Vec::new(),
    }
}

#[test]
fn ark_cluster_members_require_edition_id_and_shared_directory() {
    let report = assemble_report(
        "island",
        vec![
            member(
                "island",
                "arksurvivalevolved",
                "friends",
                Some("/shared/friends"),
            ),
            member(
                "center",
                "arksurvivalevolved",
                "friends",
                Some("/shared/friends"),
            ),
            member(
                "independent",
                "arksurvivalevolved",
                "another",
                Some("/shared/another"),
            ),
        ],
    )
    .unwrap();
    assert_eq!(report.identity.unwrap().member_ids, ["center", "island"]);
    assert!(report.related_instances.is_empty());
    assert!(!report.start_blocked);
    assert_eq!(report.members[0].ports[0].port, 7777);
}

#[test]
fn ark_cluster_same_id_in_private_directories_never_forms_a_shared_cluster() {
    let report = assemble_report(
        "island",
        vec![
            member(
                "island",
                "arksurvivalevolved",
                "friends",
                Some("/instances/island/cluster"),
            ),
            member(
                "center",
                "arksurvivalevolved",
                "friends",
                Some("/instances/center/cluster"),
            ),
        ],
    )
    .unwrap();
    assert_eq!(report.identity.unwrap().member_ids, ["island"]);
    assert_eq!(report.related_instances[0].summary.id, "center");
    assert_eq!(report.issues[0].code, "id_directory_mismatch");
    assert!(report.start_blocked);
}

#[test]
fn ark_cluster_shared_root_with_other_id_or_edition_blocks_group_start() {
    let report = assemble_report(
        "island",
        vec![
            member("island", "arksurvivalevolved", "friends", Some("/shared")),
            member("another", "arksurvivalevolved", "others", Some("/shared")),
            member(
                "ascended",
                "arksurvivalascended",
                "friends",
                Some("/shared"),
            ),
        ],
    )
    .unwrap();
    assert_eq!(report.members.len(), 1);
    assert_eq!(report.related_instances.len(), 2);
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.code == "directory_id_conflict")
    );
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.code == "directory_edition_conflict")
    );
    assert!(report.start_blocked);
}

#[test]
fn ark_cluster_unverified_directory_is_not_an_operation_identity() {
    let report = assemble_report(
        "island",
        vec![member("island", "arksurvivalevolved", "friends", None)],
    )
    .unwrap();
    assert!(report.identity.is_none());
    assert!(report.members.is_empty());
    assert!(report.start_blocked);
}

#[test]
fn ark_cluster_nested_roots_are_related_but_similar_prefixes_are_not() {
    let report = assemble_report(
        "island",
        vec![
            member(
                "island",
                "arksurvivalevolved",
                "friends",
                Some("/shared/friends"),
            ),
            member(
                "nested",
                "arksurvivalevolved",
                "other",
                Some("/shared/friends/other"),
            ),
            member(
                "unrelated",
                "arksurvivalevolved",
                "next",
                Some("/shared/friends-next"),
            ),
        ],
    )
    .unwrap();
    assert_eq!(report.related_instances.len(), 1);
    assert_eq!(report.related_instances[0].summary.id, "nested");
    assert_eq!(report.issues[0].code, "directory_overlap");
    assert!(report.start_blocked);
}

#[test]
fn ark_cluster_missing_tail_resolves_without_creating_transfer_data() {
    let root =
        std::env::temp_dir().join(format!("langame-ark-cluster-path-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let missing = root.join("new").join("cluster");
    let resolved = canonical_cluster_directory(&missing).unwrap();
    assert_eq!(
        resolved,
        std::fs::canonicalize(&root).unwrap().join("new/cluster")
    );
    assert!(!missing.exists());
    std::fs::remove_dir(&root).unwrap();
}

#[cfg(windows)]
#[test]
fn ark_cluster_windows_directory_keys_ignore_case_aliases() {
    assert_eq!(
        directory_key(Path::new(r"C:\Clusters\Friends")),
        directory_key(Path::new(r"c:\clusters\friends"))
    );
}
