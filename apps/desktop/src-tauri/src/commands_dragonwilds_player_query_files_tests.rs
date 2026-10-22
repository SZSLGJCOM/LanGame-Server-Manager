use super::*;
use std::path::PathBuf;

#[path = "commands_dragonwilds_startup_tests.rs"]
mod startup;

const PROXY: &[u8] = b"MZfixture-proxy";
const REBUILT_PROXY: &[u8] = b"MZsame-reviewed-source-different-PE-output";
const SERVER: &[u8] = b"MZfixture-shipping";
const UE4SS_LICENSE: &[u8] = b"fixture UE4SS proxy license";
const FMT_LICENSE: &[u8] = b"fixture fmt license";

// These fixtures exercise the installer and protocol, not real UE4SS loading.
struct Fixture {
    root: PathBuf,
    payload: BTreeMap<String, Vec<u8>>,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lgsm-dragonwilds-files-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(root.join(WIN64)).unwrap();
        fs::write(root.join("RSDragonwildsServer.exe"), b"MZfixture-bootstrap").unwrap();
        fs::write(
            root.join(WIN64)
                .join("RSDragonwildsServer-Win64-Shipping.exe"),
            SERVER,
        )
        .unwrap();
        Self {
            root,
            payload: BTreeMap::from([
                ("UE4SS.dll".into(), b"MZfixture-core".to_vec()),
                ("LICENSE".into(), b"fixture upstream license".to_vec()),
                (
                    "UE4SS_SDK_Backends/UE4SS.json".into(),
                    b"{\"fixture\":true}".to_vec(),
                ),
            ]),
        }
    }

    fn with_release<T>(&self, operation: impl FnOnce(&[(&str, usize, &str)]) -> T) -> T {
        let values = self
            .payload
            .iter()
            .map(|(name, bytes)| (name.as_str(), bytes.len(), digest(bytes)))
            .collect::<Vec<_>>();
        let release = values
            .iter()
            .map(|(name, size, hash)| (*name, *size, hash.as_str()))
            .collect::<Vec<_>>();
        operation(&release)
    }

    fn inspect(&self) -> Result<Preparation, String> {
        self.with_release(|release| inspect_release(&self.root, PROXY, &[&digest(SERVER)], release))
    }

    fn install(&self) -> Result<(), String> {
        self.with_release(|release| {
            install_payload(
                &self.root,
                PROXY,
                UE4SS_LICENSE,
                FMT_LICENSE,
                self.payload.clone(),
                release,
            )
        })
    }

    fn intent(&self) -> Ownership {
        let owner = expected_files(PROXY, UE4SS_LICENSE, FMT_LICENSE, &self.payload);
        fs::write(
            self.root.join(INTENT),
            encode_intent(&owner, PROXY).unwrap(),
        )
        .unwrap();
        owner
    }

    fn proxy(&self) -> PathBuf {
        self.root.join(WIN64).join("version.dll")
    }
    fn loader(&self) -> PathBuf {
        self.root.join(WIN64).join(LOADER)
    }

    fn archive(&self, extra: bool) -> PathBuf {
        let path = self.root.join("fixture.zip");
        let mut archive = zip::ZipWriter::new(File::create(&path).unwrap());
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, bytes) in &self.payload {
            archive
                .start_file(format!("ue4ss/{name}"), options)
                .unwrap();
            archive.write_all(bytes).unwrap();
        }
        if extra {
            for name in [
                "dwmapi.dll",
                "ue4ss/Mods/Unwanted/Scripts/main.lua",
                "../../escaped.dll",
            ] {
                archive.start_file(name, options).unwrap();
                archive.write_all(b"unselected payload").unwrap();
            }
        }
        archive.finish().unwrap();
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn fresh_verified_server_needs_download_and_unknown_build_stays_native() {
    let fixture = Fixture::new();
    assert_eq!(fixture.inspect().unwrap(), Preparation::NeedsInstall);
    fs::write(
        fixture
            .root
            .join(WIN64)
            .join("RSDragonwildsServer-Win64-Shipping.exe"),
        b"MZnew-build",
    )
    .unwrap();
    assert_eq!(fixture.inspect().unwrap(), Preparation::UnsupportedBuild);
    assert!(!fixture.root.join(INTENT).exists());
    assert!(!fixture.proxy().exists());
}

#[test]
fn external_loader_files_are_preserved_without_download_or_ownership() {
    for name in ["version.dll", "dwmapi.dll", "ue4ss"] {
        let fixture = Fixture::new();
        let path = fixture.root.join(WIN64).join(name);
        if name == "ue4ss" {
            fs::create_dir(&path).unwrap();
            fs::write(path.join("operator.ini"), b"operator settings").unwrap();
        } else {
            fs::write(&path, b"operator DLL").unwrap();
        }
        assert_eq!(fixture.inspect().unwrap(), Preparation::ExternalLoader);
        assert!(!fixture.root.join(INTENT).exists());
        assert_eq!(
            fs::read(if name == "ue4ss" {
                path.join("operator.ini")
            } else {
                path
            })
            .unwrap(),
            if name == "ue4ss" {
                b"operator settings".as_slice()
            } else {
                b"operator DLL".as_slice()
            }
        );
    }
}

#[test]
fn installer_is_idempotent_and_native_runtime_output_is_allowed() {
    let fixture = Fixture::new();
    fixture.install().unwrap();
    assert_eq!(fixture.inspect().unwrap(), Preparation::Ready);
    fs::write(fixture.loader().join("UE4SS.log"), b"native runtime log").unwrap();
    let foreign_mod = fixture.loader().join("Mods/OperatorMod");
    fs::create_dir(&foreign_mod).unwrap();
    fs::write(foreign_mod.join("enabled.txt"), b"operator").unwrap();
    assert_eq!(fixture.inspect().unwrap(), Preparation::Ready);
    fixture.install().unwrap();
    assert_eq!(
        fs::read(fixture.loader().join("UE4SS.log")).unwrap(),
        b"native runtime log"
    );
    assert_eq!(
        fs::read(foreign_mod.join("enabled.txt")).unwrap(),
        b"operator"
    );
    assert_eq!(fs::read(fixture.proxy()).unwrap(), PROXY);
    assert!(!fixture.root.join(INTENT).exists());
    assert!(fixture.root.join("langame_player_query").is_dir());
    assert!(!fixture.root.join(WIN64).join("dwmapi.dll").exists());
}

#[test]
fn rebuilt_bundled_proxy_keeps_complete_verified_existing_installation_ready() {
    let fixture = Fixture::new();
    fixture.install().unwrap();
    let receipt = fs::read(fixture.root.join(RECEIPT)).unwrap();
    fixture.with_release(|release| {
        assert_eq!(
            inspect_release(&fixture.root, REBUILT_PROXY, &[&digest(SERVER)], release).unwrap(),
            Preparation::Ready
        );
        install_payload(
            &fixture.root,
            REBUILT_PROXY,
            UE4SS_LICENSE,
            FMT_LICENSE,
            fixture.payload.clone(),
            release,
        )
        .unwrap();
    });
    assert_eq!(fs::read(fixture.proxy()).unwrap(), PROXY);
    assert_eq!(fs::read(fixture.loader().join(PROXY_COPY)).unwrap(), PROXY);
    assert_eq!(fs::read(fixture.root.join(RECEIPT)).unwrap(), receipt);
}

#[test]
fn retired_proxy_restores_its_recorded_bytes_after_application_rebuild() {
    let fixture = Fixture::new();
    fixture.install().unwrap();
    let shipping = fixture
        .root
        .join(WIN64)
        .join("RSDragonwildsServer-Win64-Shipping.exe");
    fs::write(&shipping, b"MZunknown-update").unwrap();
    fixture.with_release(|release| {
        assert_eq!(
            inspect_release(&fixture.root, REBUILT_PROXY, &[&digest(SERVER)], release).unwrap(),
            Preparation::UnsupportedBuild
        );
        assert!(!fixture.proxy().exists());
        assert_eq!(fs::read(fixture.loader().join(PROXY_COPY)).unwrap(), PROXY);
        fs::write(&shipping, SERVER).unwrap();
        assert_eq!(
            inspect_release(&fixture.root, REBUILT_PROXY, &[&digest(SERVER)], release).unwrap(),
            Preparation::NeedsInstall
        );
        install_payload(
            &fixture.root,
            REBUILT_PROXY,
            UE4SS_LICENSE,
            FMT_LICENSE,
            fixture.payload.clone(),
            release,
        )
        .unwrap();
        assert_eq!(
            inspect_release(&fixture.root, REBUILT_PROXY, &[&digest(SERVER)], release).unwrap(),
            Preparation::Ready
        );
    });
    assert_eq!(fs::read(fixture.proxy()).unwrap(), PROXY);
}

#[test]
fn rebuilt_application_recovers_from_intent_before_any_proxy_copy_was_written() {
    let fixture = Fixture::new();
    let mut original_proxy = vec![0x5a; 32 * 1024];
    original_proxy[..2].copy_from_slice(b"MZ");
    let owner = expected_files(
        &original_proxy,
        UE4SS_LICENSE,
        FMT_LICENSE,
        &fixture.payload,
    );
    fs::write(
        fixture.root.join(INTENT),
        encode_intent(&owner, &original_proxy).unwrap(),
    )
    .unwrap();
    assert!(!fixture.root.join(WIN64).join(STAGE).exists());
    assert!(!fixture.loader().exists());
    assert!(!fixture.proxy().exists());
    fixture.with_release(|release| {
        assert_eq!(
            inspect_release(&fixture.root, REBUILT_PROXY, &[&digest(SERVER)], release).unwrap(),
            Preparation::NeedsInstall
        );
        install_payload(
            &fixture.root,
            REBUILT_PROXY,
            UE4SS_LICENSE,
            FMT_LICENSE,
            fixture.payload.clone(),
            release,
        )
        .unwrap();
        assert_eq!(
            inspect_release(&fixture.root, REBUILT_PROXY, &[&digest(SERVER)], release).unwrap(),
            Preparation::Ready
        );
    });
    assert_eq!(fs::read(fixture.proxy()).unwrap(), original_proxy);
    assert_eq!(
        fs::read(fixture.loader().join(PROXY_COPY)).unwrap(),
        original_proxy
    );
    let receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.root.join(RECEIPT)).unwrap()).unwrap();
    assert!(receipt.get("pending_proxy_base64").is_none());
    assert!(!fixture.root.join(INTENT).exists());
}

#[test]
fn changed_or_oversized_intent_recovery_bytes_are_rejected_before_publication() {
    let fixture = Fixture::new();
    fixture.intent();
    let mut owner = ownership(&fixture.root, INTENT).unwrap().unwrap();
    owner.pending_proxy_base64 =
        Some(base64::engine::general_purpose::STANDARD.encode(b"MZchanged"));
    fs::write(
        fixture.root.join(INTENT),
        serde_json::to_vec(&owner).unwrap(),
    )
    .unwrap();
    assert!(fixture.inspect().unwrap_err().contains("recovery bytes"));
    owner.pending_proxy_base64 = Some("A".repeat(MAX_ENCODED_PROXY + 1));
    assert!(
        pending_proxy(&owner)
            .unwrap_err()
            .contains("recovery limit")
    );
    assert!(!fixture.proxy().exists());
    assert!(!fixture.loader().exists());
}

#[test]
fn incomplete_intent_and_staging_are_recovered_before_proxy_publication() {
    let fixture = Fixture::new();
    fixture.intent();
    let stage = fixture.root.join(WIN64).join(STAGE);
    fs::create_dir(&stage).unwrap();
    fs::write(stage.join("UE4SS.dll"), &fixture.payload["UE4SS.dll"]).unwrap();
    fs::write(
        stage.join(".0123456789abcdef0123456789abcdef.pending"),
        b"partial unpublished copy",
    )
    .unwrap();
    assert_eq!(fixture.inspect().unwrap(), Preparation::NeedsInstall);
    assert!(!fixture.proxy().exists());
    fixture.install().unwrap();
    assert_eq!(fixture.inspect().unwrap(), Preparation::Ready);
    assert!(
        !fixture
            .loader()
            .join(".0123456789abcdef0123456789abcdef.pending")
            .exists()
    );
    assert!(!stage.exists());
}

#[test]
fn interruption_after_loader_or_proxy_publication_can_finalize() {
    for proxy_published in [false, true] {
        let fixture = Fixture::new();
        fixture.install().unwrap();
        let receipt = fs::read(fixture.root.join(RECEIPT)).unwrap();
        fs::write(fixture.root.join(INTENT), receipt).unwrap();
        fs::remove_file(fixture.root.join(RECEIPT)).unwrap();
        if !proxy_published {
            fs::remove_file(fixture.proxy()).unwrap();
        }
        assert_eq!(fixture.inspect().unwrap(), Preparation::NeedsInstall);
        fixture.install().unwrap();
        assert_eq!(fixture.inspect().unwrap(), Preparation::Ready);
        assert!(!fixture.root.join(INTENT).exists());
    }
}

#[test]
fn failed_exchange_preparation_never_publishes_a_loadable_proxy() {
    let fixture = Fixture::new();
    fs::write(
        fixture.root.join("langame_player_query"),
        b"operator obstruction",
    )
    .unwrap();
    assert!(fixture.install().is_err());
    assert!(!fixture.proxy().exists());
    assert!(fixture.root.join(INTENT).is_file());
    assert_eq!(
        fs::read(fixture.root.join("langame_player_query")).unwrap(),
        b"operator obstruction"
    );
    fs::remove_file(fixture.root.join("langame_player_query")).unwrap();
    fixture.install().unwrap();
    assert_eq!(fixture.inspect().unwrap(), Preparation::Ready);
}

#[test]
fn modified_published_core_and_proxy_are_rejected_without_overwrite() {
    for relative in [
        "ue4ss/UE4SS.dll",
        "version.dll",
        "ue4ss/UE4SS-settings.ini",
        "ue4ss/LgsmPlayerQueryProxy.bin",
    ] {
        let fixture = Fixture::new();
        fixture.install().unwrap();
        let path = fixture.root.join(WIN64).join(relative);
        fs::write(&path, b"user changed").unwrap();
        assert!(fixture.inspect().is_err());
        assert_eq!(fs::read(&path).unwrap(), b"user changed");
    }
}

#[test]
fn updated_game_retires_only_owned_proxy_and_supported_build_can_restore_it() {
    let fixture = Fixture::new();
    fixture.install().unwrap();
    let shipping = fixture
        .root
        .join(WIN64)
        .join("RSDragonwildsServer-Win64-Shipping.exe");
    fs::write(&shipping, b"MZunknown-update").unwrap();
    assert_eq!(fixture.inspect().unwrap(), Preparation::UnsupportedBuild);
    assert!(!fixture.proxy().exists());
    assert!(fixture.loader().join("UE4SS.dll").exists());
    assert_eq!(fixture.inspect().unwrap(), Preparation::UnsupportedBuild);
    fs::write(&shipping, SERVER).unwrap();
    assert_eq!(fixture.inspect().unwrap(), Preparation::NeedsInstall);
    fixture.install().unwrap();
    assert_eq!(fixture.inspect().unwrap(), Preparation::Ready);
}

#[test]
fn unsupported_build_never_deletes_a_modified_owned_proxy() {
    let fixture = Fixture::new();
    fixture.install().unwrap();
    fs::write(fixture.proxy(), b"operator replaced proxy").unwrap();
    fs::write(
        fixture
            .root
            .join(WIN64)
            .join("RSDragonwildsServer-Win64-Shipping.exe"),
        b"MZunknown-update",
    )
    .unwrap();
    assert!(fixture.inspect().is_err());
    assert_eq!(
        fs::read(fixture.proxy()).unwrap(),
        b"operator replaced proxy"
    );
}

#[test]
fn zip_size_digest_and_selected_file_digest_are_independently_checked() {
    let fixture = Fixture::new();
    let archive = fixture.archive(false);
    let size = fs::metadata(&archive).unwrap().len();
    let hash = digest(&fs::read(&archive).unwrap());
    fixture.with_release(|release| {
        assert!(
            read_archive(&fs::read(&archive).unwrap(), size + 1, &hash, release)
                .unwrap_err()
                .contains("size or SHA-256")
        );
        assert!(
            read_archive(&fs::read(&archive).unwrap(), size, &"0".repeat(64), release)
                .unwrap_err()
                .contains("size or SHA-256")
        );
        let altered = [("UE4SS.dll", fixture.payload["UE4SS.dll"].len(), "invalid")];
        assert!(
            read_archive(&fs::read(&archive).unwrap(), size, &hash, &altered)
                .unwrap_err()
                .contains("payload checksum")
        );
        assert_eq!(
            read_archive(&fs::read(&archive).unwrap(), size, &hash, release).unwrap(),
            fixture.payload
        );
    });
    assert!(
        read_archive(
            &fs::read(&archive).unwrap(),
            ARCHIVE_BYTES,
            ARCHIVE_SHA,
            RELEASE
        )
        .is_err()
    );
    assert!(!fixture.proxy().exists());
}

#[test]
fn only_reviewed_archive_members_are_read_and_installed() {
    let fixture = Fixture::new();
    let archive = fixture.archive(true);
    let body = fs::read(&archive).unwrap();
    fixture.with_release(|release| {
        let payload = read_archive(
            &fs::read(&archive).unwrap(),
            body.len() as u64,
            &digest(&body),
            release,
        )
        .unwrap();
        assert_eq!(payload, fixture.payload);
        install_payload(
            &fixture.root,
            PROXY,
            UE4SS_LICENSE,
            FMT_LICENSE,
            payload,
            release,
        )
        .unwrap();
    });
    assert!(!fixture.root.join(WIN64).join("dwmapi.dll").exists());
    assert!(!fixture.loader().join("Mods/Unwanted").exists());
    assert!(!fixture.root.join("escaped.dll").exists());
}

#[test]
fn invalid_ownership_and_non_pe_programs_fail_closed() {
    let fixture = Fixture::new();
    fs::write(fixture.root.join(INTENT), b"{\"schema\":2}").unwrap();
    assert!(fixture.inspect().is_err());
    fs::remove_file(fixture.root.join(INTENT)).unwrap();
    fs::write(
        fixture.root.join("RSDragonwildsServer.exe"),
        b"not a program",
    )
    .unwrap();
    assert!(fixture.inspect().is_err());
    assert!(!fixture.proxy().exists());
}

#[test]
fn player_query_accepts_only_the_reviewed_shipping_candidates() {
    let fixture = Fixture::new();
    let next = b"MZ-reviewed-next-shipping";
    let old_hash = digest(SERVER);
    let next_hash = digest(next);
    let supported = [old_hash.as_str(), next_hash.as_str()];
    assert!(program_supported(&fixture.root, &supported).unwrap());
    let path = fixture
        .root
        .join(WIN64)
        .join("RSDragonwildsServer-Win64-Shipping.exe");
    fs::write(&path, next).unwrap();
    assert!(program_supported(&fixture.root, &supported).unwrap());
    fs::write(path, b"MZ-unreviewed-next-shipping").unwrap();
    assert!(!program_supported(&fixture.root, &supported).unwrap());
}
