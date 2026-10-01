use super::*;

static NEXT_FIXTURE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

struct CacheFixture {
    root: PathBuf,
    settings: AppSettings,
    install: PathBuf,
    steamcmd: PathBuf,
}

impl CacheFixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "langame-workshop-cache-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT_FIXTURE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let steamcmd = root.join("steamcmd");
        let install = root.join("install");
        let settings = AppSettings {
            archives_root: String::new(),
            servers_root: root.join("servers").to_string_lossy().into_owned(),
            games_root: root.join("games").to_string_lossy().into_owned(),
            modules_root: root.join("modules").to_string_lossy().into_owned(),
            steamcmd_root: steamcmd.to_string_lossy().into_owned(),
        };
        fs::create_dir_all(&steamcmd).unwrap();
        fs::write(steamcmd.join("steamcmd.exe"), b"fixture").unwrap();
        Self {
            root,
            settings,
            install,
            steamcmd,
        }
    }

    fn content(&self, shared: bool, app: u32) -> PathBuf {
        let base = if shared {
            &self.steamcmd
        } else {
            &self.install
        };
        base.join("steamapps/workshop/content")
            .join(app.to_string())
    }

    fn item(&self, shared: bool, app: u32, id: &str, bytes: &[u8]) -> PathBuf {
        let item = self.content(shared, app).join(id);
        fs::create_dir_all(&item).unwrap();
        if !bytes.is_empty() {
            fs::write(item.join("payload.bin"), bytes).unwrap();
        }
        item
    }

    fn manifest(&self, shared: bool, app: u32, body: &str) {
        let root = self
            .content(shared, app)
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join(format!("appworkshop_{app}.acf")), body).unwrap();
    }

    fn inspect(&self, app: u32, id: &str) -> SteamWorkshopInstallationSnapshot {
        inspect_workshop_items(&self.settings, app, &self.install, &[id.to_owned()]).unwrap()
    }
}

impl Drop for CacheFixture {
    fn drop(&mut self) {
        if self.root.exists() {
            fs::remove_dir_all(&self.root).unwrap();
        }
    }
}

fn manifest(app: u32, id: &str, size: usize, installed: &str, available: &str) -> String {
    format!(
        r#""AppWorkshop" {{ "appid" "{app}" "WorkshopItemsInstalled" {{ "{id}" {{ "manifest" "{installed}" "size" "{size}" }} }} "WorkshopItemDetails" {{ "{id}" {{ "manifest" "{available}" }} }} }}"#
    )
}

#[test]
fn workshop_cache_reuse_requires_manifest_and_complete_payload() {
    for app in [108_600, 393_380, 440_900, 602_960, 1_623_730] {
        let fixture = CacheFixture::new();
        fixture.item(false, app, "100000", b"");
        assert!(
            !fixture.inspect(app, "100000").items[0].installed,
            "empty cache for {app}"
        );
        let item = fixture.item(false, app, "100000", b"abc");
        assert!(
            !fixture.inspect(app, "100000").items[0].installed,
            "unrecorded payload"
        );
        fixture.manifest(false, app, &manifest(app, "100000", 4, "111", "111"));
        assert!(
            !fixture.inspect(app, "100000").items[0].installed,
            "truncated payload"
        );
        fixture.manifest(false, app, &manifest(app, "100000", 3, "111", "222"));
        assert!(
            !fixture.inspect(app, "100000").items[0].installed,
            "pending manifest"
        );
        fixture.manifest(false, app, &manifest(app, "100000", 3, "111", "111"));
        assert!(fixture.inspect(app, "100000").items[0].installed);
        assert_eq!(
            fs::read(item.join("payload.bin")).unwrap(),
            b"abc",
            "inspection preserves bytes"
        );
    }
}

#[test]
fn workshop_cache_uses_complete_alternate_instead_of_partial_primary() {
    let fixture = CacheFixture::new();
    fixture.item(false, 393_380, "100000", b"partial");
    fixture.manifest(
        false,
        393_380,
        &manifest(393_380, "100000", 20, "111", "111"),
    );
    let complete = fixture.item(true, 393_380, "100000", b"complete");
    fixture.manifest(true, 393_380, &manifest(393_380, "100000", 8, "222", "222"));
    let snapshot = fixture.inspect(393_380, "100000");
    assert!(snapshot.items[0].installed);
    assert_eq!(Path::new(&snapshot.items[0].path), complete);
}

#[test]
fn workshop_cache_rejects_wrong_app_and_malformed_install_records() {
    let fixture = CacheFixture::new();
    fixture.item(false, 393_380, "100000", b"abc");
    for body in [
        manifest(108_600, "100000", 3, "111", "111"),
        manifest(393_380, "100000", 3, "0", "0"),
        manifest(393_380, "200000", 3, "111", "111"),
        String::from("\"AppWorkshop\" {"),
    ] {
        fixture.manifest(false, 393_380, &body);
        assert!(!fixture.inspect(393_380, "100000").items[0].installed);
    }
}

#[test]
fn workshop_download_rechecks_cache_payload_after_native_success() {
    let fixture = CacheFixture::new();
    let item = fixture.item(false, 393_380, "100000", b"abc");
    fixture.manifest(
        false,
        393_380,
        &manifest(393_380, "100000", 3, "111", "111"),
    );
    let inspect = || {
        inspect_workshop_item_paths(
            393_380,
            &fixture.content(false, 393_380),
            &fixture.content(true, 393_380),
            vec!["100000".to_owned()],
        )
        .unwrap()
    };
    assert!(crate::verify_downloaded_workshop_items(&inspect(), "native success").is_ok());
    fs::write(item.join("payload.bin"), b"a").unwrap();
    let error = crate::verify_downloaded_workshop_items(&inspect(), "native success").unwrap_err();
    assert!(error.to_string().contains("100000"));
}

#[test]
fn workshop_cache_rejects_excessive_payload_nesting() {
    let fixture = CacheFixture::new();
    let mut directory = fixture.item(false, 393_380, "100000", b"abc");
    for _ in 0..65 {
        directory = directory.join("d");
    }
    fs::create_dir_all(directory).unwrap();
    fixture.manifest(
        false,
        393_380,
        &manifest(393_380, "100000", 3, "111", "111"),
    );
    let error = inspect_workshop_items(
        &fixture.settings,
        393_380,
        &fixture.install,
        &["100000".to_owned()],
    )
    .unwrap_err();
    assert!(error.to_string().contains("64-level"));
}

#[cfg(windows)]
#[test]
fn workshop_cache_rejects_linked_payload_without_reading_or_changing_target() {
    let fixture = CacheFixture::new();
    let item = fixture.item(false, 393_380, "100000", b"abc");
    let outside = fixture.root.join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("sentinel"), b"preserve").unwrap();
    let link = item.join("linked");
    let output = std::process::Command::new("cmd")
        .args(["/D", "/C", "mklink", "/J"])
        .arg(link.to_string_lossy().replace('/', "\\"))
        .arg(outside.to_string_lossy().replace('/', "\\"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    fixture.manifest(
        false,
        393_380,
        &manifest(393_380, "100000", 11, "111", "111"),
    );
    let result = inspect_workshop_items(
        &fixture.settings,
        393_380,
        &fixture.install,
        &["100000".to_owned()],
    );
    fs::remove_dir(&link).unwrap();
    assert!(result.unwrap_err().to_string().contains("reparse point"));
    assert_eq!(fs::read(outside.join("sentinel")).unwrap(), b"preserve");
}
