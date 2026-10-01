use super::*;

const ITEM_ID: &str = "3739491677";
const MODINFO: &[u8] = b"name = 'Native fixture'";

struct Fixture {
    root: PathBuf,
    install: PathBuf,
    settings: AppSettings,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "langame-dst-inventory-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let install = root.join("install");
        let steamcmd = root.join("steamcmd");
        fs::create_dir_all(&steamcmd).unwrap();
        fs::write(steamcmd.join("steamcmd.exe"), b"fixture").unwrap();
        let settings = AppSettings {
            archives_root: String::new(),
            servers_root: root.join("instances").to_string_lossy().into_owned(),
            games_root: root.join("games").to_string_lossy().into_owned(),
            modules_root: root.join("modules").to_string_lossy().into_owned(),
            steamcmd_root: steamcmd.to_string_lossy().into_owned(),
        };
        Self {
            root,
            install,
            settings,
        }
    }

    fn roots(&self) -> [PathBuf; 4] {
        [
            self.install.join("steamapps/workshop/content/322330"),
            self.install.join("ugc_mods/main/Master/content/322330"),
            self.root
                .join("instances/dst/data/ugc/Caves/content/322330"),
            self.install.join("mods"),
        ]
    }

    fn inspect(&self) -> SteamWorkshopInstallationItemStatus {
        inspect_workshop_items_with_dst_ugc_roots(
            &self.settings,
            DST_STEAM_APP_ID,
            &self.install,
            &self.roots()[2..3],
            &[ITEM_ID.to_owned()],
        )
        .unwrap()
        .items
        .into_iter()
        .find(|item| item.item_id == ITEM_ID)
        .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn write_manifest(root: &Path, bytes: usize, installed: &str, available: &str) {
    fs::write(
        root.parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("appworkshop_322330.acf"),
        format!(
            r#""AppWorkshop" {{ "appid" "322330" "WorkshopItemsInstalled" {{ "{ITEM_ID}" {{ "manifest" "{installed}" "size" "{bytes}" }} }} "WorkshopItemDetails" {{ "{ITEM_ID}" {{ "manifest" "{available}" }} }} }}"#
        ),
    )
    .unwrap();
}

#[test]
fn dst_inventory_rejects_empty_and_metadata_free_native_directories() {
    for index in 0..4 {
        let fixture = Fixture::new();
        let name = if index == 3 {
            format!("workshop-{ITEM_ID}")
        } else {
            ITEM_ID.to_owned()
        };
        let item = fixture.roots()[index].join(name);
        fs::create_dir_all(&item).unwrap();
        assert!(!fixture.inspect().installed, "empty source {index}");
        fs::write(item.join("download.tmp"), b"partial download").unwrap();
        assert!(
            !fixture.inspect().installed,
            "no mod metadata in source {index}"
        );
        fs::write(item.join("modinfo.lua"), b"").unwrap();
        assert!(
            !fixture.inspect().installed,
            "empty mod metadata in source {index}"
        );
    }
}

#[test]
fn dst_inventory_discovers_shared_islands_and_volcano_without_accepting_empty_payloads() {
    for shard in &app_core::dst_shards::DST_SHARDS[2..] {
        for cluster in ["main", "another-cluster"] {
            let fixture = Fixture::new();
            let content = fixture
                .install
                .join("ugc_mods")
                .join(cluster)
                .join(shard.directory)
                .join("content/322330");
            let item = content.join(ITEM_ID);
            fs::create_dir_all(&item).unwrap();
            assert!(
                !fixture.inspect().installed,
                "{} empty cache",
                shard.directory
            );
            fs::write(item.join("modinfo.lua"), MODINFO).unwrap();
            let snapshot =
                inspect_workshop_items(&fixture.settings, DST_STEAM_APP_ID, &fixture.install, &[])
                    .unwrap();
            let installed = snapshot
                .items
                .iter()
                .find(|item| item.item_id == ITEM_ID)
                .expect("native IA cache must be discovered without an explicit mod ID");
            assert!(installed.installed, "{} {cluster}", shard.directory);
            assert_eq!(Path::new(&installed.path), item);
            assert!(
                snapshot
                    .searched_roots
                    .iter()
                    .any(|root| Path::new(root) == content)
            );
        }
    }
}

#[test]
fn dst_inventory_requires_complete_manifest_payload_in_steam_and_ugc_caches() {
    for index in 0..3 {
        let fixture = Fixture::new();
        let root = fixture.roots()[index].clone();
        let item = root.join(ITEM_ID);
        fs::create_dir_all(&item).unwrap();
        fs::write(item.join("modinfo.lua"), MODINFO).unwrap();
        write_manifest(&root, MODINFO.len() + 1, "111", "111");
        assert!(!fixture.inspect().installed, "truncated source {index}");
        write_manifest(&root, MODINFO.len(), "111", "222");
        assert!(
            !fixture.inspect().installed,
            "pending update in source {index}"
        );
        write_manifest(&root, MODINFO.len(), "111", "111");
        let status = fixture.inspect();
        assert!(status.installed, "complete source {index}");
        assert_eq!(Path::new(&status.path), item);
        assert_eq!(fs::read(item.join("modinfo.lua")).unwrap(), MODINFO);
    }
}

#[test]
fn dst_inventory_accepts_manifestless_native_payloads_but_not_steam_cache() {
    for index in 0..4 {
        let fixture = Fixture::new();
        let name = if index == 3 {
            format!("workshop-{ITEM_ID}")
        } else {
            ITEM_ID.to_owned()
        };
        let item = fixture.roots()[index].join(name);
        fs::create_dir_all(&item).unwrap();
        fs::write(item.join("modinfo.lua"), MODINFO).unwrap();
        let status = fixture.inspect();
        assert_eq!(status.installed, index != 0, "manifestless source {index}");
        if status.installed {
            assert_eq!(Path::new(&status.path), item);
        }
    }
}

#[test]
fn dst_inventory_skips_empty_primary_and_preserves_native_download_boundary() {
    let fixture = Fixture::new();
    let roots = fixture.roots();
    fs::create_dir_all(roots[0].join(ITEM_ID)).unwrap();
    let native = roots[1].join(ITEM_ID);
    fs::create_dir_all(&native).unwrap();
    fs::write(native.join("modinfo.lua"), MODINFO).unwrap();
    let status = fixture.inspect();
    assert!(status.installed);
    assert_eq!(Path::new(&status.path), native);
    let download = inspect_workshop_item_paths(
        DST_STEAM_APP_ID,
        &roots[0],
        &Path::new(&fixture.settings.steamcmd_root).join("steamapps/workshop/content/322330"),
        vec![ITEM_ID.to_owned()],
    )
    .unwrap();
    assert!(
        !download[0].expected_path_exists,
        "native evidence cannot authorize a SteamCMD download"
    );
}
