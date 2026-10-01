use super::*;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "langame-stop-recovery-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn directory(&self, name: &str, payload: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("payload"), payload).unwrap();
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn cancelled_jre_validation_restores_the_previous_verified_runtime() {
    let fixture = Fixture::new();
    let published = fixture.directory("jre", b"new unverified runtime");
    let rollback = fixture.directory("rollback", b"previous runtime");
    let stage = fixture.directory("stage", b"staged contents");
    let rejected = fixture.0.join("rejected");
    let result = finish_published_jre(
        Err(SteamCmdError::InstallCancelled {
            operation: String::from("JRE validation"),
        }),
        21,
        &published,
        &rollback,
        &rejected,
        &stage,
    )
    .await;
    assert!(matches!(
        result,
        Err(SteamCmdError::InstallCancelled { .. })
    ));
    assert_eq!(
        fs::read(published.join("payload")).unwrap(),
        b"previous runtime"
    );
    assert!(!rollback.exists());
    assert!(!stage.exists());
    assert!(!rejected.exists());
}

#[tokio::test]
async fn cancelled_archive_publish_recovers_old_files_before_cleanup_returns() {
    let fixture = Fixture::new();
    let published = fixture.directory("server", b"new payload");
    let rollback = fixture.directory("rollback", b"previous server files");
    let stage = fixture.directory("stage", b"staging");
    let rejected = fixture.0.join("rejected");
    let phase = fixture.0.join("publish.state");
    let archive = fixture.0.join("archive.zip");
    fs::write(&phase, b"rollback_pending").unwrap();
    fs::write(&archive, b"archive").unwrap();
    recover_direct_download_install(
        &published, &stage, &rollback, &rejected, &phase, &archive, true,
    )
    .await
    .unwrap();
    assert_eq!(
        fs::read(published.join("payload")).unwrap(),
        b"previous server files"
    );
    for path in [&rollback, &stage, &rejected, &phase, &archive] {
        assert!(!path.exists());
    }
}

#[tokio::test]
async fn unconfirmed_jre_process_cleanup_preserves_both_recovery_directories() {
    let fixture = Fixture::new();
    let published = fixture.directory("jre", b"new payload");
    let rollback = fixture.directory("rollback", b"previous runtime");
    let stage = fixture.directory("stage", b"staging");
    let result = finish_published_jre(
        Err(SteamCmdError::InstallProcessCleanupFailed {
            operation: String::from("JRE validation"),
            detail: String::from("process still present"),
        }),
        21,
        &published,
        &rollback,
        &fixture.0.join("rejected"),
        &stage,
    )
    .await;
    assert!(matches!(
        result,
        Err(SteamCmdError::InstallProcessCleanupFailed { .. })
    ));
    assert_eq!(
        fs::read(rollback.join("payload")).unwrap(),
        b"previous runtime"
    );
    assert_eq!(fs::read(published.join("payload")).unwrap(), b"new payload");
    assert!(stage.exists());
}
