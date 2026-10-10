use super::super::*;
use std::fs;
use std::path::PathBuf;

pub(super) const ID: &str = "world-test";
pub(super) const BUILDING: &str = "Difficulty.Progression.BuildingMaterialCostScale";
pub(super) const FUTURE: &str = "Difficulty.Future.UnknownRule";
pub(super) const FRIENDLY: &str = "Difficulty.Environment.FriendlyFire";
pub(super) const SAVED_AT_TICKS: i64 = 639028224000000000;

fn string(value: &str) -> Vec<u8> {
    let mut result = (value.len() as u32 + 1).to_le_bytes().to_vec();
    result.extend_from_slice(value.as_bytes());
    result.push(0);
    result
}

fn chunk(tag: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut result = tag.to_vec();
    result.extend_from_slice(&(body.len() as u32).to_le_bytes());
    result.extend_from_slice(body);
    result
}

fn table(fields: &[Vec<u8>]) -> Vec<u8> {
    let mut result = (fields.len() as u32).to_le_bytes().to_vec();
    let mut size = 0u32;
    for field in fields {
        result.extend_from_slice(&size.to_le_bytes());
        size += field.len() as u32;
    }
    result.extend_from_slice(&size.to_le_bytes());
    for field in fields {
        result.extend_from_slice(field);
    }
    result
}

fn name_table(values: &[&str]) -> Vec<u8> {
    let mut result = (values.len() as u32).to_le_bytes().to_vec();
    for value in values {
        result.extend_from_slice(&string(value));
    }
    result
}

// A redacted current-format world, with no real player data or packaged assets.
pub(super) fn world_bytes() -> Vec<u8> {
    world_bytes_with_metadata("Fixture World", SAVED_AT_TICKS)
}

pub(super) fn world_bytes_with_metadata(world_name: &str, saved_at_ticks: i64) -> Vec<u8> {
    let cinf_names = [
        "GUID_A",
        "GUID_B",
        "GUID_C",
        "GUID_D",
        "WorldName",
        "SurvivalDifficulty",
        "HardcoreState",
        "TimeOfSave",
        "UnknownInfo",
    ];
    let mut custom = name_table(&cinf_names);
    custom.extend_from_slice(&table(&[
        11u32.to_le_bytes().to_vec(),
        22u32.to_le_bytes().to_vec(),
        33u32.to_le_bytes().to_vec(),
        44u32.to_le_bytes().to_vec(),
        string(world_name),
        0u32.to_le_bytes().to_vec(),
        1u32.to_le_bytes().to_vec(),
        saved_at_ticks.to_le_bytes().to_vec(),
        b"retain-unknown-info".to_vec(),
    ]));
    let versions: Vec<u8> = [522u32, 1017]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    let mut info = 8u16.to_le_bytes().to_vec();
    info.extend_from_slice(&versions);
    info.extend_from_slice(&[0, 0, 0, 0, 255, 0, 0, 0, 0]);
    info.extend_from_slice(&string("2026-01-01T00:00:00.000Z"));
    info.extend_from_slice(&chunk(b"CINF", &custom));
    let property_names = [
        "WorldSaveSettings",
        "WorldName",
        "SurvivalDifficulty",
        "HardcoreState",
        "WorldSaveGuid",
        "CustomDifficultySettings",
        "UnknownObject",
    ];
    let class_name = "/Script/Dominion.PersistenceSubsystem";
    let mut definition = string(class_name);
    definition.extend_from_slice(&6u16.to_le_bytes());
    for (index, kind) in [30u16, 1, 1, 23, 64, 777].into_iter().enumerate() {
        definition.extend_from_slice(&(index as u32 + 1).to_le_bytes());
        definition.extend_from_slice(&0u32.to_le_bytes());
        definition.extend_from_slice(&kind.to_le_bytes());
    }
    let mut wrapped = vec![0];
    wrapped.extend_from_slice(&chunk(b"CDEF", &definition));
    let mut metadata = chunk(b"VERS", &5u32.to_le_bytes());
    metadata.extend_from_slice(&chunk(b"CNIX", &name_table(&[class_name])));
    metadata.extend_from_slice(&chunk(b"CLST", &chunk(b"CDVE", &wrapped)));
    metadata.extend_from_slice(&chunk(b"PNIX", &name_table(&property_names)));
    let guid: Vec<u8> = [11u32, 22, 33, 44]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    let mut object = 0u32.to_le_bytes().to_vec();
    object.extend_from_slice(&string("FixturePersistence"));
    object.extend_from_slice(&0u32.to_le_bytes());
    object.extend_from_slice(&versions);
    object.extend_from_slice(&chunk(
        b"PROP",
        &table(&[
            string(world_name),
            0u16.to_le_bytes().to_vec(),
            1u16.to_le_bytes().to_vec(),
            guid,
            vec![0; 8],
            b"retain-unknown-object".to_vec(),
        ]),
    ));
    let mut global = string("L_World");
    global.extend_from_slice(&chunk(b"META", &metadata));
    global.extend_from_slice(&chunk(b"GOBS", &chunk(b"NOBJ", &object)));
    let mut body = chunk(b"INFO", &info);
    body.extend_from_slice(&chunk(b"GLOB", &global));
    body.extend_from_slice(&chunk(b"LVLS", b"retain-full-world-level-data"));
    chunk(b"SAVE", &body)
}

pub(super) struct Fixture {
    pub(super) root: PathBuf,
    pub(super) paths: StoragePaths,
    pub(super) instance: PathBuf,
    pub(super) saves: PathBuf,
    pub(super) world: PathBuf,
}

impl Fixture {
    pub(super) async fn new(custom: bool) -> Self {
        let root =
            std::env::temp_dir().join(format!("lgsm-dragonwilds-world-{}", uuid::Uuid::new_v4()));
        let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let paths = StoragePaths {
            app_data_root: root.join("appdata"),
            settings_path: root.join("appdata/settings.json"),
            database_path: root.join("appdata/database.sqlite"),
            logs_root: root.join("logs"),
            modules_root: repo.join("modules"),
            migrations_root: repo.join("migrations"),
            steamcmd_root: root.join("steamcmd"),
            games_root: root.join("games"),
            instances_root: root.join("instances"),
            archives_root: root.join("instances/.trash"),
        };
        let instance = paths.instances_root.join(ID);
        let saves = instance.join("runtime/RSDragonwilds/Saved/SaveGames");
        for path in [
            &paths.app_data_root,
            &paths.games_root,
            &instance.join("config"),
            &saves,
        ] {
            fs::create_dir_all(path).unwrap();
        }
        fs::write(
            instance.join("runtime/.langame-private-runtime"),
            b"managed\n",
        )
        .unwrap();
        fs::write(
            instance.join("config/instance.json"),
            br#"{"settings":{"default_world_name":"Fixture World"},"ports":[]}"#,
        )
        .unwrap();
        crate::initialize_database(&paths).await.unwrap();
        let descriptor = app_modules::discover_modules(&paths.modules_root)
            .unwrap()
            .into_iter()
            .find(|m| m.summary.id == "runescapedragonwilds")
            .unwrap();
        crate::sync_modules(&paths, &[descriptor]).await.unwrap();
        let pool = connect_pool(&paths).await.unwrap();
        sqlx::query("INSERT INTO instances(id,name,module_id,data_path,config_path,logs_path,saves_path,runtime_mode,status) VALUES(?1,'Fixture','runescapedragonwilds',?2,?3,?4,?5,'independent','stopped')")
            .bind(ID).bind(instance.join("data").to_string_lossy().as_ref())
            .bind(instance.join("config").to_string_lossy().as_ref()).bind(instance.join("logs").to_string_lossy().as_ref())
            .bind(saves.to_string_lossy().as_ref()).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO game_installs(module_id,install_root,install_state,scope,owner_instance_id) VALUES('runescapedragonwilds',?1,'installed','instance',?2)")
            .bind(instance.join("runtime").to_string_lossy().as_ref()).bind(ID).execute(&pool).await.unwrap();
        sqlx::query("UPDATE instances SET install_id=(SELECT id FROM game_installs WHERE owner_instance_id=?1) WHERE id=?1")
            .bind(ID).execute(&pool).await.unwrap();
        pool.close().await;
        let world = saves.join("Fixture.sav");
        let bytes = if custom {
            patch_world_settings(
                &world_bytes(),
                3,
                &BTreeMap::from([(FUTURE.to_owned(), 2.25)]),
            )
            .unwrap()
        } else {
            world_bytes()
        };
        fs::write(&world, bytes).unwrap();
        Self {
            root,
            paths,
            instance,
            saves,
            world,
        }
    }

    pub(super) fn context(&self) -> WorldContext {
        WorldContext {
            saves: self.saves.clone(),
            writable: true,
            default_world_name: "Fixture World".into(),
        }
    }

    pub(super) fn input(
        &self,
        mode: DragonwildsWorldMode,
        values: &[(&str, f64)],
    ) -> WriteDragonwildsWorldSettingsInput {
        WriteDragonwildsWorldSettingsInput {
            instance_id: ID.to_owned(),
            world_file: "Fixture.sav".into(),
            expected_revision: sha256(&fs::read(&self.world).unwrap()),
            world_mode: mode,
            values: values
                .iter()
                .map(|(tag, value)| ((*tag).to_owned(), *value))
                .collect(),
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        assert_eq!(self.root.parent(), Some(std::env::temp_dir().as_path()));
        fs::remove_dir_all(&self.root).unwrap();
    }
}
