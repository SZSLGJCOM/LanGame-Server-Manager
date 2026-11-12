#![cfg(test)]

use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;

use app_modules::ModuleDescriptor;

use crate::{
    StorageError, library_program_is_pristine, program_launch_files_match_baseline,
    record_library_program_baseline,
};

const RUST_EXE: &str = "RustDedicated.exe";
const RUST_ASSEMBLY: &str = "RustDedicated_Data/Managed/Assembly-CSharp.dll";
const MINECRAFT_ENTRIES: [&str; 2] = ["server.jar", "jre/bin/java.exe"];
const ORIGINAL: &[u8] = b"official fixture program content";

struct Fixture {
    root: PathBuf,
    descriptor: ModuleDescriptor,
}

impl Fixture {
    fn new(module_id: &str) -> Self {
        static MODULES: OnceLock<Vec<ModuleDescriptor>> = OnceLock::new();
        let descriptor = MODULES
            .get_or_init(|| {
                app_modules::discover_modules(
                    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../modules"),
                )
                .expect("load the real module catalog")
            })
            .iter()
            .find(|descriptor| descriptor.summary.id == module_id)
            .expect("the launch regression names a real module")
            .clone();
        let root =
            std::env::temp_dir().join(format!("pl-{:016x}", uuid::Uuid::new_v4().as_u128() as u64));
        fs::create_dir(&root).expect("create an exclusively owned short fixture root");
        Self { root, descriptor }
    }

    fn write(&self, relative: &str, contents: &[u8]) {
        let path = self.root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn record(&self) {
        record_library_program_baseline(&self.root, &self.descriptor, true, None).unwrap();
    }

    fn launch_matches(&self) -> Option<bool> {
        program_launch_files_match_baseline(&self.root, &self.descriptor, None).unwrap()
    }

    fn pristine(&self) -> bool {
        library_program_is_pristine(&self.root, &self.descriptor, None).unwrap()
    }

    fn baselines(&self) -> [Option<Vec<u8>>; 2] {
        [
            ".langame-clean-package.json",
            ".langame-initial-package.json",
        ]
        .map(|name| {
            let path = self.root.join(name);
            path.exists().then(|| fs::read(path).unwrap())
        })
    }

    fn patch_first_byte(&self, relative: &str) -> Vec<u8> {
        let path = self.root.join(relative);
        let original = fs::read(&path).unwrap();
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        let mut byte = [0_u8; 1];
        file.read_exact(&mut byte).unwrap();
        byte[0] ^= 0xff;
        file.seek(SeekFrom::Start(0)).unwrap();
        file.write_all(&byte).unwrap();
        file.sync_all().unwrap();
        drop(file);
        let changed = fs::read(path).unwrap();
        assert_eq!(
            changed.len(),
            original.len(),
            "the patch must keep file size"
        );
        assert_ne!(changed, original, "the patch must change file content");
        changed
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        assert_eq!(self.root.parent(), Some(std::env::temp_dir().as_path()));
        fs::remove_dir_all(&self.root).expect("remove only the owned launch fixture");
    }
}

#[test]
fn rust_external_loader_patch_keeps_launch_ready_without_certifying_a_clean_package() {
    let fixture = Fixture::new("rust");
    fixture.write(RUST_EXE, ORIGINAL);
    fixture.write(RUST_ASSEMBLY, ORIGINAL);
    fixture.record();
    assert_eq!(fixture.launch_matches(), Some(true));
    assert!(fixture.pristine());
    let baselines = fixture.baselines();

    // Oxide can patch this original assembly outside LanGame's managed loader
    // records. An ordinary start must not validate it back to vanilla.
    let patched = fixture.patch_first_byte(RUST_ASSEMBLY);
    assert_eq!(fixture.launch_matches(), Some(true));
    assert!(!fixture.pristine());
    assert_eq!(fs::read(fixture.root.join(RUST_ASSEMBLY)).unwrap(), patched);
    assert_eq!(fs::read(fixture.root.join(RUST_EXE)).unwrap(), ORIGINAL);
    assert_eq!(fixture.baselines(), baselines);
}

#[test]
fn rust_same_length_launch_executable_corruption_is_not_ready() {
    let fixture = Fixture::new("rust");
    fixture.write(RUST_EXE, ORIGINAL);
    fixture.write(RUST_ASSEMBLY, ORIGINAL);
    fixture.record();
    assert_eq!(fixture.launch_matches(), Some(true));
    let baselines = fixture.baselines();

    let damaged = fixture.patch_first_byte(RUST_EXE);
    assert_eq!(fixture.launch_matches(), Some(false));
    assert!(!fixture.pristine());
    assert_eq!(fs::read(fixture.root.join(RUST_EXE)).unwrap(), damaged);
    assert_eq!(fixture.baselines(), baselines);
}

#[test]
fn legacy_launch_files_without_a_baseline_have_unknown_integrity() {
    let fixture = Fixture::new("rust");
    fixture.write(RUST_EXE, ORIGINAL);
    fixture.write(RUST_ASSEMBLY, ORIGINAL);

    assert_eq!(fixture.launch_matches(), None);
    assert_eq!(fixture.baselines(), [None, None]);
}

#[test]
fn minecraft_requires_baseline_evidence_for_both_jar_and_concrete_java_executable() {
    for missing in MINECRAFT_ENTRIES {
        let fixture = Fixture::new("minecraft");
        for entry in MINECRAFT_ENTRIES {
            if entry != missing {
                fixture.write(entry, ORIGINAL);
            }
        }
        fixture.record();
        let baselines = fixture.baselines();
        // Present bytes added after certification cannot stand in for an
        // installer baseline, even if every recorded file is still intact.
        fixture.write(missing, ORIGINAL);
        assert!(fixture.pristine());
        assert_eq!(fixture.launch_matches(), Some(false), "missing {missing}");
        assert_eq!(fixture.baselines(), baselines);
    }
}

#[test]
fn minecraft_detects_a_missing_jar_or_java_executable_after_certification() {
    for missing in MINECRAFT_ENTRIES {
        let fixture = Fixture::new("minecraft");
        for entry in MINECRAFT_ENTRIES {
            fixture.write(entry, ORIGINAL);
        }
        fixture.record();
        assert_eq!(fixture.launch_matches(), Some(true));
        let baselines = fixture.baselines();

        fs::remove_file(fixture.root.join(missing)).unwrap();
        assert_eq!(fixture.launch_matches(), Some(false), "missing {missing}");
        assert_eq!(fixture.baselines(), baselines);
    }
}

#[test]
fn terraria_uses_its_verification_path_when_the_launch_executable_is_dynamic() {
    let fixture = Fixture::new("terraria");
    assert!(
        fixture
            .descriptor
            .process
            .as_ref()
            .unwrap()
            .executable
            .contains("{{")
    );
    let verification = fixture
        .descriptor
        .install
        .as_ref()
        .unwrap()
        .verification_path
        .as_deref()
        .unwrap();
    fixture.write(verification, ORIGINAL);
    fixture.record();
    assert_eq!(fixture.launch_matches(), Some(true));

    fixture.patch_first_byte(verification);
    assert_eq!(fixture.launch_matches(), Some(false));
}

#[test]
fn cancelled_launch_verification_never_returns_readiness_or_adopts_a_baseline() {
    for has_baseline in [false, true] {
        let fixture = Fixture::new("rust");
        fixture.write(RUST_EXE, ORIGINAL);
        if has_baseline {
            fixture.record();
        }
        let baselines = fixture.baselines();
        let cancelled = AtomicBool::new(true);
        assert!(matches!(
            program_launch_files_match_baseline(
                &fixture.root,
                &fixture.descriptor,
                Some(&cancelled)
            ),
            Err(StorageError::InstanceCreationCancelled)
        ));
        assert_eq!(fs::read(fixture.root.join(RUST_EXE)).unwrap(), ORIGINAL);
        assert_eq!(fixture.baselines(), baselines);
    }
}
