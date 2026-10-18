use super::*;

const APP: u32 = 1_623_730;
const ITEM: u64 = 3_813_477_429;

struct Fixture {
    root: PathBuf,
    install: PathBuf,
    script: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = crate::tests::unique_test_root();
        let install = root.join("game with spaces");
        std::fs::create_dir_all(&install).unwrap();
        let fixture = Self {
            script: root.join("workshop-script.txt"),
            root,
            install,
        };
        fixture.script(&format!("workshop_download_item {APP} {ITEM} validate"));
        fixture
    }

    fn script(&self, requests: &str) {
        std::fs::write(
            &self.script,
            format!(
                "@ShutdownOnFailedCommand 1\nforce_install_dir \"{}\"\nlogin anonymous\n{requests}\nquit\n",
                self.install.to_string_lossy().replace('\\', "/")
            ),
        )
        .unwrap();
    }

    fn manifest(&self, root: &Path, source: &str) {
        let workshop = root.join("steamapps/workshop");
        std::fs::create_dir_all(&workshop).unwrap();
        std::fs::write(workshop.join(format!("appworkshop_{APP}.acf")), source).unwrap();
    }

    fn details(&self, root: &Path, manifest: u64) {
        self.manifest(
            root,
            &format!(
                "\"AppWorkshop\" {{ \"appid\" \"{APP}\" \"WorkshopItemsInstalled\" {{ \"{ITEM}\" {{ \"manifest\" \"111\" }} }} \"WorkshopItemDetails\" {{ \"{ITEM}\" {{ \"manifest\" \"{manifest}\" }} \"42\" {{ \"manifest\" \"444\" }} }} }}"
            ),
        );
    }

    async fn read(&self) -> Vec<(u32, u64, u64)> {
        requested_item_manifests(&self.script, &self.root).await
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[tokio::test]
async fn unchanged_retry_uses_requested_target_details_in_either_cache_root() {
    for fallback in [false, true] {
        let fixture = Fixture::new();
        let root = if fallback {
            &fixture.root
        } else {
            &fixture.install
        };
        fixture.details(root, 222);
        assert_eq!(fixture.read().await, [(APP, ITEM, 222)]);
    }
}

#[tokio::test]
async fn installed_older_manifest_and_unrequested_items_cannot_seed_a_retry() {
    let fixture = Fixture::new();
    fixture.manifest(
        &fixture.install,
        &format!(
            "\"AppWorkshop\" {{ \"appid\" \"{APP}\" \"WorkshopItemsInstalled\" {{ \"{ITEM}\" {{ \"manifest\" \"111\" }} }} \"WorkshopItemDetails\" {{ \"42\" {{ \"manifest\" \"444\" }} }} }}"
        ),
    );
    assert!(fixture.read().await.is_empty());
}

#[tokio::test]
async fn conflicting_cache_targets_do_not_guess_a_manifest() {
    let fixture = Fixture::new();
    fixture.details(&fixture.install, 222);
    fixture.details(&fixture.root, 333);
    assert!(fixture.read().await.is_empty());
    fixture.details(&fixture.root, 222);
    assert_eq!(fixture.read().await, [(APP, ITEM, 222)]);
}

#[tokio::test]
async fn wrong_app_duplicate_metadata_and_non_numeric_manifests_are_rejected() {
    let fixture = Fixture::new();
    for source in [
        format!(
            "\"AppWorkshop\" {{ \"appid\" \"42\" \"WorkshopItemDetails\" {{ \"{ITEM}\" {{ \"manifest\" \"222\" }} }} }}"
        ),
        format!(
            "\"AppWorkshop\" {{ \"appid\" \"{APP}\" \"appid\" \"{APP}\" \"WorkshopItemDetails\" {{ \"{ITEM}\" {{ \"manifest\" \"222\" }} }} }}"
        ),
        format!(
            "\"AppWorkshop\" {{ \"appid\" \"{APP}\" \"WorkshopItemDetails\" {{ \"{ITEM}\" {{ \"manifest\" \"222\" }} \"{ITEM}\" {{ \"manifest\" \"333\" }} }} }}"
        ),
        format!(
            "\"AppWorkshop\" {{ \"appid\" \"{APP}\" \"WorkshopItemDetails\" {{ \"{ITEM}\" {{ \"manifest\" \"222\" \"manifest\" \"333\" }} }} }}"
        ),
        format!(
            "\"AppWorkshop\" {{ \"appid\" \"{APP}\" \"WorkshopItemDetails\" {{ \"{ITEM}\" {{ \"manifest\" \"0\" }} }} }}"
        ),
        format!(
            "\"AppWorkshop\" {{ \"appid\" \"{APP}\" \"WorkshopItemDetails\" {{ \"{ITEM}\" {{ \"manifest\" \"../../222\" }} }} }}"
        ),
    ] {
        fixture.manifest(&fixture.install, &source);
        assert!(fixture.read().await.is_empty(), "{source}");
    }
}

#[tokio::test]
async fn bounded_script_and_manifest_fail_closed() {
    let fixture = Fixture::new();
    fixture.details(&fixture.install, 222);
    std::fs::write(&fixture.script, vec![b'x'; MAX_SCRIPT_BYTES + 1]).unwrap();
    assert!(fixture.read().await.is_empty());
    fixture.script(&format!("workshop_download_item {APP} {ITEM} validate"));
    fixture.manifest(&fixture.install, &" ".repeat(MAX_MANIFEST_BYTES + 1));
    assert!(fixture.read().await.is_empty());
}

#[test]
fn script_requires_an_absolute_non_traversing_install_and_exact_item_command() {
    let fixture = Fixture::new();
    let root = fixture.install.to_string_lossy().replace('\\', "/");
    for source in [
        format!("force_install_dir \"{root}/../other\"\nworkshop_download_item {APP} {ITEM}"),
        format!("force_install_dir \"relative/path\"\nworkshop_download_item {APP} {ITEM}"),
        format!(
            "force_install_dir \"{root}\"\nforce_install_dir \"{root}\"\nworkshop_download_item {APP} {ITEM}"
        ),
        format!("force_install_dir \"{root}\"\nworkshop_download_item 0 {ITEM}"),
        format!("force_install_dir \"{root}\"\nworkshop_download_item {APP} 0"),
        format!("force_install_dir \"{root}\"\nworkshop_download_item {APP} {ITEM} validate extra"),
        format!("force_install_dir \"{root}\"\nworkshop_download_item {APP} ../../42"),
    ] {
        assert!(script_requests(&source).is_none(), "{source}");
    }
}
