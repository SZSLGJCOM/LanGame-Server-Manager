use super::*;

#[test]
fn resolves_items_across_game_and_independent_steamcmd_caches() {
    let root = make_temp_root("multiple-caches");
    let install_root = root.join("server-files").join("projectzomboid");
    let steamcmd_root = root.join("separate-tools").join("steamcmd");
    for (cache_root, item_id, mod_id) in [
        (&install_root, "3000000001", "GameCacheMod"),
        (&steamcmd_root, "3000000002", "SteamCmdCacheMod"),
    ] {
        let mod_root = cache_root
            .join("steamapps/workshop/content/108600")
            .join(item_id)
            .join("Contents/mods")
            .join(mod_id);
        fs::create_dir_all(mod_root.join("media/maps").join(mod_id))
            .expect("create local cache mod");
        fs::write(mod_root.join("mod.info"), format!("id={mod_id}\n"))
            .expect("write local mod metadata");
    }

    let snapshot = read_project_zomboid_workshop_mods_snapshot(
        &install_root,
        &steamcmd_root,
        108600,
        &[
            String::from("3000000001"),
            String::from("3000000002"),
            String::from("3000000003"),
        ],
    );
    assert!(snapshot.workshop_root_exists);
    assert_eq!(snapshot.items[0].status, "installed");
    assert_eq!(snapshot.items[1].status, "installed");
    assert_eq!(
        snapshot.items[1].mods[0].mod_id.as_deref(),
        Some("SteamCmdCacheMod")
    );
    assert_eq!(snapshot.items[1].mods[0].map_ids, ["SteamCmdCacheMod"]);
    assert!(Path::new(&snapshot.items[1].item_path).starts_with(&steamcmd_root));
    assert_eq!(snapshot.items[2].status, "missing_item");
    fs::remove_dir_all(root).expect("remove fixture");
}

#[test]
fn resolves_fallback_when_game_cache_is_absent() {
    let root = make_temp_root("fallback-only");
    let install_root = root.join("server-files").join("projectzomboid");
    let steamcmd_root = root.join("tools").join("steamcmd");
    let workshop_root = steamcmd_root.join("steamapps/workshop/content/108600");
    fs::create_dir_all(workshop_root.join("3000000001/Contents/mods/Example"))
        .expect("create fallback mod");
    fs::write(
        workshop_root.join("3000000001/Contents/mods/Example/mod.info"),
        "id=Example\n",
    )
    .expect("write fallback metadata");

    let snapshot = read_project_zomboid_workshop_mods_snapshot(
        &install_root,
        &steamcmd_root,
        108600,
        &[String::from("3000000001")],
    );
    assert_eq!(Path::new(&snapshot.workshop_root), workshop_root);
    assert_eq!(snapshot.items[0].status, "installed");
    fs::remove_dir_all(root).expect("remove fixture");
}

fn make_temp_root(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "langame-pz-mods-{label}-{}",
        uuid::Uuid::new_v4().simple()
    ));
    fs::create_dir_all(&path).expect("create temp root");
    path
}

#[test]
fn reads_local_workshop_mod_ids_and_map_ids() {
    let root = make_temp_root("basic");
    let install_root = root.join("projectzomboid");
    let item_root = install_root
        .join("steamapps")
        .join("workshop")
        .join("content")
        .join("108600")
        .join("2945221351");
    let tile_pack_root = item_root.join("Contents").join("mods").join("TilePack");
    let weapons_root = item_root
        .join("Contents")
        .join("mods")
        .join("SimpleWeapons");
    fs::create_dir_all(tile_pack_root.join("media").join("maps").join("RavenCreek"))
        .expect("create tile pack map");
    fs::create_dir_all(weapons_root.join("42")).expect("create versioned mod");
    fs::write(
        tile_pack_root.join("mod.info"),
        "name=Raven Creek Tiles\nid=RavenCreekTilePack\n",
    )
    .expect("write tile pack mod info");
    fs::write(
        weapons_root.join("42").join("mod.info"),
        "name=Simple Weapons\nid=SimpleWeapons\n",
    )
    .expect("write weapons mod info");

    let snapshot = read_project_zomboid_workshop_mods_snapshot(
        &install_root,
        &root.join("steamcmd"),
        108600,
        &[String::from("2945221351")],
    );

    assert!(snapshot.workshop_root_exists);
    assert_eq!(snapshot.items.len(), 1);
    assert_eq!(snapshot.items[0].status, "installed");
    assert_eq!(snapshot.items[0].mods.len(), 2);
    let raven_creek_mod = snapshot.items[0]
        .mods
        .iter()
        .find(|item| item.mod_id.as_deref() == Some("RavenCreekTilePack"))
        .expect("raven creek mod");
    let simple_weapons_mod = snapshot.items[0]
        .mods
        .iter()
        .find(|item| item.mod_id.as_deref() == Some("SimpleWeapons"))
        .expect("simple weapons mod");
    assert_eq!(raven_creek_mod.map_ids, vec![String::from("RavenCreek")]);
    assert_eq!(
        raven_creek_mod.mod_id.as_deref(),
        Some("RavenCreekTilePack")
    );
    assert_eq!(simple_weapons_mod.mod_id.as_deref(), Some("SimpleWeapons"));

    fs::remove_dir_all(root).ok();
}

#[test]
fn decodes_gbk_mod_info_names() {
    let root = make_temp_root("gbk");
    let install_root = root.join("projectzomboid");
    let mod_root = install_root
        .join("steamapps")
        .join("workshop")
        .join("content")
        .join("108600")
        .join("2945221351")
        .join("Contents")
        .join("mods")
        .join("ChineseMap");
    fs::create_dir_all(mod_root.join("media").join("maps").join("CNMap")).expect("create mod map");
    fs::write(
        mod_root.join("mod.info"),
        [
            0x6E, 0x61, 0x6D, 0x65, 0x3D, 0xD6, 0xD0, 0xCE, 0xC4, 0xB5, 0xD8, 0xCD, 0xBC, 0x0A,
            0x69, 0x64, 0x3D, 0x43, 0x68, 0x69, 0x6E, 0x65, 0x73, 0x65, 0x4D, 0x61, 0x70, 0x0A,
        ],
    )
    .expect("write gbk mod info");

    let snapshot = read_project_zomboid_workshop_mods_snapshot(
        &install_root,
        &root.join("steamcmd"),
        108600,
        &[String::from("2945221351")],
    );

    assert_eq!(
        snapshot.items[0].mods[0].mod_name.as_deref(),
        Some("\u{4e2d}\u{6587}\u{5730}\u{56fe}")
    );

    fs::remove_dir_all(root).ok();
}

#[test]
fn prefers_versioned_mod_info_when_present() {
    let root = make_temp_root("versioned");
    let install_root = root.join("projectzomboid");
    let mod_root = install_root
        .join("steamapps")
        .join("workshop")
        .join("content")
        .join("108600")
        .join("3000000000")
        .join("Contents")
        .join("mods")
        .join("VersionedMod");
    fs::create_dir_all(
        mod_root
            .join("42")
            .join("media")
            .join("maps")
            .join("Dir42Map"),
    )
    .expect("create versioned map");
    fs::write(mod_root.join("mod.info"), "name=Legacy\nid=LegacyId\n")
        .expect("write root mod info");
    fs::write(
        mod_root.join("42").join("mod.info"),
        "name=Build 42\nid=Build42Id\n",
    )
    .expect("write build 42 mod info");

    let snapshot = read_project_zomboid_workshop_mods_snapshot(
        &install_root,
        &root.join("steamcmd"),
        108600,
        &[String::from("3000000000")],
    );

    assert_eq!(snapshot.items[0].mods.len(), 1);
    assert_eq!(
        snapshot.items[0].mods[0].mod_id.as_deref(),
        Some("Build42Id")
    );
    assert_eq!(
        snapshot.items[0].mods[0].map_ids,
        vec![String::from("Dir42Map")]
    );

    fs::remove_dir_all(root).ok();
}

#[test]
fn reports_missing_workshop_root_and_missing_item() {
    let root = make_temp_root("missing");
    let install_root = root.join("projectzomboid");
    fs::create_dir_all(&install_root).expect("create install root");

    let missing_root_snapshot = read_project_zomboid_workshop_mods_snapshot(
        &install_root,
        &root.join("steamcmd"),
        108600,
        &[String::from("3012345678")],
    );
    assert!(!missing_root_snapshot.workshop_root_exists);
    assert_eq!(
        missing_root_snapshot.items[0].status,
        "missing_workshop_root"
    );

    let workshop_root = install_root
        .join("steamapps")
        .join("workshop")
        .join("content")
        .join("108600");
    fs::create_dir_all(&workshop_root).expect("create workshop root");
    let missing_item_snapshot = read_project_zomboid_workshop_mods_snapshot(
        &install_root,
        &root.join("steamcmd"),
        108600,
        &[String::from("3012345678")],
    );
    assert!(missing_item_snapshot.workshop_root_exists);
    assert_eq!(missing_item_snapshot.items[0].status, "missing_item");

    fs::remove_dir_all(root).ok();
}
