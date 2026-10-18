use super::super::super::{ManagedModState, ManualModInventoryItem};
use super::*;

const APP: u32 = 1623730;
const ID: &str = "123456789";

struct Fixture {
    root: PathBuf,
    install: PathBuf,
    native: PathBuf,
    settings: app_core::AppSettings,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lg-workshop-missing-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let install = root.join("game");
        let native = root.join("steamcmd");
        fs::create_dir_all(&install).unwrap();
        fs::create_dir_all(&native).unwrap();
        Self {
            settings: app_core::AppSettings {
                steamcmd_root: native.to_string_lossy().into_owned(),
                ..Default::default()
            },
            root,
            install,
            native,
        }
    }

    fn seed(&self, base: &Path, entries: &[(&str, &str)]) -> SteamWorkshopDownloadResult {
        let content = base.join(format!("steamapps/workshop/content/{APP}"));
        let mut installed = String::new();
        let mut items = Vec::new();
        for (id, name) in entries {
            let path = content.join(id);
            fs::create_dir_all(&path).unwrap();
            let info = serde_json::to_vec(&json!({"PackageName":name})).unwrap();
            fs::write(path.join("Info.json"), &info).unwrap();
            fs::write(path.join("old.pak"), "cached").unwrap();
            installed.push_str(&format!(
                r#""{id}" {{ "manifest" "9" "size" "{}" }}"#,
                info.len() + 6
            ));
            items.push(app_steamcmd::SteamWorkshopDownloadItemResult {
                item_id: (*id).into(),
                expected_path: path.to_string_lossy().into_owned(),
                expected_path_exists: true,
            });
        }
        fs::write(
            base.join(format!("steamapps/workshop/appworkshop_{APP}.acf")),
            format!(
                r#""AppWorkshop" {{ "appid" "{APP}" "WorkshopItemsInstalled" {{ {installed} }} }}"#
            ),
        )
        .unwrap();
        SteamWorkshopDownloadResult {
            consumer_app_id: APP,
            install_root: self.install.to_string_lossy().into_owned(),
            workshop_root: content.to_string_lossy().into_owned(),
            items,
            output_excerpt: "boundary fixture: prior native validate success".into(),
        }
    }

    fn request(&self) -> Request {
        Request {
            settings: self.settings.clone(),
            root: self.install.clone(),
            app: APP,
            wanted: BTreeSet::from(["fixturemod".into()]),
            removed: BTreeSet::new(),
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        assert_eq!(self.root.parent(), Some(std::env::temp_dir().as_path()));
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[tokio::test]
async fn missing_package_uses_only_instance_membership_and_hash_verified_matching_revision()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new();
    let orphan = fixture.root.join("unrelated-instance");
    fs::create_dir(&orphan)?;
    let locks = app_steamcmd::InstallLockContext::new(fixture.root.join("locks"));
    let install = app_steamcmd::acquire_game_install_lifecycle(
        &locks,
        "palworld",
        &[fixture.install.clone(), orphan.clone()],
    )
    .await?;
    let steamcmd = install.acquire_steamcmd(&fixture.settings).await?;
    let cancellation = app_steamcmd::InstallCancellation::new();
    // Both revisions are intact. Only the shared-cache revision has the
    // configured PackageName; primary directory order must not override it.
    for result in [
        fixture.seed(&fixture.install, &[(ID, "RenamedFixtureMod")]),
        fixture.seed(&fixture.native, &[(ID, "FixtureMod")]),
    ] {
        app_steamcmd::record_validated_workshop_items(
            &result,
            &install,
            &steamcmd,
            &fixture.settings,
            &cancellation,
        )?;
    }
    assert_eq!(
        resolve_locked(&install, &steamcmd, &fixture.request(), &cancellation)?,
        [ID]
    );
    fs::remove_dir_all(fixture.install.join("steamapps"))?;
    // A private native-success record still permits the shared payload.
    assert_eq!(
        resolve_locked(&install, &steamcmd, &fixture.request(), &cancellation)?,
        [ID]
    );
    let mut request = fixture.request();
    request.root = orphan;
    assert!(
        resolve_locked(&install, &steamcmd, &request, &cancellation)
            .unwrap_err()
            .contains("属于该实例"),
        "a global same-name package is never instance membership"
    );
    request = fixture.request();
    request.removed.insert(ID.into());
    assert!(
        resolve_locked(&install, &steamcmd, &request, &cancellation).is_err(),
        "removed or disabled IDs cannot be re-adopted"
    );
    fs::write(
        fixture
            .native
            .join(format!("steamapps/workshop/content/{APP}/{ID}/old.pak")),
        "BROKEN",
    )?;
    assert!(
        resolve_locked(&install, &steamcmd, &fixture.request(), &cancellation).is_err(),
        "same-size corruption cannot reconstruct missing deployment"
    );
    cancellation.cancel();
    assert!(resolve_locked(&install, &steamcmd, &fixture.request(), &cancellation).is_err());
    Ok(())
}

#[tokio::test]
async fn missing_package_with_two_owned_authors_is_ambiguous()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new();
    let locks = app_steamcmd::InstallLockContext::new(fixture.root.join("locks"));
    let install = app_steamcmd::acquire_game_install_lifecycle(
        &locks,
        "palworld",
        std::slice::from_ref(&fixture.install),
    )
    .await?;
    let steamcmd = install.acquire_steamcmd(&fixture.settings).await?;
    let cancellation = app_steamcmd::InstallCancellation::new();
    let result = fixture.seed(
        &fixture.install,
        &[(ID, "FixtureMod"), ("234567890", "FixtureMod")],
    );
    app_steamcmd::record_validated_workshop_items(
        &result,
        &install,
        &steamcmd,
        &fixture.settings,
        &cancellation,
    )?;
    assert!(
        resolve_locked(&install, &steamcmd, &fixture.request(), &cancellation)
            .unwrap_err()
            .contains("多个")
    );
    Ok(())
}

#[test]
fn identified_local_or_disabled_package_does_not_authorize_same_name_cache_adoption() {
    let mut inventory = ManualModInventoryResult {
        instance_id: "one".into(),
        module_id: "palworld".into(),
        source_label: "Workshop".into(),
        target_label: "Mods".into(),
        target_path: "unused".into(),
        target_exists: true,
        items: Vec::new(),
    };
    let settings = json!({"mod_package_names":"FixtureMod"});
    assert_eq!(
        missing_names(&settings, &inventory).unwrap(),
        BTreeSet::from(["fixturemod".into()])
    );
    inventory.items.push(ManualModInventoryItem {
        name: "LocalManualPackage".into(),
        path: "unused/local".into(),
        item_type: "directory".into(),
        inferred_id: Some("FixtureMod".into()),
        file_count: 0,
        total_bytes: 0,
        modified_unix_ms: None,
        management: None,
    });
    assert!(missing_names(&settings, &inventory).unwrap().is_empty());
    // A local/manual partial package and disabled management intent are already
    // identified; neither is permission to replace it with a Workshop author.
    inventory.items[0].management = Some(ManagedModState {
        package_id: "local".into(),
        enabled: false,
        dependencies: Vec::new(),
        dependents: Vec::new(),
        dependency_status: "unknown".into(),
    });
    assert!(missing_names(&settings, &inventory).unwrap().is_empty());
}
