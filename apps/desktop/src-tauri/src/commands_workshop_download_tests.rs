use super::*;
use app_steamcmd::SteamWorkshopInstallationItemStatus;

fn snapshot(items: &[(&str, bool)]) -> SteamWorkshopInstallationSnapshot {
    SteamWorkshopInstallationSnapshot {
        consumer_app_id: 393380,
        searched_roots: vec![String::from("cache")],
        items: items
            .iter()
            .map(|(id, installed)| SteamWorkshopInstallationItemStatus {
                item_id: String::from(*id),
                path: format!("cache/{id}"),
                installed: *installed,
            })
            .collect(),
    }
}

#[test]
fn missing_only_plan_downloads_missing_ids_and_retains_all_requested_deployments() {
    let before = snapshot(&[("100000", true), ("200000", false), ("900000", true)]);
    let ids = ["100000", "200000", "300000", " 100000 "].map(String::from);
    let plan = WorkshopDownloadPlan::from_snapshot(&ids, &before);
    assert_eq!(plan.download_ids, ["200000", "300000"]);
    assert_eq!(plan.requested_ids, ["100000", "200000", "300000"]);

    let after = snapshot(&[
        ("300000", true),
        ("900000", true),
        ("200000", true),
        ("100000", true),
    ]);
    let result = complete_download_result(
        &plan.requested_ids,
        Path::new("install"),
        after,
        String::new(),
    )
    .expect("every requested item is now cached");
    assert_eq!(
        result
            .items
            .iter()
            .map(|item| item.item_id.as_str())
            .collect::<Vec<_>>(),
        ["100000", "200000", "300000"]
    );
    assert!(result.items.iter().all(|item| item.expected_path_exists));
}

#[test]
fn cached_only_plan_does_not_request_a_download_and_reinspection_can_fail() {
    let ids = [String::from("100000")];
    let before = snapshot(&[("100000", true)]);
    let plan = WorkshopDownloadPlan::from_snapshot(&ids, &before);
    assert!(plan.download_ids.is_empty());
    for after in [snapshot(&[]), snapshot(&[("100000", false)])] {
        assert!(
            complete_download_result(
                &plan.requested_ids,
                Path::new("install"),
                after,
                String::new()
            )
            .expect_err("vanished cache must not report success")
            .contains("100000")
        );
    }
}

#[test]
fn existing_cache_is_deployed_to_an_instance_without_redownloading() {
    let root = std::env::temp_dir().join(format!(
        "langame-cache-deploy-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let cache_root = root.join("steamcmd/steamapps/workshop/content/393380");
    let item_root = cache_root.join("100000");
    let target_root = root.join("instance/runtime/SquadGame/Plugins/Mods");
    fs::create_dir_all(item_root.join("Content")).expect("prepare cached item");
    fs::write(item_root.join("Content/mod.pak"), "cached payload").expect("write cached payload");
    let before = SteamWorkshopInstallationSnapshot {
        consumer_app_id: 393380,
        searched_roots: vec![cache_root.to_string_lossy().into_owned()],
        items: vec![SteamWorkshopInstallationItemStatus {
            item_id: String::from("100000"),
            path: item_root.to_string_lossy().into_owned(),
            installed: true,
        }],
    };
    let plan = WorkshopDownloadPlan::from_snapshot(&[String::from("100000")], &before);
    assert!(plan.download_ids.is_empty());
    assert!(!target_root.exists());
    let result = complete_download_result(
        &plan.requested_ids,
        &root.join("shared-install"),
        before,
        String::from("Reused cached item."),
    )
    .expect("cached item remains part of deployment");
    let target = ResolvedManualModTarget {
        instance_id: String::from("server"),
        module_id: String::from("squad"),
        source_label: String::from("Steam Workshop"),
        target_label: String::from("SquadGame/Plugins/Mods"),
        target_path: target_root.clone(),
        accepts: vec![String::from("folder")],
        id_strategy: None,
    };
    let deployed = stage_downloaded_workshop_items_into_manual_target(&result, &target)
        .expect("deploy cached content into current instance");
    assert_eq!(deployed.copied_file_count, 1);
    assert_eq!(
        fs::read_to_string(target_root.join("100000/Content/mod.pak")).unwrap(),
        "cached payload"
    );
    assert!(item_root.join("Content/mod.pak").is_file());
    fs::remove_dir_all(root).expect("remove cache fixture");
}
