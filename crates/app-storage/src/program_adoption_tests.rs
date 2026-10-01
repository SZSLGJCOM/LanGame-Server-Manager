// Copy tests use the creation owner; adoption fixtures model interrupted transfers
// whose durable journals still require recovery after library-preserving creation.
use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[path = "instance_program_copy_tests.rs"]
mod copy_verification_tests;

struct AdoptionFixture {
    root: PathBuf,
    source: PathBuf,
    instances: PathBuf,
    instance: PathBuf,
}

impl AdoptionFixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("langame-adoption-{}", uuid::Uuid::new_v4()));
        let source = root.join("downloaded");
        let instances = root.join("instances");
        let instance = instances.join("first");
        fs::create_dir_all(source.join("Saved/Config")).unwrap();
        fs::create_dir_all(&instance).unwrap();
        fs::write(source.join("server.bin"), b"original downloaded program").unwrap();
        fs::write(source.join("Saved/world.sav"), b"existing player world").unwrap();
        fs::write(
            source.join("Saved/Config/server.ini"),
            b"existing private settings",
        )
        .unwrap();
        fs::write(source.join("server.cfg"), b"operator configuration").unwrap();
        Self {
            root,
            source,
            instances,
            instance,
        }
    }

    fn exclusions(&self) -> Vec<PathBuf> {
        vec![
            self.source.join("Saved"),
            self.source.join("Saved/Config"),
            self.source.join("server.cfg"),
        ]
    }

    fn record_clean_package(&self) {
        let modules = self.root.join("modules");
        fs::create_dir_all(modules.join("fixture")).unwrap();
        fs::write(
            modules.join("fixture/module.toml"),
            r#"
id = "fixture"
name = "Adoption fixture"
version = "1.0.0"
[storage]
runtime_copy_exclusions = ["Saved", "server.cfg"]
"#,
        )
        .unwrap();
        let descriptor = app_modules::discover_modules(&modules).unwrap().remove(0);
        crate::record_library_program_baseline(&self.source, &descriptor, true, None).unwrap();
    }

    fn adopt(&self) -> ProgramAdoption {
        ProgramAdoption::begin(
            &self.source,
            &self.instance,
            &self.exclusions(),
            false,
            crate::program_runtime::ProgramFileSelection::Automatic,
            None,
        )
        .unwrap()
        .expect("same-volume program adoption")
    }

    fn pending(&self, adoption: ProgramAdoption) -> PendingInstanceDirectory {
        PendingInstanceDirectory {
            instances_root: self.instances.clone(),
            instance_root: self.instance.clone(),
            armed: true,
            adoption: Some(adoption),
        }
    }

    fn assert_original(&self) {
        assert_eq!(
            fs::read(self.source.join("server.bin")).unwrap(),
            b"original downloaded program"
        );
        assert_eq!(
            fs::read(self.source.join("Saved/world.sav")).unwrap(),
            b"existing player world"
        );
        assert_eq!(
            fs::read(self.source.join("Saved/Config/server.ini")).unwrap(),
            b"existing private settings"
        );
        assert_eq!(
            fs::read(self.source.join("server.cfg")).unwrap(),
            b"operator configuration"
        );
        assert!(!self.source.join(".langame-private-runtime").exists());
        assert!(!self.source.join(".langame-package-baseline.json").exists());
    }
}

impl Drop for AdoptionFixture {
    fn drop(&mut self) {
        assert!(self.root.starts_with(std::env::temp_dir()));
        fs::remove_dir_all(&self.root).expect("remove only this adoption fixture");
    }
}

#[test]
fn independent_local_adoption_does_not_hash_payload_or_create_refresh_baseline() {
    for selection in [
        crate::program_runtime::ProgramFileSelection::Local,
        crate::program_runtime::ProgramFileSelection::Automatic,
    ] {
        let f = AdoptionFixture::new();
        fs::write(f.source.join("operator-mod.bin"), vec![37_u8; 768 * 1024]).unwrap();
        let before = crate::test_file_snapshot::tree_snapshot(&f.source).unwrap();
        let cancellation = Arc::new(AtomicBool::new(false));
        let reads = crate::instance_creation_io::test_gate::count_hash_reads(&cancellation);
        let adoption = ProgramAdoption::begin(
            &f.source,
            &f.instance,
            &f.exclusions(),
            false,
            selection,
            Some(&cancellation),
        )
        .unwrap()
        .expect("same-volume program adoption");
        assert_eq!(
            reads.chunks(),
            0,
            "local ownership transfer must not hash program payloads"
        );
        let runtime = f.instance.join("runtime");
        assert!(!runtime.join(".langame-package-baseline.json").exists());
        assert_eq!(
            fs::read(runtime.join("operator-mod.bin")).unwrap(),
            vec![37_u8; 768 * 1024]
        );
        assert_eq!(
            fs::read(f.instance.join("installation-retained/Saved/world.sav")).unwrap(),
            b"existing player world"
        );
        adoption.rollback().unwrap();
        assert_eq!(
            crate::test_file_snapshot::tree_snapshot(&f.source).unwrap(),
            before
        );
    }
}

#[test]
fn independent_local_copy_does_not_hash_payload_or_create_refresh_baseline() {
    for selection in [
        crate::program_runtime::ProgramFileSelection::Local,
        crate::program_runtime::ProgramFileSelection::Automatic,
    ] {
        let f = AdoptionFixture::new();
        fs::write(f.source.join("operator-mod.bin"), vec![43_u8; 768 * 1024]).unwrap();
        let before = crate::test_file_snapshot::tree_snapshot(&f.source).unwrap();
        let cancellation = Arc::new(AtomicBool::new(false));
        let reads = crate::instance_creation_io::test_gate::count_hash_reads(&cancellation);
        let runtime = prepare_private_runtime_root(
            &f.source,
            &f.instance,
            &f.exclusions(),
            None,
            false,
            selection,
            Some(&cancellation),
        )
        .unwrap();
        assert_eq!(
            reads.chunks(),
            0,
            "independent copying must not hash an unused refresh baseline"
        );
        assert!(!runtime.join(".langame-package-baseline.json").exists());
        assert!(!runtime.join("Saved").exists());
        assert!(!runtime.join("server.cfg").exists());
        assert_eq!(
            fs::read(runtime.join("operator-mod.bin")).unwrap(),
            vec![43_u8; 768 * 1024]
        );
        assert_eq!(
            crate::test_file_snapshot::tree_snapshot(&f.source).unwrap(),
            before
        );
    }
}

#[test]
fn independent_official_copy_verifies_each_payload_only_while_copying() {
    for selection in [
        crate::program_runtime::ProgramFileSelection::Verified("fixture"),
        crate::program_runtime::ProgramFileSelection::Automatic,
    ] {
        let f = AdoptionFixture::new();
        fs::write(f.source.join("payload.bin"), vec![43_u8; 256 * 1024 + 1]).unwrap();
        f.record_clean_package();
        let cancellation = Arc::new(AtomicBool::new(false));
        let reads = crate::instance_creation_io::test_gate::count_hash_reads(&cancellation);
        let runtime = prepare_private_runtime_root(
            &f.source,
            &f.instance,
            &f.exclusions(),
            None,
            false,
            selection,
            Some(&cancellation),
        )
        .unwrap();
        assert_eq!(
            reads.chunks(),
            3,
            "one server chunk and two payload chunks must be verified during their only read"
        );
        assert!(runtime.join(".langame-package-baseline.json").is_file());
        assert!(
            !runtime
                .join(crate::private_runtime::PROJECTION_RUNTIME_MARKER)
                .exists()
        );
        assert_eq!(
            fs::read(runtime.join("server.bin")).unwrap(),
            b"original downloaded program"
        );
        f.assert_original();

        fs::write(f.source.join("server.bin"), b"tampered official program").unwrap();
        let second = f.instances.join("second");
        let pending = PendingInstanceDirectory {
            instances_root: f.instances.clone(),
            instance_root: second.clone(),
            armed: true,
            adoption: None,
        };
        assert!(
            prepare_private_runtime_root(
                &f.source,
                &second,
                &f.exclusions(),
                None,
                false,
                selection,
                None,
            )
            .is_err()
        );
        assert!(!second.join("runtime").exists());
        pending.rollback().unwrap();
        assert!(!second.exists());
        assert_eq!(
            fs::read(f.source.join("server.bin")).unwrap(),
            b"tampered official program"
        );
        assert_eq!(
            fs::read(runtime.join("server.bin")).unwrap(),
            b"original downloaded program"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn independent_local_copy_cancellation_removes_only_its_pending_copy() {
    use crate::instance_creation_io::test_gate::{PausePoint, pause_at};
    let f = AdoptionFixture::new();
    fs::write(f.source.join("payload.bin"), vec![42_u8; 768 * 1024]).unwrap();
    let before = crate::test_file_snapshot::tree_snapshot(&f.source).unwrap();
    let cancellation = Arc::new(AtomicBool::new(false));
    let mut gate = pause_at(&cancellation, PausePoint::Copy);
    let source = f.source.clone();
    let instance = f.instance.clone();
    let exclusions = f.exclusions();
    let owner = PendingInstanceDirectory {
        instances_root: f.instances.clone(),
        instance_root: instance.clone(),
        armed: true,
        adoption: None,
    };
    let token = Arc::clone(&cancellation);
    let worker = tokio::task::spawn_blocking(move || {
        let result = prepare_private_runtime_root(
            &source,
            &instance,
            &exclusions,
            None,
            false,
            crate::program_runtime::ProgramFileSelection::Local,
            Some(&token),
        );
        owner.rollback().unwrap();
        result
    });
    gate.reached().await;
    assert!(f.instance.join("runtime.staging").is_dir());
    cancellation.store(true, Ordering::Release);
    drop(gate);
    assert!(matches!(
        worker.await.unwrap(),
        Err(StorageError::InstanceCreationCancelled)
    ));
    assert!(!f.instance.exists());
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&f.source).unwrap(),
        before
    );
}

#[cfg(windows)]
fn file_identity(path: &Path) -> (u64, u64) {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };
    let file = fs::File::open(path).unwrap();
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the live file handle and writable structure belong to this fixture.
    assert_ne!(
        unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) },
        0
    );
    (
        u64::from(info.dwVolumeSerialNumber),
        (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
    )
}

#[cfg(unix)]
fn file_identity(path: &Path) -> (u64, u64) {
    use std::os::unix::fs::MetadataExt;
    let metadata = fs::metadata(path).unwrap();
    (metadata.dev(), metadata.ino())
}

#[cfg(any(windows, unix))]
#[test]
fn committed_adoption_journal_preserves_transferred_file_identity_and_retained_data() {
    let f = AdoptionFixture::new();
    let program_identity = file_identity(&f.source.join("server.bin"));
    let world_identity = file_identity(&f.source.join("Saved/world.sav"));
    let adoption = f.adopt();
    let runtime = f.instance.join("runtime");
    assert!(
        !f.source.exists(),
        "the downloaded directory was transferred, not copied"
    );
    assert_eq!(file_identity(&runtime.join("server.bin")), program_identity);
    assert_eq!(
        fs::read(runtime.join("server.bin")).unwrap(),
        b"original downloaded program"
    );
    assert!(!runtime.join("Saved").exists());
    assert!(!runtime.join("server.cfg").exists());
    let retained = f.instance.join("installation-retained");
    assert_eq!(
        file_identity(&retained.join("Saved/world.sav")),
        world_identity
    );
    assert_eq!(
        fs::read(retained.join("server.cfg")).unwrap(),
        b"operator configuration"
    );
    assert_eq!(
        fs::read(retained.join("Saved/Config/server.ini")).unwrap(),
        b"existing private settings"
    );
    assert!(f.instance.join(ADOPTION_JOURNAL).is_file());
    adoption.commit().unwrap();
    assert!(!f.instance.join(ADOPTION_JOURNAL).exists());
    assert!(!f.source.exists());
    assert_eq!(
        crate::resolve_instance_private_runtime_root(&f.instance).unwrap(),
        runtime
    );
}

#[test]
fn trusted_program_manifest_retains_added_loaders_and_mods_outside_the_fresh_runtime() {
    let f = AdoptionFixture::new();
    fs::create_dir(f.source.join("Bin")).unwrap();
    fs::write(f.source.join("Bin/official.dll"), b"official dependency").unwrap();
    f.record_clean_package();
    // These are added after the official package manifest was recorded. Being
    // outside a game's usual mods directory must not make a loader trusted.
    fs::write(f.source.join("loader.dll"), b"operator loader").unwrap();
    fs::create_dir(f.source.join("ModsExtra")).unwrap();
    fs::write(f.source.join("ModsExtra/mod.dll"), b"extra operator mod").unwrap();
    fs::write(f.source.join("Bin/injected.dll"), b"injected library").unwrap();
    let adoption = f.adopt();
    let runtime = f.instance.join("runtime");
    let retained = f.instance.join("installation-retained");
    assert_eq!(
        fs::read(runtime.join("server.bin")).unwrap(),
        b"original downloaded program"
    );
    assert_eq!(
        fs::read(runtime.join("Bin/official.dll")).unwrap(),
        b"official dependency"
    );
    for (relative, expected) in [
        ("loader.dll", b"operator loader".as_slice()),
        ("ModsExtra/mod.dll", b"extra operator mod".as_slice()),
        ("Bin/injected.dll", b"injected library".as_slice()),
    ] {
        assert!(
            !runtime.join(relative).exists(),
            "unlisted file was inherited: {relative}"
        );
        assert_eq!(fs::read(retained.join(relative)).unwrap(), expected);
    }
    adoption.commit().unwrap();
    assert!(!f.source.exists());
    assert_eq!(
        fs::read(retained.join("Saved/world.sav")).unwrap(),
        b"existing player world"
    );
}

#[test]
fn copy_fallback_uses_the_same_trusted_program_boundary_without_copying_added_mods() {
    let f = AdoptionFixture::new();
    fs::create_dir(f.source.join("Bin")).unwrap();
    fs::write(f.source.join("Bin/official.dll"), b"official dependency").unwrap();
    f.record_clean_package();
    fs::write(f.source.join("loader.dll"), b"unlisted loader").unwrap();
    fs::create_dir(f.source.join("ModsExtra")).unwrap();
    fs::write(f.source.join("ModsExtra/mod.dll"), b"unlisted mod").unwrap();
    fs::write(f.source.join("Bin/injected.dll"), b"unlisted dependency").unwrap();
    // The copy fallback preserves the source library and excludes unlisted files.
    let runtime = prepare_private_runtime_root(
        &f.source,
        &f.instance,
        &f.exclusions(),
        None,
        false,
        crate::program_runtime::ProgramFileSelection::Automatic,
        None,
    )
    .unwrap();
    assert_eq!(
        fs::read(runtime.join("server.bin")).unwrap(),
        b"original downloaded program"
    );
    assert_eq!(
        fs::read(runtime.join("Bin/official.dll")).unwrap(),
        b"official dependency"
    );
    for relative in [
        "Saved/world.sav",
        "server.cfg",
        "loader.dll",
        "ModsExtra/mod.dll",
        "Bin/injected.dll",
    ] {
        assert!(
            !runtime.join(relative).exists(),
            "fallback copied private data: {relative}"
        );
        assert!(
            f.source.join(relative).is_file(),
            "fallback removed source data: {relative}"
        );
    }
    fs::write(runtime.join("server.bin"), b"independently changed program").unwrap();
    f.assert_original();
    assert_eq!(
        fs::read(f.source.join("loader.dll")).unwrap(),
        b"unlisted loader"
    );
    assert!(!f.instance.join(ADOPTION_JOURNAL).exists());
    assert!(!f.instance.join("runtime.staging").exists());
}

#[test]
fn materialization_failure_rolls_back_program_and_all_retained_private_data() {
    let f = AdoptionFixture::new();
    let pending = f.pending(f.adopt());
    pending.rollback().unwrap();
    f.assert_original();
    assert!(!f.instance.exists());
}

#[test]
fn newly_materialized_empty_native_directories_do_not_block_private_data_restore() {
    let f = AdoptionFixture::new();
    let pending = f.pending(f.adopt());
    fs::create_dir_all(f.instance.join("runtime/Saved/Config")).unwrap();
    pending.rollback().unwrap();
    f.assert_original();
    assert!(!f.instance.exists());
}

#[test]
fn rollback_preserves_newly_materialized_data_beside_the_restored_originals() {
    let f = AdoptionFixture::new();
    let pending = f.pending(f.adopt());
    fs::create_dir_all(f.instance.join("runtime/Saved/Config")).unwrap();
    fs::write(
        f.instance.join("runtime/Saved/Config/server.ini"),
        b"newly generated settings",
    )
    .unwrap();
    fs::write(
        f.instance.join("runtime/server.cfg"),
        b"newly generated configuration",
    )
    .unwrap();
    pending.rollback().unwrap();
    f.assert_original();
    assert!(!f.instance.exists());
    let recovery = f.source.join(".langame-creation-recovery");
    let preserved = fs::read_dir(&recovery)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(preserved.len(), 2);
    assert!(preserved.iter().any(|path| {
        fs::read(path.join("Saved/Config/server.ini"))
            .ok()
            .as_deref()
            == Some(b"newly generated settings")
    }));
    assert!(
        preserved
            .iter()
            .any(|path| fs::read(path.join("server.cfg")).ok().as_deref()
                == Some(b"newly generated configuration"))
    );
}

#[test]
fn dropping_an_uncommitted_adoption_restores_the_download_before_cleanup() {
    let f = AdoptionFixture::new();
    let pending = f.pending(f.adopt());
    drop(pending);
    f.assert_original();
    assert!(!f.instance.exists());
}

#[test]
fn conflicting_source_preserves_the_journal_runtime_and_every_private_file() {
    let f = AdoptionFixture::new();
    let pending = f.pending(f.adopt());
    fs::create_dir(&f.source).unwrap();
    fs::write(f.source.join("foreign.dat"), b"concurrent directory owner").unwrap();
    assert!(pending.rollback().is_err());
    assert!(cleanup_owned_instance_directory(&f.instance, &f.instances).is_err());
    assert!(f.instance.join(ADOPTION_JOURNAL).is_file());
    assert_eq!(
        fs::read(f.instance.join("runtime/server.bin")).unwrap(),
        b"original downloaded program"
    );
    assert_eq!(
        fs::read(f.instance.join("installation-retained/Saved/world.sav")).unwrap(),
        b"existing player world"
    );
    assert_eq!(
        fs::read(f.source.join("foreign.dat")).unwrap(),
        b"concurrent directory owner"
    );
}

#[test]
fn cancellation_before_adoption_keeps_the_original_download_intact() {
    let f = AdoptionFixture::new();
    let cancelled = AtomicBool::new(true);
    let error = ProgramAdoption::begin(
        &f.source,
        &f.instance,
        &f.exclusions(),
        false,
        crate::program_runtime::ProgramFileSelection::Automatic,
        Some(&cancelled),
    )
    .unwrap_err();
    assert!(matches!(error, StorageError::InstanceCreationCancelled));
    f.assert_original();
    assert!(!f.instance.join("runtime").exists());
    assert!(!f.instance.join(ADOPTION_JOURNAL).exists());
}

#[test]
fn explicit_local_adoption_keeps_modified_program_and_retains_original_private_data() {
    let f = AdoptionFixture::new();
    f.record_clean_package();
    fs::write(f.source.join("server.bin"), b"operator patched program").unwrap();
    fs::write(f.source.join("loader.dll"), b"operator loader").unwrap();
    let manifest = fs::read(f.source.join(".langame-clean-package.json")).unwrap();
    let adoption = ProgramAdoption::begin(
        &f.source,
        &f.instance,
        &f.exclusions(),
        false,
        crate::program_runtime::ProgramFileSelection::Local,
        None,
    )
    .unwrap()
    .unwrap();
    let runtime = adoption.runtime_root();
    assert_eq!(
        fs::read(runtime.join("server.bin")).unwrap(),
        b"operator patched program"
    );
    assert_eq!(
        fs::read(runtime.join("loader.dll")).unwrap(),
        b"operator loader"
    );
    assert!(!runtime.join("Saved").exists());
    assert!(!runtime.join(".langame-clean-package.json").exists());
    let retained = f.instance.join("installation-retained");
    assert_eq!(
        fs::read(retained.join("Saved/world.sav")).unwrap(),
        b"existing player world"
    );
    assert_eq!(
        fs::read(retained.join("server.cfg")).unwrap(),
        b"operator configuration"
    );
    assert_eq!(
        fs::read(retained.join(".langame-clean-package.json")).unwrap(),
        manifest
    );
    adoption.rollback().unwrap();
    assert_eq!(
        fs::read(f.source.join("server.bin")).unwrap(),
        b"operator patched program"
    );
    assert_eq!(
        fs::read(f.source.join("Saved/world.sav")).unwrap(),
        b"existing player world"
    );
    assert_eq!(
        fs::read(f.source.join(".langame-clean-package.json")).unwrap(),
        manifest
    );
}

#[test]
fn explicit_local_copy_leaves_source_unchanged_and_does_not_claim_clean_provenance() {
    let f = AdoptionFixture::new();
    f.record_clean_package();
    fs::write(f.source.join("server.bin"), b"operator patched program").unwrap();
    let manifest = fs::read(f.source.join(".langame-clean-package.json")).unwrap();
    let runtime = prepare_private_runtime_root(
        &f.source,
        &f.instance,
        &f.exclusions(),
        None,
        false,
        crate::program_runtime::ProgramFileSelection::Local,
        None,
    )
    .unwrap();
    assert_eq!(
        fs::read(runtime.join("server.bin")).unwrap(),
        b"operator patched program"
    );
    assert!(!runtime.join("Saved/world.sav").exists());
    assert!(!runtime.join(".langame-clean-package.json").exists());
    assert_eq!(
        fs::read(f.source.join("Saved/world.sav")).unwrap(),
        b"existing player world"
    );
    assert_eq!(
        fs::read(f.source.join(".langame-clean-package.json")).unwrap(),
        manifest
    );
}

#[test]
fn an_exclusion_count_exceeding_the_recovery_limit_never_moves_the_download() {
    let f = AdoptionFixture::new();
    let mut exclusions = f.exclusions();
    exclusions.extend((0..4_097).map(|index| f.source.join(format!("f{index:04}"))));
    let error = ProgramAdoption::begin(
        &f.source,
        &f.instance,
        &exclusions,
        false,
        crate::program_runtime::ProgramFileSelection::Automatic,
        None,
    )
    .unwrap_err();
    assert!(error.to_string().contains("recovery limit"), "{error}");
    f.assert_original();
    assert!(!f.instance.join("runtime").exists());
    assert!(!f.instance.join("installation-retained").exists());
    assert!(!f.instance.join(ADOPTION_JOURNAL).exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_at_the_real_source_hash_boundary_never_moves_the_download() {
    use crate::instance_creation_io::test_gate::{PausePoint, pause_at};
    let f = AdoptionFixture::new();
    f.record_clean_package();
    let cancellation = Arc::new(AtomicBool::new(false));
    let mut gate = pause_at(&cancellation, PausePoint::Hash);
    let source = f.source.clone();
    let instance = f.instance.clone();
    let exclusions = f.exclusions();
    let worker_token = Arc::clone(&cancellation);
    let worker = tokio::task::spawn_blocking(move || {
        ProgramAdoption::begin(
            &source,
            &instance,
            &exclusions,
            false,
            crate::program_runtime::ProgramFileSelection::Automatic,
            Some(&worker_token),
        )
    });
    gate.reached().await;
    cancellation.store(true, Ordering::Release);
    drop(gate);
    assert!(matches!(
        worker.await.unwrap(),
        Err(StorageError::InstanceCreationCancelled)
    ));
    f.assert_original();
    assert!(!f.instance.join("runtime").exists());
    assert!(!f.instance.join(ADOPTION_JOURNAL).exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_while_verifying_a_trusted_manifest_preserves_the_download_and_private_data() {
    use crate::instance_creation_io::test_gate::{PausePoint, pause_at};
    let f = AdoptionFixture::new();
    f.record_clean_package();
    let cancellation = Arc::new(AtomicBool::new(false));
    let mut gate = pause_at(&cancellation, PausePoint::Hash);
    let source = f.source.clone();
    let instance = f.instance.clone();
    let exclusions = f.exclusions();
    let token = Arc::clone(&cancellation);
    let worker = tokio::task::spawn_blocking(move || {
        ProgramAdoption::begin(
            &source,
            &instance,
            &exclusions,
            false,
            crate::program_runtime::ProgramFileSelection::Verified("fixture"),
            Some(&token),
        )
    });
    gate.reached().await;
    cancellation.store(true, Ordering::Release);
    drop(gate);
    assert!(matches!(
        worker.await.unwrap(),
        Err(StorageError::InstanceCreationCancelled)
    ));
    f.assert_original();
    assert!(f.source.join(".langame-clean-package.json").is_file());
    assert!(!f.instance.join("runtime").exists());
    assert!(!f.instance.join(ADOPTION_JOURNAL).exists());
}
