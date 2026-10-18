//! Controlled boundary injections at copy completion and moved-original checks;
//! no sleep, external account, native server, or global filesystem mutation.
use super::*;

const ID: &str = "123456";

struct Fixture {
    root: PathBuf,
    target: ResolvedManualModTarget,
    downloaded: SteamWorkshopDownloadResult,
}

impl Fixture {
    fn new(unchanged: bool) -> Self {
        let root = std::env::temp_dir().join(format!(
            "lg-workshop-stage-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let source = root.join("source").join(ID);
        let actual = root.join("actual").join(ID);
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&actual).unwrap();
        fs::write(actual.join("old.pak"), b"original").unwrap();
        if unchanged {
            fs::write(source.join("old.pak"), b"original").unwrap();
        } else {
            fs::write(source.join("new.pak"), b"native latest").unwrap();
        }
        Self {
            root: root.clone(),
            target: ResolvedManualModTarget {
                instance_id: "guard-fixture".into(),
                module_id: "palworld".into(),
                source_label: "Steam Workshop".into(),
                target_label: "Mods".into(),
                target_path: root.join("actual"),
                accepts: Vec::new(),
                id_strategy: None,
            },
            downloaded: SteamWorkshopDownloadResult {
                consumer_app_id: 1623730,
                install_root: root.join("game").to_string_lossy().into_owned(),
                workshop_root: root.join("source").to_string_lossy().into_owned(),
                items: vec![app_steamcmd::SteamWorkshopDownloadItemResult {
                    item_id: ID.into(),
                    expected_path: source.to_string_lossy().into_owned(),
                    expected_path_exists: true,
                }],
                output_excerpt: "controlled native download boundary fixture".into(),
            },
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        assert_eq!(self.root.parent(), Some(std::env::temp_dir().as_path()));
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn workshop_start_copy_completion_guard_preserves_configuration_added_during_staging() {
    let fixture = Fixture::new(false);
    let cancel = app_steamcmd::InstallCancellation::new();
    let actual = fixture.target.target_path.join(ID);
    let baseline = app_steamcmd::WorkshopDeploymentBaseline::capture(ID, &actual, &cancel).unwrap();
    let before_commit = || {
        // This seam runs after the real staging copy, before the actual rename.
        let transaction = fs::read_dir(&fixture.root)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(".langame-mod-transaction-")
            })
            .unwrap();
        assert_eq!(
            fs::read(transaction.join("staging").join(ID).join("new.pak")).unwrap(),
            b"native latest"
        );
        fs::create_dir(actual.join("Config")).unwrap();
        fs::write(
            actual.join("Config/user.ini"),
            b"keep data added while copying",
        )
        .unwrap();
        baseline
            .verify(&actual, &cancel)
            .map_err(|error| error.to_string())
    };
    let error = stage_downloaded_workshop_items_with_preconditions(
        &fixture.downloaded,
        &fixture.target,
        &before_commit,
        &|_| Ok(()),
    )
    .unwrap_err();
    assert!(error.guard_failed);
    assert!(error.message.contains("deployment changed"));
    assert_eq!(fs::read(actual.join("old.pak")).unwrap(), b"original");
    assert!(!actual.join("new.pak").exists());
    assert_eq!(
        fs::read(actual.join("Config/user.ini")).unwrap(),
        b"keep data added while copying"
    );
    assert!(fs::read_dir(&fixture.root).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".langame-mod-")
    }));
}

#[test]
fn workshop_start_changed_moved_original_is_preserved_and_never_authorizes_fallback() {
    let fixture = Fixture::new(true);
    let cancel = app_steamcmd::InstallCancellation::new();
    let actual = fixture.target.target_path.join(ID);
    let baseline = app_steamcmd::WorkshopDeploymentBaseline::capture(ID, &actual, &cancel).unwrap();
    let original_path = std::cell::RefCell::new(None);
    let before_cleanup = |backup: &Path| {
        let original = backup.join(ID);
        fs::create_dir(original.join("Config")).unwrap();
        fs::write(
            original.join("Config/user.ini"),
            b"keep data added before original cleanup",
        )
        .unwrap();
        original_path.replace(Some(original.clone()));
        baseline
            .verify_moved_original(&original, &cancel)
            .map_err(|error| error.to_string())
    };
    let error = stage_downloaded_workshop_items_with_preconditions(
        &fixture.downloaded,
        &fixture.target,
        &|| {
            baseline
                .verify(&actual, &cancel)
                .map_err(|error| error.to_string())
        },
        &before_cleanup,
    )
    .unwrap_err();
    assert!(error.guard_failed);
    assert!(
        error
            .message
            .contains("preserved original Workshop deployment")
    );
    // Incoming bytes intentionally equal the old baseline. That equality must
    // never turn a changed original/cleanup guard into a cached-start success.
    baseline.verify(&actual, &cancel).unwrap();
    let original = original_path.into_inner().unwrap();
    assert!(original.exists());
    assert_eq!(fs::read(original.join("old.pak")).unwrap(), b"original");
    assert_eq!(
        fs::read(original.join("Config/user.ini")).unwrap(),
        b"keep data added before original cleanup"
    );
    assert_eq!(fs::read(actual.join("old.pak")).unwrap(), b"original");
}
