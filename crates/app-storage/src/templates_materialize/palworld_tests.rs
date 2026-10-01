use super::*;

const SECTION: &str = "[/Script/Pal.PalGameWorldSettings]";

struct Fixture {
    root: PathBuf,
    paths: StoragePaths,
    install: PathBuf,
    config: PathBuf,
    live: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("palworld-ini-{}", uuid::Uuid::new_v4()));
        let paths = StoragePaths {
            app_data_root: root.join("appdata"),
            settings_path: root.join("appdata/settings.json"),
            database_path: root.join("appdata/db/lgs.db"),
            logs_root: root.join("appdata/logs"),
            modules_root: root.join("modules"),
            migrations_root: root.join("migrations"),
            steamcmd_root: root.join("steamcmd"),
            games_root: root.join("games"),
            instances_root: root.join("instances"),
            archives_root: root.join("instances/.trash"),
        };
        let install = root.join("install");
        let config = paths.instances_root.join("selected/config");
        let live = install.join("Pal/Saved/Config/WindowsServer");
        fs::create_dir_all(&config).unwrap();
        fs::create_dir_all(&live).unwrap();
        fs::write(
            config.join(PALWORLD_GAME_USER_SETTINGS_FILE),
            "[Server]\nName=new\n",
        )
        .unwrap();
        fs::write(
            live.join(PALWORLD_GAME_USER_SETTINGS_FILE),
            "[Server]\nName=original\n",
        )
        .unwrap();
        fs::write(config.join(PALWORLD_WORLD_SETTINGS_FILE),
            format!("{SECTION}\nOptionSettings=(ServerName=\"new\",DeathPenalty=All,CrossplayPlatforms=(Steam,Xbox))\n")).unwrap();
        Self {
            root,
            paths,
            install,
            config,
            live,
        }
    }

    fn world(&self) -> PathBuf {
        self.live.join(PALWORLD_WORLD_SETTINGS_FILE)
    }

    fn apply(&self) -> Result<(), StorageError> {
        super::super::materialize_module_support_files(&ModuleSupportMaterializationContext {
            storage_paths: &self.paths,
            module_id: "palworld",
            install_root: &self.install,
            shared_install_root: &self.install,
            config_dir: &self.config,
            saves_dir: &self.root,
            instance_id: "selected",
            instance_running: false,
            settings: &Map::new(),
        })
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn palworld_preserves_unmanaged_options_and_other_ini_content() {
    let fixture = Fixture::new();
    let original = format!(
        "\u{feff}; operator comment\r\n{SECTION}\r\nOptionSettings=(ServerName=\"old\",Difficulty=Hard,CoopPlayerMaxNum=8,bIsMultiplay=True,bEnableDefenseOtherGuildPlayer=True,bEnableNonLoginPenalty=False,ModOption=(Name=\"a,b=\\\"c\\\"\",Values=(1,2)),DeathPenalty=Item)\r\nKeepEntry=42\r\n[Other]\r\nUnmanaged=yes\r\n"
    );
    fs::write(fixture.world(), original).unwrap();
    fixture.apply().unwrap();
    let merged = fs::read_to_string(fixture.world()).unwrap();
    for fragment in [
        "; operator comment",
        "Difficulty=Hard",
        "CoopPlayerMaxNum=8",
        "bIsMultiplay=True",
        "bEnableDefenseOtherGuildPlayer=True",
        "bEnableNonLoginPenalty=False",
        "ModOption=(Name=\"a,b=\\\"c\\\"\",Values=(1,2))",
        "KeepEntry=42",
        "[Other]\nUnmanaged=yes",
        "ServerName=\"new\"",
        "DeathPenalty=All",
        "CrossplayPlatforms=(Steam,Xbox)",
    ] {
        assert!(merged.contains(fragment), "missing {fragment} in {merged}");
    }
    assert!(merged.starts_with('\u{feff}'));
    assert!(!merged.contains("ServerName=\"old\""));
    assert!(!merged.contains("DeathPenalty=Item"));
    fixture.apply().unwrap();
    assert_eq!(fs::read_to_string(fixture.world()).unwrap(), merged);
}

#[test]
fn palworld_rejects_ambiguous_option_maps_without_changing_support_files() {
    let fixture = Fixture::new();
    let invalid = [
        format!("{SECTION}\nOptionSettings=(X=1,x=2)\n").into_bytes(),
        format!("{SECTION}\nOptionSettings=(X=\"unterminated)\n").into_bytes(),
        format!("{SECTION}\nOptionSettings=(X=(1,2)\n").into_bytes(),
        format!(
            "{SECTION}\nOptionSettings=(X={}1{})\n",
            "(".repeat(65),
            ")".repeat(65)
        )
        .into_bytes(),
        format!(
            "{SECTION}\nOptionSettings=({})\n",
            (0..4097)
                .map(|index| format!("Field{index}=1"))
                .collect::<Vec<_>>()
                .join(",")
        )
        .into_bytes(),
        format!("{SECTION}\nOptionSettings=(X=1)\nOptionSettings=(Y=2)\n").into_bytes(),
        format!("{SECTION}\nOptionSettings=(X=1)\n{SECTION}\nOptionSettings=(Y=2)\n").into_bytes(),
        vec![0xff, 0xfe, 0x00],
        vec![b' '; 256 * 1024 + 1],
    ];
    for original in invalid {
        fs::write(fixture.world(), &original).unwrap();
        assert!(
            fixture.apply().is_err(),
            "invalid native map must not be replaced"
        );
        assert_eq!(fs::read(fixture.world()).unwrap(), original);
        assert_eq!(
            fs::read(fixture.live.join(PALWORLD_GAME_USER_SETTINGS_FILE)).unwrap(),
            b"[Server]\nName=original\n"
        );
        assert!(
            !fixture
                .install
                .join("Pal/Binaries/Win64/Mods/PalModSettings.ini")
                .exists()
        );
    }
}

#[test]
fn palworld_fresh_native_output_matches_the_managed_document() {
    let fixture = Fixture::new();
    fixture.apply().unwrap();
    assert_eq!(
        fs::read(fixture.world()).unwrap(),
        fs::read(fixture.config.join(PALWORLD_WORLD_SETTINGS_FILE)).unwrap()
    );
}

#[test]
fn palworld_rolls_back_native_merge_if_later_mod_preparation_fails() {
    let fixture = Fixture::new();
    let original = format!("{SECTION}\nOptionSettings=(Difficulty=Hard,FutureOption=keep)\n");
    fs::write(fixture.world(), &original).unwrap();
    let mod_root = fixture.install.join("Pal/Binaries/Win64/Mods");
    fs::create_dir_all(mod_root.parent().unwrap()).unwrap();
    fs::write(&mod_root, b"obstructed directory").unwrap();
    assert!(fixture.apply().is_err());
    assert_eq!(fs::read_to_string(fixture.world()).unwrap(), original);
    assert_eq!(
        fs::read(fixture.live.join(PALWORLD_GAME_USER_SETTINGS_FILE)).unwrap(),
        b"[Server]\nName=original\n"
    );
    assert_eq!(fs::read(mod_root).unwrap(), b"obstructed directory");
}

#[test]
fn palworld_rejects_a_plan_if_native_settings_changed_after_reading() {
    let fixture = Fixture::new();
    fs::write(
        fixture.world(),
        format!("{SECTION}\nOptionSettings=(FutureOption=before)\n"),
    )
    .unwrap();
    let plan = palworld::plan_world_settings(
        &fixture.config.join(PALWORLD_WORLD_SETTINGS_FILE),
        &fixture.world(),
    )
    .unwrap();
    let intervening = format!("{SECTION}\nOptionSettings=(FutureOption=external_edit)\n");
    fs::write(fixture.world(), &intervening).unwrap();
    let mut mutation = ManagedConfigMutation::new("palworld");
    assert!(mutation.apply(vec![plan]).is_err());
    assert_eq!(fs::read_to_string(fixture.world()).unwrap(), intervening);
}
