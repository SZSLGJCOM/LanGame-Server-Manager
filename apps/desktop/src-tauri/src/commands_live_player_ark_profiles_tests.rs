use super::*;
use crate::live_players::{ark_profile::tests::asa_profile, response_codecs};

const ACCOUNT: &str = "0123456789abcdef0123456789abcdef";
const SECOND: &str = "0123456789abcdef0123456789abcdee";

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("langame-ark-profile-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("runtime")).unwrap();
        fs::write(root.join("runtime/.langame-private-runtime"), b"managed\n").unwrap();
        Self(root)
    }

    fn details(&self) -> InstanceDetails {
        serde_json::from_value(serde_json::json!({
            "summary": {
                "id": "fixture", "name": "Native profile fixture", "module_id": "arksurvivalascended",
                "status": "Running", "active_process_count": 0, "bind_ip": "127.0.0.1",
                "port_count": 0, "autostart": false
            },
            "config_file_path": self.0.join("config/settings.json"),
            "saves_path": self.0.join("runtime/ShooterGame/Saved/fixture"),
            "auto_backup_on_stop": false, "backup_retention_count": 1,
            "settings_json": serde_json::json!({"map_name": "TheIsland_WP", "additional_maps": [
                {"id": "desert", "name": "Desert", "map_name": "ScorchedEarth_WP", "enabled": true}
            ]}).to_string(),
            "ports": [], "active_run": null
        })).unwrap()
    }

    fn directory(&self, key: &str) -> PathBuf {
        let map = app_core::ark_maps::processes(&self.details())
            .unwrap()
            .into_iter()
            .find(|map| map.process_key == key)
            .unwrap();
        profile_directory(&self.0.join("runtime"), &map, Edition::Asa)
    }

    fn write(&self, key: &str, account: &str, bytes: &[u8]) -> PathBuf {
        let directory = self.directory(key);
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join(format!("{account}.arkprofile"));
        fs::write(&path, bytes).unwrap();
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn roster(text: &str) -> response_codecs::ParsedPlayerList {
    response_codecs::parse(
        app_core::ModulePlayerListCodec::ArkListPlayers,
        text,
        "0123456789abcdef0123456789abcdef",
        1000,
        &["kick_player".into(), "ban_player".into()],
    )
    .unwrap()
}

#[tokio::test]
async fn native_profiles_restore_names_without_extension_or_rcon() {
    let fixture = Fixture::new();
    let bytes = asa_profile(ACCOUNT, "LanGame玩家 🚀");
    let path = fixture.write("main", ACCOUNT, &bytes);
    fixture.write("main", SECOND, &asa_profile(SECOND, "Offline saved player"));
    let mut native = roster(&format!("0. LanGame?? ?, {ACCOUNT}"));
    let original = native.entries[0].clone();
    let bindings = native.bindings.clone();

    // No DLL, RCON listener, credentials or active extension target exist.
    // This calls the production async enrichment through real filesystem I/O.
    assert!(
        super::super::supplement(&fixture.details(), None, "unused", &mut native.entries)
            .await
            .is_none()
    );
    assert_eq!(native.entries[0].display_name, "LanGame玩家 🚀");
    assert_eq!(native.entries.len(), 1);
    assert_eq!(native.current_players, Some(1));
    assert_eq!(native.entries[0].player_key, original.player_key);
    assert_eq!(
        serde_json::to_value(&native.entries[0].identifiers).unwrap(),
        serde_json::to_value(original.identifiers).unwrap()
    );
    assert_eq!(
        native.entries[0].available_action_ids,
        original.available_action_ids
    );
    assert_eq!(native.bindings, bindings);
    assert_eq!(fs::read(path).unwrap(), bytes);
    assert!(
        !fixture
            .0
            .join("runtime/ShooterGame/Binaries/Win64/ArkApi")
            .exists()
    );
}

#[tokio::test]
async fn native_names_follow_the_selected_map_without_reading_other_maps_or_backups() {
    let fixture = Fixture::new();
    fixture.write("main", ACCOUNT, &asa_profile(ACCOUNT, "岛屿账户"));
    let desert = fixture.write("map-desert", ACCOUNT, &asa_profile(ACCOUNT, "沙漠账户"));
    let mut native = roster(&format!("0. ????, {ACCOUNT}"));
    assert!(
        super::super::supplement(
            &fixture.details(),
            Some("map-desert"),
            "unused",
            &mut native.entries
        )
        .await
        .is_none()
    );
    assert_eq!(native.entries[0].display_name, "沙漠账户");
    fs::rename(&desert, desert.with_extension("profilebak")).unwrap();
    let mut native = roster(&format!("0. ????, {ACCOUNT}"));
    assert!(
        super::super::supplement(
            &fixture.details(),
            Some("map-desert"),
            "unused",
            &mut native.entries
        )
        .await
        .is_some()
    );
    assert_eq!(native.entries[0].display_name, "????");
}

#[tokio::test]
async fn a_damaged_or_foreign_profile_does_not_block_other_accounts_or_invent_a_name() {
    let fixture = Fixture::new();
    fixture.write("main", ACCOUNT, &asa_profile(ACCOUNT, "完整账户"));
    for bytes in [
        b"truncated".to_vec(),
        asa_profile(ACCOUNT, "Wrong internal identity"),
    ] {
        fixture.write("main", SECOND, &bytes);
        let mut native = roster(&format!("0. ????, {ACCOUNT}\n1. ????, {SECOND}"));
        let issue =
            super::super::supplement(&fixture.details(), None, "unused", &mut native.entries)
                .await
                .unwrap();
        assert_eq!(
            issue.code,
            app_core::RuntimeLivePlayerIssueCode::NamesUnavailable
        );
        assert_eq!(native.entries[0].display_name, "完整账户");
        assert_eq!(native.entries[1].display_name, "????");
        assert_eq!(native.current_players, Some(2));
        assert_eq!(native.bindings.len(), 4);
    }
}

#[test]
fn profile_reads_are_bounded_and_reject_account_path_injection() {
    let fixture = Fixture::new();
    let path = fixture.write("main", ACCOUNT, &asa_profile(ACCOUNT, "中文"));
    let directory = fixture.directory("main");
    let mut budget = MAX_COLLECTION_BYTES;
    for account in [
        "../outside",
        "account/../../../outside",
        "",
        "0123456789abcdef0123456789abcdef:stream",
    ] {
        assert!(read_name(&directory, Edition::Asa, account, &mut budget).is_err());
    }
    let mut small_budget = 1;
    assert!(read_name(&directory, Edition::Asa, ACCOUNT, &mut small_budget).is_err());
    OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap()
        .set_len(MAX_PROFILE_BYTES as u64 + 1)
        .unwrap();
    assert!(read_name(&directory, Edition::Asa, ACCOUNT, &mut budget).is_err());
    assert!(
        read_name(&directory, Edition::Asa, SECOND, &mut budget)
            .unwrap()
            .is_none()
    );
}

#[test]
fn ase_and_asa_profile_paths_follow_their_native_save_layouts() {
    let fixture = Fixture::new();
    let runtime = fixture.0.join("runtime");
    let map = app_core::ark_maps::processes(&fixture.details())
        .unwrap()
        .remove(0);
    assert_eq!(
        profile_directory(&runtime, &map, Edition::Ase),
        runtime.join("ShooterGame/Saved/fixture")
    );
    assert_eq!(
        profile_directory(&runtime, &map, Edition::Asa),
        runtime.join("ShooterGame/Saved/fixture/TheIsland_WP")
    );
}
