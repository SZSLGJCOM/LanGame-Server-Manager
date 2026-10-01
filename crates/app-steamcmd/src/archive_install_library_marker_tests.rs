use super::*;
use crate::install_publication_metadata::RETAINED_LIBRARY_MARKER;

fn write_library_marker(fixture: &Fixture) -> Vec<u8> {
    let root = fixture.install_root();
    let bytes = serde_json::to_vec(&serde_json::json!({
        "version": 1,
        "module_id": fixture.module.summary.id,
        "program_root": fs::canonicalize(&root).unwrap().to_string_lossy(),
    }))
    .unwrap();
    fs::write(root.join(RETAINED_LIBRARY_MARKER), &bytes).unwrap();
    bytes
}

#[tokio::test]
async fn retained_library_marker_is_published_with_the_replacement_payload() {
    for preserve_data in [false, true] {
        let fixture = Fixture::new();
        if !preserve_data {
            fs::remove_file(fixture.install_root().join(RETAINED_INSTALL_DATA_MARKER)).unwrap();
        }
        let marker = write_library_marker(&fixture);
        let mut server = PackageServer::new(fixture.archive(None).await).await;
        let mut verified_with_marker = false;
        fixture
            .install(&server.url, &mut |update| {
                if update
                    .install_progress
                    .as_ref()
                    .is_some_and(|progress| progress.phase == InstallPhase::Verifying)
                    && fixture.install_root().join("bin/Server.exe").is_file()
                {
                    assert_eq!(
                        fs::read(fixture.install_root().join(RETAINED_LIBRARY_MARKER)).unwrap(),
                        marker
                    );
                    verified_with_marker = true;
                }
            })
            .await
            .unwrap();
        server.finish().await;
        assert!(verified_with_marker);
        assert_eq!(
            fs::read(fixture.install_root().join(RETAINED_LIBRARY_MARKER)).unwrap(),
            marker
        );
        assert_eq!(
            fs::read(fixture.install_root().join("bin/Server.exe")).unwrap(),
            b"new server payload"
        );
        if preserve_data {
            fixture.assert_retained(false);
        }
        fixture.assert_transaction_clean().await;
    }
}

#[tokio::test]
async fn retained_library_marker_change_before_publication_preserves_the_old_root() {
    let fixture = Fixture::new();
    write_library_marker(&fixture);
    let mut server = PackageServer::new(fixture.archive(None).await).await;
    let mut changed = false;
    let error = fixture
        .install(&server.url, &mut |update| {
            if update
                .install_progress
                .as_ref()
                .is_some_and(|progress| progress.phase == InstallPhase::Extracting)
            {
                fs::write(
                    fixture.install_root().join(RETAINED_LIBRARY_MARKER),
                    b"changed ownership",
                )
                .unwrap();
                changed = true;
            }
        })
        .await
        .unwrap_err();
    server.finish().await;
    assert!(changed);
    assert!(error.to_string().contains("ownership changed"), "{error}");
    fixture.assert_retained(true);
    assert!(!fixture.install_root().join("bin/Server.exe").exists());
    assert_eq!(
        fs::read(fixture.install_root().join(RETAINED_LIBRARY_MARKER)).unwrap(),
        b"changed ownership"
    );
    fixture.assert_transaction_clean().await;
}

#[tokio::test]
async fn retained_library_marker_in_archive_is_rejected_without_replacing_the_library() {
    let fixture = Fixture::new();
    let marker = write_library_marker(&fixture);
    let mut server = PackageServer::new(fixture.archive(Some(RETAINED_LIBRARY_MARKER)).await).await;
    let error = fixture.install(&server.url, &mut |_| {}).await.unwrap_err();
    server.finish().await;
    assert!(
        error.to_string().contains("retained-library marker"),
        "{error}"
    );
    fixture.assert_retained(true);
    assert_eq!(
        fs::read(fixture.install_root().join(RETAINED_LIBRARY_MARKER)).unwrap(),
        marker
    );
    assert!(!fixture.install_root().join("bin/Server.exe").exists());
    fixture.assert_transaction_clean().await;
}

#[tokio::test]
async fn retained_library_marker_rolls_back_with_a_rejected_payload() {
    let fixture = Fixture::new();
    let marker = write_library_marker(&fixture);
    let mut server = PackageServer::new(fixture.archive(None).await).await;
    let mut published = false;
    let error = fixture
        .install(&server.url, &mut |update| {
            if update
                .install_progress
                .as_ref()
                .is_some_and(|progress| progress.phase == InstallPhase::Verifying)
                && !has_retained_install_data(&fixture.install_root())
            {
                assert_eq!(
                    fs::read(fixture.install_root().join(RETAINED_LIBRARY_MARKER)).unwrap(),
                    marker
                );
                fs::remove_file(fixture.install_root().join("bin/Server.exe")).unwrap();
                published = true;
            }
        })
        .await
        .unwrap_err();
    server.finish().await;
    assert!(published);
    assert!(matches!(
        error,
        SteamCmdError::InstallationVerificationFailed { .. }
    ));
    fixture.assert_retained(true);
    assert_eq!(
        fs::read(fixture.install_root().join(RETAINED_LIBRARY_MARKER)).unwrap(),
        marker
    );
    assert!(!fixture.install_root().join("bin/Server.exe").exists());
    fixture.assert_transaction_clean().await;
}
