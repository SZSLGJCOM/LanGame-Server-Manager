use super::*;
use app_steamcmd::SteamWorkshopInstallationItemStatus;

#[test]
fn dst_workshop_incomplete_cache_requires_repair_and_valid_alternate_is_reused() {
    let root = std::env::temp_dir().join(format!(
        "dst-cache-selection-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let primary = root.join("primary");
    let alternate = root.join("alternate");
    let ids = vec![String::from("2039181790")];
    let relative = Path::new("content/322330").join(&ids[0]);
    fs::create_dir_all(primary.join(&relative)).unwrap();
    fs::write(primary.join(&relative).join("modinfo.lua"), "partial").unwrap();
    let mut snapshot = SteamWorkshopInstallationSnapshot {
        consumer_app_id: 322_330,
        searched_roots: vec![
            primary
                .join("content/322330")
                .to_string_lossy()
                .into_owned(),
            alternate
                .join("content/322330")
                .to_string_lossy()
                .into_owned(),
        ],
        items: vec![SteamWorkshopInstallationItemStatus {
            item_id: ids[0].clone(),
            path: primary.join(&relative).to_string_lossy().into_owned(),
            installed: true,
        }],
    };
    verify_cached_items(&mut snapshot, &ids);
    assert!(
        !snapshot.items[0].installed,
        "a residual directory must trigger SteamCMD repair"
    );
    fs::create_dir_all(alternate.join(&relative)).unwrap();
    fs::write(alternate.join(&relative).join("modinfo.lua"), "complete").unwrap();
    fs::write(
        alternate.join("appworkshop_322330.acf"),
        r#"
"AppWorkshop" { "appid" "322330"
"WorkshopItemsInstalled" { "2039181790" { "manifest" "123456" "size" "8" } }
"WorkshopItemDetails" { "2039181790" { "manifest" "123456" } }
}"#,
    )
    .unwrap();
    verify_cached_items(&mut snapshot, &ids);
    assert!(snapshot.items[0].installed);
    assert_eq!(
        Path::new(&snapshot.items[0].path),
        alternate.join(&relative)
    );
    assert!(
        !primary.join("appworkshop_322330.acf").exists(),
        "inspection must not publish metadata"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn dst_workshop_inventory_verification_does_not_change_other_games() {
    let mut snapshot = SteamWorkshopInstallationSnapshot {
        consumer_app_id: 393_380,
        searched_roots: vec![],
        items: vec![SteamWorkshopInstallationItemStatus {
            item_id: String::from("2039181790"),
            path: String::from("any-layout"),
            installed: true,
        }],
    };
    verify_cached_items(&mut snapshot, &[String::from("2039181790")]);
    assert!(snapshot.items[0].installed);
    assert_eq!(snapshot.items[0].path, "any-layout");
}
