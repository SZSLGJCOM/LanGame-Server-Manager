use super::*;

const BEFORE: &str = r#""AppState" { "appid" "42" "StateFlags" "4" "InstalledDepots" { "43" { "manifest" "12345678901234567890" } } "StagedDepots" { "44" { "manifest" "99" } } }"#;
const FAILED: &str = r#""AppState" { "appid" "42" "StateFlags" "6" "InstalledDepots" { "43" { "manifest" "12345678901234567890" } } }"#;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = crate::tests::unique_test_root();
        std::fs::create_dir_all(root.join("steamapps")).unwrap();
        std::fs::write(root.join("steamapps/appmanifest_42.acf"), BEFORE).unwrap();
        std::fs::write(root.join("server.cfg"), "operator configuration").unwrap();
        std::fs::write(root.join("world.sav"), "operator world").unwrap();
        Self(root)
    }
    fn manifest(&self) -> PathBuf {
        self.0.join("steamapps/appmanifest_42.acf")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn native_denials_require_exact_context_and_nonzero_numeric_identifiers() {
    let line = "[2026-10-15 18:42:29] CDepotDownloadMgr::BYldRequestDepotManifest(App: 896660, Depot: 896662, Manifest: 6497701753597013477, branch: ): Failed to get manifest request code, 'Access Denied'";
    assert_eq!(
        manifest_denial(line),
        Some(ManifestDenial {
            app: 896660,
            depot: 896662,
            manifest: 6497701753597013477,
        })
    );
    for invalid in [
        line.replace("896660", "0"),
        line.replace("896660", "4294967296"),
        line.replace("896662", "x"),
        line.replace("6497701753597013477", "18446744073709551616"),
        line.replace("Access Denied", "No connection"),
        line.replace("Failed to get manifest request code", "Download failed"),
        "ERROR! App state is 0x6, No connection, Access Denied".into(),
    ] {
        assert_eq!(manifest_denial(&invalid), None, "{invalid}");
    }
}

#[tokio::test]
async fn only_installed_depots_of_the_attempted_app_can_match() {
    let fixture = Fixture::new();
    let installed = InstalledManifest::before_attempt(&fixture.0, 42)
        .await
        .unwrap();
    let matched = ManifestDenial {
        app: 42,
        depot: 43,
        manifest: 12345678901234567890,
    };
    assert_eq!(installed.matching_denial(&[matched]), Some(matched));
    for denied in [
        ManifestDenial { app: 7, ..matched },
        ManifestDenial {
            depot: 44,
            ..matched
        },
        ManifestDenial {
            manifest: 99,
            ..matched
        },
        ManifestDenial {
            app: 42,
            depot: 44,
            manifest: 99,
        },
    ] {
        assert_eq!(installed.matching_denial(&[denied]), None);
    }
    assert!(
        InstalledManifest::before_attempt(&fixture.0, 7)
            .await
            .is_none()
    );
    std::fs::write(fixture.manifest(), r#""AppState" { "appid" "41" }"#).unwrap();
    assert!(
        InstalledManifest::before_attempt(&fixture.0, 42)
            .await
            .is_none()
    );
}

#[tokio::test]
async fn recovery_preserves_original_and_failed_manifests_without_touching_operator_data() {
    let fixture = Fixture::new();
    let installed = InstalledManifest::before_attempt(&fixture.0, 42)
        .await
        .unwrap();
    std::fs::write(fixture.manifest(), FAILED).unwrap();
    let backup = installed
        .preserve_and_reset(InstallDeadline::new(
            "fixture",
            std::time::Duration::from_secs(10),
        ))
        .await
        .unwrap();
    assert!(!fixture.manifest().exists());
    assert_eq!(
        std::fs::read_to_string(backup.join("before-attempt.acf")).unwrap(),
        BEFORE
    );
    assert_eq!(
        std::fs::read_to_string(backup.join("failed-attempt.acf")).unwrap(),
        FAILED
    );
    assert_eq!(
        std::fs::read_to_string(fixture.0.join("server.cfg")).unwrap(),
        "operator configuration"
    );
    assert_eq!(
        std::fs::read_to_string(fixture.0.join("world.sav")).unwrap(),
        "operator world"
    );
}

#[tokio::test]
async fn cancellation_before_reset_keeps_the_active_manifest() {
    let fixture = Fixture::new();
    let installed = InstalledManifest::before_attempt(&fixture.0, 42)
        .await
        .unwrap();
    let cancellation = InstallCancellation::new();
    cancellation.cancel();
    let result = cancellation
        .scope(installed.preserve_and_reset(InstallDeadline::new(
            "fixture",
            std::time::Duration::from_secs(10),
        )))
        .await;
    assert!(matches!(
        result,
        Err(SteamCmdError::InstallCancelled { .. })
    ));
    assert_eq!(std::fs::read_to_string(fixture.manifest()).unwrap(), BEFORE);
    assert_eq!(
        std::fs::read_dir(fixture.0.join("steamapps"))
            .unwrap()
            .count(),
        1
    );
}

#[tokio::test]
async fn changed_app_identity_prevents_any_reset_or_backup() {
    let fixture = Fixture::new();
    let installed = InstalledManifest::before_attempt(&fixture.0, 42)
        .await
        .unwrap();
    let changed = BEFORE.replace("\"42\"", "\"41\"");
    std::fs::write(fixture.manifest(), &changed).unwrap();
    assert!(installed.reset(|| Ok(())).is_err());
    assert_eq!(
        std::fs::read_to_string(fixture.manifest()).unwrap(),
        changed
    );
    assert_eq!(
        std::fs::read_dir(fixture.0.join("steamapps"))
            .unwrap()
            .count(),
        1
    );
}

#[tokio::test]
async fn cancellation_after_backup_leaves_active_and_recoverable_original() {
    let fixture = Fixture::new();
    let installed = InstalledManifest::before_attempt(&fixture.0, 42)
        .await
        .unwrap();
    let calls = std::cell::Cell::new(0);
    let result = installed.reset(|| {
        calls.set(calls.get() + 1);
        if calls.get() == 2 {
            Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"))
        } else {
            Ok(())
        }
    });
    assert!(result.is_err());
    assert_eq!(std::fs::read_to_string(fixture.manifest()).unwrap(), BEFORE);
    let backup = std::fs::read_dir(fixture.0.join("steamapps"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.is_dir())
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(backup.join("before-attempt.acf")).unwrap(),
        BEFORE
    );
}
