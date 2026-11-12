use super::*;

use std::path::PathBuf;
use std::sync::OnceLock;

const DLL: &str = "Libraries/steam_api64.dll";
const SECOND_DLL: &str = "Libraries/loader_target.dll";
const ORIGINAL: &[u8] = b"official program content";

struct Fixture {
    root: PathBuf,
    descriptor: ModuleDescriptor,
}

impl Fixture {
    fn new() -> Self {
        static DESCRIPTOR: OnceLock<ModuleDescriptor> = OnceLock::new();
        let descriptor = DESCRIPTOR
            .get_or_init(|| {
                app_modules::discover_modules(
                    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../modules"),
                )
                .unwrap()
                .into_iter()
                .find(|module| module.summary.id == "abioticfactor")
                .unwrap()
            })
            .clone();
        let root =
            std::env::temp_dir().join(format!("pc-{:016x}", uuid::Uuid::new_v4().as_u128() as u64));
        fs::create_dir(&root).unwrap();
        let fixture = Self { root, descriptor };
        fixture.write(
            &fixture.descriptor.process.as_ref().unwrap().executable,
            ORIGINAL,
        );
        if let Some(verification) = fixture
            .descriptor
            .install
            .as_ref()
            .and_then(|install| install.verification_path.as_deref())
        {
            fixture.write(verification, ORIGINAL);
        }
        fixture.write(DLL, ORIGINAL);
        fixture.write(SECOND_DLL, ORIGINAL);
        fixture
    }

    fn write(&self, key: &str, bytes: &[u8]) {
        let path = self.root.join(key);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    fn record(&self) {
        crate::record_library_program_baseline(&self.root, &self.descriptor, true, None).unwrap();
    }

    fn check(&self) -> Result<Option<bool>, StorageError> {
        program_copy_requires_repair(&self.root, &self.descriptor, None)
    }

    fn inventories(&self) -> [Vec<u8>; 2] {
        [
            ".langame-clean-package.json",
            ".langame-initial-package.json",
        ]
        .map(|key| fs::read(self.root.join(key)).unwrap())
    }

    fn patch(&self, key: &str) -> Vec<u8> {
        let mut bytes = fs::read(self.root.join(key)).unwrap();
        bytes[0] ^= 0xff;
        self.write(key, &bytes);
        bytes
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        assert_eq!(self.root.parent(), Some(std::env::temp_dir().as_path()));
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn missing_non_entry_program_file_requires_repair_without_changing_evidence() {
    let fixture = Fixture::new();
    fixture.record();
    let inventories = fixture.inventories();
    assert_eq!(fixture.check().unwrap(), Some(false));
    fs::remove_file(fixture.root.join(DLL)).unwrap();

    assert_eq!(
        crate::program_launch_files_match_baseline(&fixture.root, &fixture.descriptor, None)
            .unwrap(),
        Some(true)
    );
    assert_eq!(fixture.check().unwrap(), Some(true));
    assert_eq!(fixture.inventories(), inventories);
    assert!(!fixture.root.join(DLL).exists());
}

#[test]
fn a_missing_file_never_authorizes_overwriting_an_existing_program_patch() {
    let fixture = Fixture::new();
    fixture.record();
    let inventories = fixture.inventories();
    fs::remove_file(fixture.root.join(DLL)).unwrap();
    let patched = fixture.patch(SECOND_DLL);

    let error = fixture.check().unwrap_err().to_string();
    assert!(error.contains("loader_target.dll"), "{error}");
    assert!(error.contains("modified source was preserved"), "{error}");
    assert_eq!(fs::read(fixture.root.join(SECOND_DLL)).unwrap(), patched);
    assert!(!fixture.root.join(DLL).exists());
    assert_eq!(fixture.inventories(), inventories);
}

#[test]
fn changed_registered_steam_metadata_requires_provider_verification_not_recognition_as_a_patch() {
    let fixture = Fixture::new();
    let metadata = format!(
        "steamapps/appmanifest_{}.acf",
        fixture.descriptor.summary.steam_app_id.unwrap()
    );
    fixture.write(&metadata, b"previous installation record");
    fixture.record();
    let inventories = fixture.inventories();
    fixture.write(&metadata, b"changed installation record");

    assert_eq!(fixture.check().unwrap(), Some(true));
    assert_eq!(fixture.inventories(), inventories);
    fixture.patch(SECOND_DLL);
    assert!(fixture.check().is_err());
    assert_eq!(fixture.inventories(), inventories);
}

#[test]
fn declared_data_and_unknown_files_never_become_program_repair_targets() {
    let mut fixture = Fixture::new();
    fixture.write("operator.cfg", b"shipped default");
    fixture.record();
    let inventories = fixture.inventories();
    fixture
        .descriptor
        .storage
        .retained_paths
        .push("operator.cfg".into());
    fixture.write("operator.cfg", b"operator configuration");
    fixture.write("unknown.dll", b"external loader");

    assert_eq!(fixture.check().unwrap(), Some(false));
    assert_eq!(fixture.inventories(), inventories);
    assert_eq!(
        fs::read(fixture.root.join("unknown.dll")).unwrap(),
        b"external loader"
    );
}

#[test]
fn absent_directories_are_repairable_but_replaced_paths_are_conflicts() {
    let fixture = Fixture::new();
    fixture.record();
    fs::remove_file(fixture.root.join(DLL)).unwrap();
    fs::remove_file(fixture.root.join(SECOND_DLL)).unwrap();
    fs::remove_dir(fixture.root.join("Libraries")).unwrap();
    assert_eq!(fixture.check().unwrap(), Some(true));

    fixture.write("Libraries", b"operator file replacing a directory");
    assert!(fixture.check().is_err());
    fs::remove_file(fixture.root.join("Libraries")).unwrap();
    fs::create_dir_all(fixture.root.join(DLL)).unwrap();
    assert!(fixture.check().is_err());
}

#[test]
fn absent_baseline_and_cancellation_do_not_invent_repair_evidence() {
    let fixture = Fixture::new();
    assert_eq!(fixture.check().unwrap(), None);
    for recorded in [false, true] {
        if recorded {
            fixture.record();
        }
        assert!(matches!(
            program_copy_requires_repair(
                &fixture.root,
                &fixture.descriptor,
                Some(&AtomicBool::new(true)),
            ),
            Err(StorageError::InstanceCreationCancelled)
        ));
    }
}

#[cfg(windows)]
#[test]
fn unreadable_existing_file_is_an_error_not_a_missing_file() {
    use std::os::windows::fs::OpenOptionsExt;

    let fixture = Fixture::new();
    fixture.record();
    let locked = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(fixture.root.join(SECOND_DLL))
        .unwrap();
    match fixture.check() {
        Err(StorageError::ReadPath { source, .. }) => assert_eq!(source.raw_os_error(), Some(32)),
        other => panic!("sharing violation must remain an error: {other:?}"),
    }
    drop(locked);
}
