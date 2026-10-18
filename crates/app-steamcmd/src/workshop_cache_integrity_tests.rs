use super::*;
use std::sync::atomic::AtomicU64;

static NEXT: AtomicU64 = AtomicU64::new(1);
const ID: &str = "123456";

struct Fixture {
    root: PathBuf,
    content: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lg-workshop-hash-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let content = root.join("steamapps/workshop/content/7");
        fs::create_dir_all(content.join(ID)).unwrap();
        fs::write(content.join(format!("{ID}/x.bin")), b"abc").unwrap();
        let fixture = Self { root, content };
        fixture.metadata("9");
        fixture
    }
    fn metadata(&self, manifest: &str) {
        fs::write(self.content.parent().unwrap().parent().unwrap().join("appworkshop_7.acf"),format!(r#""AppWorkshop" {{ "appid" "7" "WorkshopItemsInstalled" {{ "{ID}" {{ "manifest" "{manifest}" "size" "3" }} }} }}"#)).unwrap();
    }
    fn verify(&self, manifest: u64) -> io::Result<Files> {
        verify_old_item(
            &self.content,
            &self.root,
            &self.root,
            7,
            ID,
            InstalledItem { manifest, bytes: 3 },
            &InstallCancellation::new(),
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        assert_eq!(self.root.parent(), Some(std::env::temp_dir().as_path()));
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn native_success_receipt_detects_same_size_damage_without_a_cached_depot_manifest() {
    let fixture = Fixture::new();
    assert!(
        fixture.verify(9).is_err(),
        "unverified legacy bytes cannot become their own integrity proof"
    );
    // Simulates the trusted native-success boundary; hashes and persistence are real.
    record_item(&fixture.content, 7, ID, &InstallCancellation::new()).unwrap();
    let proof = fixture.verify(9).unwrap();
    assert_eq!(
        proof["x.bin"].sha256,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    fs::write(fixture.content.join(format!("{ID}/x.bin")), b"xyz").unwrap();
    assert!(fixture.verify(9).is_err());
}

#[test]
fn native_validation_receipt_replaces_old_revision_and_rejects_wrong_manifest_or_file_set() {
    let fixture = Fixture::new();
    record_item(&fixture.content, 7, ID, &InstallCancellation::new()).unwrap();
    fixture.metadata("10");
    fs::write(fixture.content.join(format!("{ID}/x.bin")), b"xyz").unwrap();
    assert!(fixture.verify(10).is_err());
    record_item(&fixture.content, 7, ID, &InstallCancellation::new()).unwrap();
    fixture.verify(10).unwrap();
    assert!(fixture.verify(9).is_err());
    fs::rename(
        fixture.content.join(format!("{ID}/x.bin")),
        fixture.content.join(format!("{ID}/other.bin")),
    )
    .unwrap();
    assert!(
        fixture.verify(10).is_err(),
        "same total size with another file name is incomplete"
    );
}
