use super::*;

fn fixture_key() -> PublicKey {
    PublicKey::decode(include_str!(
        "../test-data/enshrouded-extension/public-key.pub"
    ))
    .unwrap()
}

fn fixture() -> SignedPackage {
    verify_package(
        include_bytes!("../test-data/enshrouded-extension/manifest.json").to_vec(),
        include_bytes!("../test-data/enshrouded-extension/manifest.json.minisig").to_vec(),
        include_bytes!("../test-data/enshrouded-extension/payload.txt").to_vec(),
        include_bytes!("../test-data/enshrouded-extension/Lgsm.Enshrouded.Control_1.0.0_windows-x86_64.dll.minisig").to_vec(),
        io::EXE_SHA256, &fixture_key(),
    ).unwrap()
}

struct Scratch(std::path::PathBuf);
impl Scratch {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("lgsm-extension-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn independently_signed_manifest_and_dll_require_exact_build_and_distinct_purposes() {
    let package = fixture();
    assert_eq!(package.manifest.version, "1.0.0");
    assert!(
        verify_package(
            package.manifest_bytes.clone(),
            package.manifest_signature.clone(),
            package.dll.clone(),
            package.dll_signature.clone(),
            &"f".repeat(64),
            &fixture_key()
        )
        .is_err()
    );
    assert!(
        signature(
            &fixture_key(),
            &package.dll,
            &package.dll_signature,
            "lgsm-enshrouded-control-dll:2.0.0"
        )
        .is_err()
    );
    assert!(
        signature(
            &key().unwrap(),
            &package.dll,
            &package.dll_signature,
            "lgsm-enshrouded-control-dll:1.0.0"
        )
        .is_err()
    );
    // A manifest signature cannot authorize a DLL, even if signed by the same key.
    assert!(
        signature(
            &fixture_key(),
            &package.manifest_bytes,
            &package.manifest_signature,
            "lgsm-enshrouded-control-dll:1.0.0"
        )
        .is_err()
    );
}

#[test]
fn tampering_with_either_signed_file_or_signature_is_rejected() {
    let package = fixture();
    for field in 0..4 {
        let (mut manifest, mut manifest_signature, mut dll, mut dll_signature) = (
            package.manifest_bytes.clone(),
            package.manifest_signature.clone(),
            package.dll.clone(),
            package.dll_signature.clone(),
        );
        let bytes = match field {
            0 => &mut manifest,
            1 => &mut manifest_signature,
            2 => &mut dll,
            _ => &mut dll_signature,
        };
        let last = bytes.len() / 2;
        bytes[last] ^= 1;
        assert!(
            verify_package(
                manifest,
                manifest_signature,
                dll,
                dll_signature,
                io::EXE_SHA256,
                &fixture_key()
            )
            .is_err()
        );
    }
}

#[test]
fn offline_cache_revalidates_full_signatures_and_retains_a_signed_revision_watermark() {
    let root = Scratch::new();
    let directory = root.0.join("cache");
    ensure_cache(&directory).unwrap();
    let package = fixture();
    cache(&directory, &package).unwrap();
    let (loaded, revision) = cached(&directory, io::EXE_SHA256, &fixture_key()).unwrap();
    assert_eq!(loaded.unwrap().dll, package.dll);
    assert_eq!(revision, 1);
    let (loaded, revision) = cached(&directory, &"f".repeat(64), &fixture_key()).unwrap();
    assert!(loaded.is_none());
    assert_eq!(revision, 1);
    // Local edits cannot upgrade cache authority. A missing payload still leaves the signed watermark.
    let dll = directory
        .join(io::digest(&package.manifest_bytes))
        .join("dbghelp.dll");
    fs::write(&dll, b"tampered").unwrap();
    let (loaded, revision) = cached(&directory, io::EXE_SHA256, &fixture_key()).unwrap();
    assert!(loaded.is_none());
    assert_eq!(revision, 1);
    fs::remove_file(dll).unwrap();
    assert_eq!(
        cached(&directory, io::EXE_SHA256, &fixture_key())
            .unwrap()
            .1,
        1
    );
}

#[test]
fn installed_payload_requires_signed_evidence_not_an_arbitrary_owner_hash() {
    let root = Scratch::new();
    fs::create_dir(root.0.join(io::DIRECTORY)).unwrap();
    assert!(verify_installed(&root.0, &"f".repeat(64), b"untrusted DLL").is_err());
    let package = fixture();
    package.publish_proof(&root.0.join(io::DIRECTORY)).unwrap();
    // The fixture uses a discarded test key, which the production trust root rejects.
    assert!(verify_installed(&root.0, io::EXE_SHA256, &package.dll).is_err());
}

#[test]
fn asset_identity_cannot_escape_the_fixed_release_namespace() {
    assert!(!version("1.0.0/../../other"));
    assert!(!version("01.0.0"));
    assert!(!version("1.0.0?token=secret"));
    assert!(!sha256("../untrusted"));
    assert!(!sha256(&"F".repeat(64)));
    assert!(version("1.0.0"));
}

#[test]
fn a_valid_old_signature_cannot_roll_back_the_recorded_revision() {
    let package = fixture();
    assert!(
        admit_manifest(
            &package.manifest_bytes,
            &package.manifest_signature,
            &fixture_key(),
            1
        )
        .is_ok()
    );
    assert!(
        admit_manifest(
            &package.manifest_bytes,
            &package.manifest_signature,
            &fixture_key(),
            2
        )
        .is_err()
    );
}

#[test]
fn local_selection_needs_no_network_or_async_runtime() {
    let root = Scratch::new();
    let result = resolve_local(&root.0, io::EXE_SHA256);
    assert_eq!(result.source, "bundled");
    assert!(result.package.is_none());
    assert!(result.update_issue.is_none());
    let result = resolve_local(&root.0, &"f".repeat(64));
    assert_eq!(result.source, "unsupported");
    assert!(result.package.is_none());
}

#[test]
fn an_unusable_optional_cache_still_selects_bundled_support() {
    let root = Scratch::new();
    fs::write(root.0.join("cache"), b"not a directory").unwrap();
    let result = resolve_local(&root.0, io::EXE_SHA256);
    assert_eq!(result.source, "bundled");
    assert!(result.package.is_none());
    assert!(result.update_issue.is_some());
}
