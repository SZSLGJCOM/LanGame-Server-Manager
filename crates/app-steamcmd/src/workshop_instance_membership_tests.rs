use super::*;
use serde_json::json;

const APP: u32 = 2394010;
const ID: &str = "1234567890";

struct Fixture {
    root: PathBuf,
    install: PathBuf,
    global: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "langame-workshop-membership-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        let install = root.join("private-instance");
        let global = root.join("shared-steamcmd");
        fs::create_dir_all(&install).unwrap();
        fs::create_dir_all(&global).unwrap();
        Self {
            root,
            install,
            global,
        }
    }
    fn candidates(&self) -> io::Result<Vec<String>> {
        instance_candidate_ids(&self.install, APP, &InstallCancellation::new())
    }
    fn record(&self) {
        record_native_success(&self.install, APP, ID, 77, &InstallCancellation::new()).unwrap();
    }
    fn record_path(&self) -> PathBuf {
        membership_path(&self.install, APP, ID).unwrap()
    }
    fn write_record(&self, value: serde_json::Value) {
        let path = self.record_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
    }
    fn acf(&self, root: &Path, app: u32, id: &str, manifest: u64) {
        let workshop = root.join("steamapps/workshop");
        fs::create_dir_all(workshop.join("content").join(app.to_string())).unwrap();
        fs::write(workshop.join(format!("appworkshop_{app}.acf")), format!(r#""AppWorkshop" {{ "appid" "{app}" "WorkshopItemsInstalled" {{ "{id}" {{ "manifest" "{manifest}" "size" "3" }} }} "WorkshopItemDetails" {{ "{id}" {{ "manifest" "99" }} }} }}"#)).unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        assert_eq!(self.root.parent(), Some(std::env::temp_dir().as_path()));
        assert!(
            self.root
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("langame-workshop-membership-")
        );
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn global_cache_and_same_package_name_never_contribute_instance_candidates() {
    let fixture = Fixture::new();
    fixture.acf(&fixture.global, APP, ID, 77);
    let payload = fixture
        .global
        .join(format!("steamapps/workshop/content/{APP}/{ID}"));
    fs::create_dir(&payload).unwrap();
    fs::write(
        payload.join("Info.json"),
        br#"{"PackageName":"SamePackageName"}"#,
    )
    .unwrap();
    assert!(fixture.candidates().unwrap().is_empty());
    fixture.record();
    assert_eq!(fixture.candidates().unwrap(), [ID]);
    assert_eq!(
        fs::read(payload.join("Info.json")).unwrap(),
        br#"{"PackageName":"SamePackageName"}"#
    );
}

#[test]
fn private_old_installed_acf_and_membership_form_a_deduplicated_local_union() {
    let fixture = Fixture::new();
    fixture.acf(&fixture.install, APP, ID, 77);
    fixture.record();
    record_native_success(
        &fixture.install,
        APP,
        "2234567890",
        88,
        &InstallCancellation::new(),
    )
    .unwrap();
    assert_eq!(fixture.candidates().unwrap(), [ID, "2234567890"]);
}

#[test]
fn native_success_record_has_only_bounded_provenance_and_is_atomically_replaced() {
    let fixture = Fixture::new();
    fixture.record();
    let before: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.record_path()).unwrap()).unwrap();
    assert_eq!(
        before,
        json!({"version":1,"app_id":APP,"item_id":ID,"manifest":77,"native_success":true})
    );
    record_native_success(&fixture.install, APP, ID, 88, &InstallCancellation::new()).unwrap();
    let after: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.record_path()).unwrap()).unwrap();
    assert_eq!(after["manifest"], 88);
    assert_eq!(
        fs::read_dir(fixture.record_path().parent().unwrap())
            .unwrap()
            .count(),
        1
    );
    assert_eq!(fixture.candidates().unwrap(), [ID]);
}

#[test]
fn wrong_scope_malformed_and_non_success_records_cannot_authorize_ids() {
    for mutation in 0..7 {
        let fixture = Fixture::new();
        let mut record =
            json!({"version":1,"app_id":APP,"item_id":ID,"manifest":77,"native_success":true});
        match mutation {
            0 => record["version"] = json!(2),
            1 => record["app_id"] = json!(1),
            2 => record["item_id"] = json!("2234567890"),
            3 => record["manifest"] = json!(0),
            4 => record["native_success"] = json!(false),
            5 => record["users"] = json!(["inert-fixture-account"]),
            _ => record["manifest"] = json!("not-a-number"),
        }
        fixture.write_record(record);
        let error = fixture.candidates().unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(!error.to_string().contains("inert-fixture-account"));
    }
    let fixture = Fixture::new();
    fixture.record();
    fs::write(fixture.record_path(), b"not JSON").unwrap();
    assert_eq!(
        fixture.candidates().unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
}

#[test]
fn only_the_current_app_is_read_and_unrelated_user_files_are_not_parsed() {
    let fixture = Fixture::new();
    let other_app = fixture.install.join(".langame-workshop-membership/1");
    fs::create_dir_all(&other_app).unwrap();
    fs::write(
        other_app.join(format!("{ID}.json")),
        b"not JSON; unrelated data",
    )
    .unwrap();
    let context = fixture.install.join("sdk-context");
    fs::create_dir(&context).unwrap();
    fs::write(
        context.join("users.json"),
        b"not JSON; unrelated user context",
    )
    .unwrap();
    assert!(fixture.candidates().unwrap().is_empty());
    fixture.record();
    assert_eq!(fixture.candidates().unwrap(), [ID]);
}

#[test]
fn missing_or_nonpositive_local_acf_manifest_is_not_membership() {
    let fixture = Fixture::new();
    fixture.acf(&fixture.install, APP, ID, 0);
    assert!(fixture.candidates().unwrap().is_empty());
    fixture.acf(&fixture.global, APP, ID, 77);
    assert!(fixture.candidates().unwrap().is_empty());
}

#[test]
fn membership_read_write_and_identity_are_bounded_and_cancellable() {
    let fixture = Fixture::new();
    for id in [
        "000000",
        "0123456789",
        "12345",
        "../123456",
        "123456789012345678901",
        "18446744073709551616",
    ] {
        assert!(membership_path(&fixture.install, APP, id).is_err(), "{id}");
    }
    assert!(membership_path(&fixture.install, 0, ID).is_err());
    assert!(
        record_native_success(&fixture.install, APP, ID, 0, &InstallCancellation::new()).is_err()
    );
    let cancelled = InstallCancellation::new();
    cancelled.cancel();
    assert_eq!(
        instance_candidate_ids(&fixture.install, APP, &cancelled)
            .unwrap_err()
            .kind(),
        io::ErrorKind::Interrupted
    );
    assert_eq!(
        record_native_success(&fixture.install, APP, ID, 77, &cancelled)
            .unwrap_err()
            .kind(),
        io::ErrorKind::Interrupted
    );
    assert!(!fixture.record_path().exists());
    fixture.record();
    fs::OpenOptions::new()
        .write(true)
        .open(fixture.record_path())
        .unwrap()
        .set_len(MAX_MEMBERSHIP_BYTES as u64 + 1)
        .unwrap();
    assert_eq!(
        fixture.candidates().unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
}

#[test]
fn membership_scan_counts_ignored_temporary_entries_towards_its_bound() {
    let fixture = Fixture::new();
    let directory = fixture.record_path().parent().unwrap().to_path_buf();
    fs::create_dir_all(&directory).unwrap();
    for index in 0..=MAX_ITEMS {
        fs::write(
            directory.join(format!(".langame-membership-{index}.tmp")),
            b"",
        )
        .unwrap();
    }
    let error = fixture.candidates().unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("scan limit"));
}

#[test]
fn linked_membership_namespace_cannot_be_read_or_written() {
    let fixture = Fixture::new();
    let outside = fixture.root.join("outside");
    fs::create_dir(&outside).unwrap();
    let link = fixture.install.join(".langame-workshop-membership");
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use std::os::windows::process::CommandExt;
        let quote = |path: &Path| {
            let path = path.to_str().unwrap().replace('/', "\\");
            assert!(!path.contains('"'));
            format!("\"{path}\"")
        };
        let output = std::process::Command::new("cmd.exe")
            .args(["/D", "/V:OFF", "/C"])
            .raw_arg("mklink /J")
            .raw_arg(quote(&link))
            .raw_arg(quote(&outside))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_ne!(
            fs::symlink_metadata(&link).unwrap().file_attributes() & 0x400,
            0
        );
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    assert!(fixture.candidates().is_err());
    assert!(
        record_native_success(&fixture.install, APP, ID, 77, &InstallCancellation::new()).is_err()
    );
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
    #[cfg(windows)]
    fs::remove_dir(&link).unwrap();
    #[cfg(unix)]
    fs::remove_file(&link).unwrap();
}
