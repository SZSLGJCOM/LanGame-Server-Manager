//! Exercise the optional loader through the following save-startup boundary.
use super::*;
use crate::commands::{
    commands_dragonwilds_player_control, commands_dragonwilds_player_query, commands_save_startup,
};
use crate::state::{DesktopState, RuntimeStartReservationAttempt};
use app_core::InstanceDetails;
use app_storage::{StorageBootstrap, StoragePaths, bootstrap_storage_with_paths};

struct Startup {
    program: Fixture,
    root: PathBuf,
    storage: StorageBootstrap,
    details: InstanceDetails,
}

impl Startup {
    fn new() -> Self {
        let program = Fixture::new();
        let root = std::env::temp_dir().join(format!(
            "lgsm-dragonwilds-optional-start-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let instance = root.join("instances/dragon");
        fs::create_dir_all(instance.join("runtime")).unwrap();
        let write = |path: PathBuf, value: serde_json::Value| {
            fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
        };
        write(
            program.root.join(".langame-program-identity.json"),
            serde_json::json!({
                "version":1, "id":"dragon-program", "module_id":"runescapedragonwilds"
            }),
        );
        write(
            instance.join("runtime/.langame-exclusive-program.json"),
            serde_json::json!({
                "version":1, "program_root":program.root, "program_id":"dragon-program",
                "module_id":"runescapedragonwilds"
            }),
        );
        let storage = bootstrap_storage_with_paths(StoragePaths {
            app_data_root: root.join("app"),
            settings_path: root.join("app/settings.json"),
            database_path: root.join("app/db/test.db"),
            logs_root: root.join("logs"),
            modules_root: root.join("modules"),
            migrations_root: root.join("migrations"),
            steamcmd_root: root.join("steamcmd"),
            games_root: root.join("games"),
            instances_root: root.join("instances"),
            archives_root: root.join("instances/.trash"),
        })
        .unwrap();
        let details = serde_json::from_value(serde_json::json!({
            "summary":{"id":"dragon","name":"Dragon","module_id":"runescapedragonwilds",
                "status":"Stopped","bind_ip":"127.0.0.1","port_count":0,"autostart":false},
            "config_file_path":instance.join("config/instance.json"),
            "saves_path":instance.join("data/worlds"), "auto_backup_on_stop":false,
            "backup_retention_count":0, "settings_json":"{}", "ports":[], "active_run":null
        }))
        .unwrap();
        Self {
            program,
            root,
            storage,
            details,
        }
    }

    async fn prepare(&self) -> Result<commands_save_startup::SavePreparation, String> {
        let state = DesktopState::default();
        let reservation = match state.try_reserve_runtime_start("dragon", "manual")? {
            RuntimeStartReservationAttempt::Reserved(value) => value,
            other => panic!("unexpected reservation: {other:?}"),
        };
        let log = self.root.join("startup.log");
        commands_dragonwilds_player_query::prepare(&self.details, &reservation, Some(&log)).await?;
        commands_dragonwilds_player_control::prepare(&self.details, Some(&log)).await?;
        commands_save_startup::ensure_for_start(
            &self.storage,
            &self.details,
            &reservation,
            log.to_str(),
        )
        .await
    }

    fn assert_optional_save_log(&self) {
        let log = fs::read_to_string(self.root.join("startup.log")).unwrap();
        assert!(log.contains("手动保存") && log.contains("不可用"), "{log}");
        assert!(!log.contains("保存扩展及加载器检查完成"), "{log}");
    }
}

impl Drop for Startup {
    fn drop(&mut self) {
        assert_eq!(self.root.parent(), Some(std::env::temp_dir().as_path()));
        assert!(
            self.root
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("lgsm-dragonwilds-optional-start-")
        );
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[tokio::test]
async fn optional_save_start_preserves_native_launch_after_owned_loader_retirement() {
    let fixture = Startup::new();
    fixture.program.install().unwrap();
    // An upgraded game must retire the owned proxy before touching the old
    // loader. Use the official ownership metadata with unexecuted file fixtures.
    let mut owner = ownership(&fixture.program.root, RECEIPT).unwrap().unwrap();
    for (name, _, hash) in RELEASE {
        owner.files.insert((*name).into(), (*hash).into());
    }
    fs::write(
        fixture.program.root.join(RECEIPT),
        serde_json::to_vec(&owner).unwrap(),
    )
    .unwrap();
    fs::write(
        fixture
            .program
            .root
            .join(WIN64)
            .join("RSDragonwildsServer-Win64-Shipping.exe"),
        b"MZupdated-unknown-game",
    )
    .unwrap();
    let control = fixture.program.root.join("langame_player_control");
    fs::create_dir_all(&control).unwrap();
    let session = control.join("save-session.txt");
    fs::write(&session, b"0123456789abcdef0123456789abcdef").unwrap();
    let old_core = fs::read(fixture.program.loader().join("UE4SS.dll")).unwrap();
    assert!(fixture.program.proxy().exists());

    let result = fixture.prepare().await;
    assert!(
        !fixture.program.proxy().exists(),
        "unknown game must disable only its owned proxy"
    );
    assert_eq!(
        fs::read(fixture.program.loader().join("UE4SS.dll")).unwrap(),
        old_core
    );
    assert_eq!(
        fs::read(&session).unwrap(),
        b"0123456789abcdef0123456789abcdef"
    );
    assert!(
        result
            .as_ref()
            .is_ok_and(|value| value.environment.is_empty() && !value.available),
        "optional extension blocked native launch: {result:?}"
    );
    fixture.assert_optional_save_log();
}

#[tokio::test]
async fn optional_save_start_preserves_unknown_and_external_loader_launches() {
    for external in [false, true] {
        let fixture = Startup::new();
        let proxy = fixture.program.proxy();
        if external {
            fs::write(&proxy, b"operator-loader").unwrap();
        }
        let result = fixture.prepare().await;
        assert!(
            result
                .as_ref()
                .is_ok_and(|value| value.environment.is_empty() && !value.available),
            "external={external}: {result:?}"
        );
        if external {
            assert_eq!(fs::read(&proxy).unwrap(), b"operator-loader");
        }
        assert!(
            !fixture
                .program
                .root
                .join("langame_player_control/save-session.txt")
                .exists()
        );
        fixture.assert_optional_save_log();
    }
}

#[tokio::test]
async fn optional_save_start_keeps_invalid_ownership_as_an_error() {
    let fixture = Startup::new();
    let receipt = fixture.program.root.join(RECEIPT);
    fs::write(&receipt, b"invalid ownership").unwrap();
    let state = DesktopState::default();
    let operation = state
        .begin_storage_context_operation("save admission regression")
        .unwrap();
    assert!(
        commands_save_startup::ensure_for_start(
            &fixture.storage,
            &fixture.details,
            &operation,
            None,
        )
        .await
        .is_err()
    );
    assert_eq!(fs::read(receipt).unwrap(), b"invalid ownership");
}
