use super::*;
use std::collections::BTreeMap;
use std::sync::atomic::AtomicU64;

// Independent SteamKit wire fixture with x.bin=abc (standard SHA1 vector),
// depot=7, manifest=9. This represents cached native metadata, not a live download.
const MANIFEST: &str = "d017f671230000000a210a05782e62696e100318002a14a9993e364706816aba3e25717850c26c9cd0d89dbe12481f0d000000080710092000280348b8e1fe5317b8811b00000000ab15c432";
const ID: &str = "123456";
static SEQUENCE: AtomicU64 = AtomicU64::new(1);

struct Fixture {
    root: PathBuf,
    install: PathBuf,
    native: PathBuf,
    roots: [PathBuf; 2],
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lg-workshop-recovery-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let install = root.join("game");
        let native = root.join("steamcmd");
        let roots = [
            install.join("steamapps/workshop/content/7"),
            native.join("steamapps/workshop/content/7"),
        ];
        fs::create_dir_all(&roots[0]).unwrap();
        fs::create_dir_all(native.join("depotcache")).unwrap();
        let bytes = MANIFEST
            .as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect::<Vec<_>>();
        fs::write(native.join("depotcache/7_9.manifest"), bytes).unwrap();
        let fixture = Self {
            root,
            install,
            native,
            roots,
        };
        fixture.item(0, b"abc");
        fs::write(fixture.install.join("operator.cfg"), b"keep configuration").unwrap();
        fs::create_dir(fixture.roots[0].join("999999")).unwrap();
        fs::write(
            fixture.roots[0].join("999999/disabled.bin"),
            b"keep disabled",
        )
        .unwrap();
        fixture
    }

    fn manifest(&self, index: usize) -> PathBuf {
        self.roots[index]
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("appworkshop_7.acf")
    }
    fn payload(&self, index: usize) -> PathBuf {
        self.roots[index].join(ID).join("x.bin")
    }
    fn item(&self, index: usize, payload: &[u8]) {
        fs::create_dir_all(self.roots[index].join(ID)).unwrap();
        fs::write(self.payload(index), payload).unwrap();
        fs::write(self.manifest(index), format!(r#""AppWorkshop" {{ "appid" "7" "WorkshopItemsInstalled" {{ "{ID}" {{ "manifest" "9" "size" "3" }} }} "WorkshopItemDetails" {{ "{ID}" {{ "manifest" "10" }} }} }}"#)).unwrap();
    }
    fn prepare(&self) -> WorkshopCacheTransaction {
        WorkshopCacheTransaction::capture(
            7,
            &self.install,
            &self.native,
            &self.roots,
            &[ID.into()],
            &InstallCancellation::new(),
        )
        .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        assert_eq!(self.root.parent(), Some(std::env::temp_dir().as_path()));
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn workshop_failed_update_restores_verified_old_bytes_metadata_and_preserves_other_data() {
    let fixture = Fixture::new();
    let metadata = fs::read(fixture.manifest(0)).unwrap();
    let mut transaction = fixture.prepare();
    fs::write(fixture.payload(0), b"new-partial").unwrap();
    fs::write(fixture.manifest(0), b"partial manifest").unwrap();
    fs::create_dir_all(fixture.roots[1].join(ID)).unwrap();
    fs::write(fixture.payload(1), b"new-partial").unwrap();
    let fallback = transaction.rollback().unwrap().unwrap();
    assert_eq!(fs::read(fixture.payload(0)).unwrap(), b"abc");
    assert_eq!(fs::read(fixture.manifest(0)).unwrap(), metadata);
    assert!(!fixture.roots[1].join(ID).exists());
    assert!(!transaction.recovery_path().exists());
    assert_eq!(fallback.download_result().items.len(), 1);
    assert_eq!(
        fs::read(fixture.install.join("operator.cfg")).unwrap(),
        b"keep configuration"
    );
    assert_eq!(
        fs::read(fixture.roots[0].join("999999/disabled.bin")).unwrap(),
        b"keep disabled"
    );
}

#[test]
fn workshop_total_size_cannot_certify_same_size_corruption_or_missing_hash_evidence() {
    for fault in ["same_size", "missing_hash", "missing_file", "empty"] {
        let fixture = Fixture::new();
        match fault {
            "same_size" => fs::write(fixture.payload(0), b"xyz").unwrap(),
            "missing_hash" => {
                fs::remove_file(fixture.native.join("depotcache/7_9.manifest")).unwrap()
            }
            _ => fs::remove_file(fixture.payload(0)).unwrap(),
        }
        let mut transaction = fixture.prepare();
        assert!(
            transaction.rollback().unwrap().is_none(),
            "{fault} cannot authorize fallback"
        );
    }
}

#[test]
fn workshop_cache_recovery_certifies_complete_alternate_without_adopting_partial_primary() {
    let fixture = Fixture::new();
    fs::write(fixture.payload(0), b"xyz").unwrap();
    fixture.item(1, b"abc");
    let mut transaction = fixture.prepare();
    fs::write(fixture.payload(0), b"partial").unwrap();
    fs::write(fixture.payload(1), b"partial").unwrap();
    let fallback = transaction.rollback().unwrap().unwrap();
    assert_eq!(
        Path::new(&fallback.download_result().items[0].expected_path),
        fixture.roots[1].join(ID)
    );
    assert_eq!(fs::read(fixture.payload(0)).unwrap(), b"xyz");
    assert_eq!(fs::read(fixture.payload(1)).unwrap(), b"abc");
}

#[test]
fn workshop_scoped_fallback_uses_only_adapter_selected_revision_and_can_disallow_fallback() {
    let fixture = Fixture::new();
    fixture.item(1, b"abc");
    for choices in [vec![fixture.roots[1].join(ID)], Vec::new()] {
        let ids = vec![ID.into()];
        let sources = BTreeMap::from([(ID.into(), choices.clone())]);
        let mut transaction = WorkshopCacheTransaction::capture_scoped(
            WorkshopCacheScope {
                consumer_app_id: 7,
                install_root: &fixture.install,
                ids: &ids,
                fallback_sources: Some(&sources),
            },
            &fixture.native,
            &fixture.roots,
            &InstallCancellation::new(),
        )
        .unwrap();
        fs::write(fixture.payload(0), b"partial").unwrap();
        fs::write(fixture.payload(1), b"partial").unwrap();
        let fallback = transaction.rollback().unwrap();
        if choices.is_empty() {
            assert!(fallback.is_none());
        } else {
            assert_eq!(
                Path::new(&fallback.unwrap().download_result().items[0].expected_path),
                &choices[0]
            );
        }
        assert_eq!(fs::read(fixture.payload(0)).unwrap(), b"abc");
        assert_eq!(fs::read(fixture.payload(1)).unwrap(), b"abc");
    }
}

#[test]
fn workshop_transaction_restores_native_membership_with_the_prior_package() {
    let fixture = Fixture::new();
    let cancellation = InstallCancellation::new();
    crate::workshop_instance_membership::record_native_success(
        &fixture.install,
        7,
        ID,
        9,
        &cancellation,
    )
    .unwrap();
    let path =
        crate::workshop_instance_membership::membership_path(&fixture.install, 7, ID).unwrap();
    let old = fs::read(&path).unwrap();
    let mut transaction = fixture.prepare();
    crate::workshop_instance_membership::record_native_success(
        &fixture.install,
        7,
        ID,
        10,
        &cancellation,
    )
    .unwrap();
    assert_ne!(fs::read(&path).unwrap(), old);
    transaction.rollback().unwrap().unwrap();
    assert_eq!(fs::read(path).unwrap(), old);
}

#[test]
fn workshop_corrupted_backup_blocks_recovery_and_retains_both_evidence_and_live_paths() {
    let fixture = Fixture::new();
    let mut transaction = fixture.prepare();
    fs::write(transaction.entries[0].backup.join("x.bin"), b"xyz").unwrap();
    fs::write(fixture.payload(0), b"partial").unwrap();
    assert!(transaction.rollback().is_err());
    assert_eq!(fs::read(fixture.payload(0)).unwrap(), b"partial");
    assert!(transaction.recovery_path().exists());
}

#[test]
fn workshop_actual_deployment_requires_exact_old_files_and_hashes() {
    let fixture = Fixture::new();
    let mut transaction = fixture.prepare();
    let fallback = transaction.rollback().unwrap().unwrap();
    let deployed = fixture.root.join("deployed");
    fs::create_dir(&deployed).unwrap();
    assert!(
        fallback
            .verify_deployed_item(ID, &deployed, &InstallCancellation::new())
            .is_err()
    );
    fs::write(deployed.join("x.bin"), b"xyz").unwrap();
    assert!(
        fallback
            .verify_deployed_item(ID, &deployed, &InstallCancellation::new())
            .is_err()
    );
    fs::write(deployed.join("x.bin"), b"abc").unwrap();
    fallback
        .verify_deployed_item(ID, &deployed, &InstallCancellation::new())
        .unwrap();
    fs::write(deployed.join("extra.bin"), b"extra").unwrap();
    assert!(
        fallback
            .verify_deployed_item(ID, &deployed, &InstallCancellation::new())
            .is_err()
    );
}

#[test]
fn workshop_original_operator_data_is_preserved_without_accepting_new_partial_payloads() {
    let fixture = Fixture::new();
    let mut transaction = fixture.prepare();
    let verified = transaction.rollback().unwrap().unwrap();
    let deployed = fixture.root.join("deployed");
    fs::create_dir_all(deployed.join("Config")).unwrap();
    fs::write(deployed.join("x.bin"), b"abc").unwrap();
    fs::write(deployed.join("Config/user.ini"), b"keep configuration").unwrap();
    let cancellation = InstallCancellation::new();
    let proof = verified
        .protect_deployed_item(ID, &deployed, &cancellation)
        .unwrap()
        .unwrap();
    assert!(
        verified
            .verify_deployed_item(ID, &deployed, &cancellation)
            .is_err(),
        "strict default proof is not weakened"
    );
    proof.verify(&deployed, &cancellation).unwrap();
    assert!(
        proof
            .payload_unchanged(&fixture.roots[0].join(ID), &cancellation)
            .unwrap()
    );
    fs::write(deployed.join("partial.pak"), b"new partial payload").unwrap();
    assert!(
        proof.verify(&deployed, &cancellation).is_err(),
        "later extras must not become operator baseline"
    );
    fs::remove_file(deployed.join("partial.pak")).unwrap();
    fs::write(deployed.join("x.bin"), b"xyz").unwrap();
    assert!(
        verified
            .protect_deployed_item(ID, &deployed, &cancellation)
            .is_err(),
        "ambiguous damaged payload plus user data must be retained, not replaced"
    );
    assert_eq!(
        fs::read(deployed.join("Config/user.ini")).unwrap(),
        b"keep configuration"
    );
}

#[test]
fn uncertified_old_cache_cannot_authorize_removing_unknown_actual_deployment_files() {
    let fixture = Fixture::new();
    fs::remove_file(fixture.native.join("depotcache/7_9.manifest")).unwrap();
    let mut transaction = fixture.prepare();
    assert!(transaction.verified_cache().is_none());
    let deployed = fixture.root.join("deployed");
    fs::create_dir_all(&deployed).unwrap();
    fs::write(deployed.join("x.bin"), b"abc").unwrap();
    let cancellation = InstallCancellation::new();
    assert!(
        transaction
            .protect_uncertified_deployment(ID, &deployed, &cancellation)
            .unwrap()
            .is_none()
    );
    fs::write(deployed.join("user-note.txt"), b"keep original note").unwrap();
    let protection = transaction
        .protect_uncertified_deployment(ID, &deployed, &cancellation)
        .unwrap()
        .unwrap();
    assert!(
        !protection
            .payload_unchanged(&fixture.roots[0].join(ID), &cancellation)
            .unwrap(),
        "unknown file roles cannot authorize root replacement"
    );
    assert!(transaction.rollback().unwrap().is_none());
    assert_eq!(
        fs::read(deployed.join("user-note.txt")).unwrap(),
        b"keep original note"
    );
}

#[test]
fn cancelled_workshop_cache_preparation_never_mutates_old_payload() {
    let fixture = Fixture::new();
    let cancellation = InstallCancellation::new();
    cancellation.cancel();
    let error = WorkshopCacheTransaction::capture(
        7,
        &fixture.install,
        &fixture.native,
        &fixture.roots,
        &[ID.into()],
        &cancellation,
    )
    .err()
    .unwrap();
    assert_eq!(error.kind(), io::ErrorKind::Interrupted);
    assert_eq!(fs::read(fixture.payload(0)).unwrap(), b"abc");
}

#[test]
fn workshop_recovery_stages_each_cache_on_its_own_root_for_cross_volume_safe_renames() {
    let fixture = Fixture::new();
    fixture.item(1, b"abc");
    let mut transaction = fixture.prepare();
    for entry in &transaction.entries {
        let base = if entry.path.starts_with(&fixture.native) {
            &fixture.native
        } else {
            &fixture.install
        };
        assert!(
            entry.backup.starts_with(base),
            "both rollback rename endpoints must remain on the target root's volume"
        );
    }
    fs::write(fixture.payload(0), b"partial primary").unwrap();
    fs::write(fixture.payload(1), b"partial alternate").unwrap();
    let stages = transaction.stages.clone();
    transaction.rollback().unwrap().unwrap();
    assert_eq!(fs::read(fixture.payload(0)).unwrap(), b"abc");
    assert_eq!(fs::read(fixture.payload(1)).unwrap(), b"abc");
    assert!(stages.iter().all(|stage| !stage.exists()));
}

#[test]
fn workshop_copy_errors_keep_native_error_code_for_storage_full_policy() {
    let source = io::Error::from_raw_os_error(112);
    let wrapped = crate::DstWorkshopCacheError::Io {
        operation: "copy file",
        path: PathBuf::from("owned-fixture"),
        source,
    };
    let error = cache_fs_error(wrapped);
    assert_eq!(
        error.raw_os_error(),
        Some(112),
        "IO wrapping must not hide the native disk-full signal"
    );
}

#[cfg(windows)]
#[test]
fn workshop_recovery_preserves_metadata_readonly_and_cleans_only_private_readonly_copies() {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_READONLY, SetFileAttributesW,
    };

    let fixture = Fixture::new();
    let path = fixture.manifest(0);
    let attributes = fs::metadata(&path).unwrap().file_attributes()
        | FILE_ATTRIBUTE_READONLY
        | FILE_ATTRIBUTE_HIDDEN;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // The fixture owns the file and the NUL-terminated path buffer.
    assert_ne!(unsafe { SetFileAttributesW(wide.as_ptr(), attributes) }, 0);
    let mut transaction = fixture.prepare();
    transaction.rollback().unwrap().unwrap();
    assert!(fs::metadata(&path).unwrap().permissions().readonly());
    let mut transaction = fixture.prepare();
    transaction.commit().unwrap();
    assert!(
        fs::metadata(&path).unwrap().permissions().readonly(),
        "cleanup may change only its private copies"
    );
    let attributes = fs::metadata(&path).unwrap().file_attributes();
    assert_ne!(attributes & FILE_ATTRIBUTE_HIDDEN, 0);
    // Release the disposable fixture's attribute using the same cleanup path.
    clear_private_readonly_files(path.parent().unwrap()).unwrap();
    assert_eq!(
        fs::metadata(&path).unwrap().file_attributes(),
        attributes & !FILE_ATTRIBUTE_READONLY,
        "read-only cleanup must preserve the other Windows attributes"
    );
}

#[test]
fn workshop_unconfirmed_prior_operation_preserves_recovery_and_blocks_another_mutation() {
    let fixture = Fixture::new();
    let transaction = fixture.prepare();
    let pending = transaction.recovery_path().to_path_buf();
    drop(transaction);
    let error = WorkshopCacheTransaction::capture(
        7,
        &fixture.install,
        &fixture.native,
        &fixture.roots,
        &[ID.into()],
        &InstallCancellation::new(),
    )
    .err()
    .unwrap();
    assert!(error.to_string().contains("恢复未确认"));
    assert!(pending.exists());
    assert_eq!(fs::read(fixture.payload(0)).unwrap(), b"abc");
}

#[test]
fn verified_dst_fallback_publishes_installed_revision_and_repairs_same_size_corrupt_target() {
    let fixture = Fixture::new();
    let workshop = fixture.install.join("steamapps/workshop");
    let payload = workshop.join("content/322330").join(ID);
    fs::create_dir_all(&payload).unwrap();
    fs::write(payload.join("modinfo.lua"), b"abc").unwrap();
    let metadata = format!(
        r#""AppWorkshop" {{ "appid" "322330" "WorkshopItemsInstalled" {{ "{ID}" {{ "manifest" "9" "size" "3" }} }} "WorkshopItemDetails" {{ "{ID}" {{ "manifest" "10" }} }} }}"#
    );
    fs::write(workshop.join("appworkshop_322330.acf"), &metadata).unwrap();
    let verified = VerifiedWorkshopCache {
        result: SteamWorkshopDownloadResult {
            consumer_app_id: 322330,
            install_root: fixture.install.to_string_lossy().into_owned(),
            workshop_root: workshop
                .join("content/322330")
                .to_string_lossy()
                .into_owned(),
            items: vec![SteamWorkshopDownloadItemResult {
                item_id: ID.into(),
                expected_path: payload.to_string_lossy().into_owned(),
                expected_path_exists: true,
            }],
            output_excerpt: String::new(),
        },
        items: vec![VerifiedItem {
            id: ID.into(),
            path: payload,
            files: BTreeMap::from([(
                "modinfo.lua".into(),
                integrity::FileDigest {
                    size: 3,
                    sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
                        .into(),
                },
            )]),
        }],
    };
    let target = fixture.root.join("ugc");
    assert!(
        crate::deploy_dst_workshop_cache(&workshop, &target, &[ID.into()]).is_err(),
        "normal current-version publication remains strict"
    );
    crate::deploy_verified_dst_workshop_cache(&workshop, &target, &[ID.into()], &verified).unwrap();
    assert_eq!(
        fs::read_to_string(workshop.join("appworkshop_322330.acf")).unwrap(),
        metadata,
        "source's latest-known metadata is preserved"
    );
    let deployed = target.join("content/322330").join(ID);
    verified
        .verify_deployed_item(ID, &deployed, &InstallCancellation::new())
        .unwrap();
    fs::write(deployed.join("modinfo.lua"), b"xyz").unwrap();
    let repaired =
        crate::deploy_verified_dst_workshop_cache(&workshop, &target, &[ID.into()], &verified)
            .unwrap();
    assert!(
        !repaired.unchanged,
        "byte count alone cannot skip a corrupt deployment"
    );
    assert_eq!(fs::read(deployed.join("modinfo.lua")).unwrap(), b"abc");
}
