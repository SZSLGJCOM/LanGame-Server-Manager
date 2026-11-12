use super::*;

struct Fixture {
    root: PathBuf,
    installation: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "mc-{stamp:x}-{:x}",
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let installation = root.join("game");
        fs::create_dir_all(&installation).unwrap();
        Self { root, installation }
    }

    fn publication(&self) -> MinecraftPublication {
        let publication = MinecraftPublication::new(&self.installation, "minecraft").unwrap();
        write(&publication.stage, "server.jar", b"official jar");
        write(&publication.stage, "jre/bin/java.exe", b"official java");
        write(&publication.stage, "jre/lib/modules", b"official modules");
        write(
            &publication.stage,
            ".langame/minecraft-server.json",
            b"new official metadata",
        );
        write(
            &publication.stage,
            ".langame-clean-package.json",
            b"new clean inventory",
        );
        write(
            &publication.stage,
            ".langame-initial-package.json",
            b"new initial inventory",
        );
        publication
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn write(root: &Path, relative: &str, bytes: &[u8]) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn deadline() -> InstallDeadline {
    InstallDeadline::new("testing Minecraft publication", Duration::from_secs(45))
}

#[tokio::test]
async fn merge_keeps_operator_data_out_of_inventory_and_only_recovers_changed_program_files() {
    let fixture = Fixture::new();
    for (relative, bytes) in [
        ("server.jar", b"changed jar".as_slice()),
        ("jre/bin/java.exe", b"official java".as_slice()),
        ("jre/lib/modules", b"changed modules".as_slice()),
        ("jre/operator-file.txt", b"operator java data".as_slice()),
        ("plugins/operator-only.jar", b"operator plugin".as_slice()),
        ("world/region/save.mca", b"operator world".as_slice()),
        ("server.properties", b"operator configuration".as_slice()),
        (
            ".langame-program-identity.json",
            b"stable identity".as_slice(),
        ),
        (".langame-program-usage.json", b"stable usage".as_slice()),
        (
            ".langame-clean-package.json",
            b"old clean inventory".as_slice(),
        ),
        (
            ".langame-initial-package.json",
            b"old initial inventory".as_slice(),
        ),
    ] {
        write(&fixture.installation, relative, bytes);
    }
    let before = fs::read(fixture.installation.join("server.jar")).unwrap();
    let publication = fixture.publication();
    publication.merge_existing(deadline()).await.unwrap();
    for relative in [
        "jre/operator-file.txt",
        "plugins/operator-only.jar",
        "world/region/save.mca",
        "server.properties",
        ".langame-program-identity.json",
        ".langame-program-usage.json",
    ] {
        assert_eq!(
            fs::read(publication.stage.join(relative)).unwrap(),
            fs::read(fixture.installation.join(relative)).unwrap(),
            "{relative}"
        );
    }
    assert_eq!(
        fs::read(publication.stage.join("server.jar")).unwrap(),
        b"official jar"
    );
    assert_eq!(
        fs::read(publication.stage.join(".langame-clean-package.json")).unwrap(),
        b"new clean inventory"
    );
    assert_eq!(
        fs::read(publication.stage.join(".langame-initial-package.json")).unwrap(),
        b"new initial inventory"
    );
    assert_eq!(
        fs::read(
            publication
                .stage
                .join(&publication.recovery)
                .join("server.jar")
        )
        .unwrap(),
        b"changed jar"
    );
    assert_eq!(
        fs::read(
            publication
                .stage
                .join(&publication.recovery)
                .join("jre/lib/modules")
        )
        .unwrap(),
        b"changed modules"
    );
    assert!(
        !publication
            .stage
            .join(&publication.recovery)
            .join("jre/bin/java.exe")
            .exists()
    );
    assert_eq!(
        fs::read(fixture.installation.join("server.jar")).unwrap(),
        before
    );
    publication.recover(false).await.unwrap();
}

#[tokio::test]
async fn cancellation_during_preparation_preserves_the_installed_tree() {
    let fixture = Fixture::new();
    write(&fixture.installation, "server.jar", b"old jar");
    write(&fixture.installation, "world/save", b"world");
    let publication = fixture.publication();
    let cancellation = InstallCancellation::new();
    cancellation.cancel();
    let error = cancellation
        .scope(publication.merge_existing(deadline()))
        .await
        .unwrap_err();
    assert!(matches!(error, SteamCmdError::InstallCancelled { .. }));
    publication.recover(false).await.unwrap();
    assert_eq!(
        fs::read(fixture.installation.join("server.jar")).unwrap(),
        b"old jar"
    );
    assert_eq!(
        fs::read(fixture.installation.join("world/save")).unwrap(),
        b"world"
    );
    assert!(!publication.stage.exists());
}

#[cfg(windows)]
#[tokio::test]
async fn published_payload_can_restore_the_complete_previous_generation() {
    let fixture = Fixture::new();
    write(&fixture.installation, "server.jar", b"old jar");
    write(&fixture.installation, "world/save", b"world");
    write(
        &fixture.installation,
        ".langame-clean-package.json",
        b"old inventory",
    );
    let publication = fixture.publication();
    publication.merge_existing(deadline()).await.unwrap();
    publication
        .publish(Path::new("server.jar"), deadline())
        .await
        .unwrap();
    assert_eq!(
        fs::read(fixture.installation.join("server.jar")).unwrap(),
        b"official jar"
    );
    assert_eq!(
        fs::read(fixture.installation.join("world/save")).unwrap(),
        b"world"
    );
    publication.recover(true).await.unwrap();
    assert_eq!(
        fs::read(fixture.installation.join("server.jar")).unwrap(),
        b"old jar"
    );
    assert_eq!(
        fs::read(fixture.installation.join(".langame-clean-package.json")).unwrap(),
        b"old inventory"
    );
    assert_eq!(
        fs::read(fixture.installation.join("world/save")).unwrap(),
        b"world"
    );
    assert!(!fixture.installation.join("jre").exists());
}

#[cfg(windows)]
#[tokio::test]
async fn failed_publication_keeps_the_previous_generation_and_inventory() {
    let fixture = Fixture::new();
    write(&fixture.installation, "server.jar", b"old jar");
    write(
        &fixture.installation,
        ".langame-clean-package.json",
        b"old inventory",
    );
    let publication = fixture.publication();
    publication.merge_existing(deadline()).await.unwrap();
    assert!(
        publication
            .publish(Path::new("missing.jar"), deadline())
            .await
            .is_err()
    );
    publication.recover(false).await.unwrap();
    assert_eq!(
        fs::read(fixture.installation.join("server.jar")).unwrap(),
        b"old jar"
    );
    assert_eq!(
        fs::read(fixture.installation.join(".langame-clean-package.json")).unwrap(),
        b"old inventory"
    );
}
