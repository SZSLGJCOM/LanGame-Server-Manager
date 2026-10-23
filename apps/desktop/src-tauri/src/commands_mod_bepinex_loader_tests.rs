use super::*;

const MONO_CONFIG: &[u8] = b"# operator note\r\n[General]\r\nenabled = true\r\ntarget_assembly=BepInEx\\core\\BepInEx.Preloader.dll\r\nredirect_output_log = false\r\n";
const IL2CPP_CONFIG: &[u8] = b"# operator note\r\n[General]\r\nenabled = true\r\ntarget_assembly=BepInEx\\core\\BepInEx.Unity.IL2CPP.dll\r\n[Il2Cpp]\r\ncoreclr_path=dotnet\\coreclr.dll\r\ncorlib_dir=dotnet\r\n";

struct Fixture {
    directory: PathBuf,
    root: PathBuf,
    archive: PathBuf,
    game: &'static ThunderstoreGameProfile,
    loader: BepInExProfile,
    config: &'static [u8],
}

impl Fixture {
    fn new(module_id: &str) -> Self {
        Self::with_extra(module_id, &[], false)
    }

    fn with_extra(module_id: &str, extra: &[(&str, &[u8])], large_coreclr: bool) -> Self {
        let game = thunderstore_game_profile(module_id).unwrap();
        let mut loader = game.loader.unwrap().clone();
        let directory = staging_tests::staging_test_root("bepinex-loader");
        let root = directory.join("runtime");
        fs::create_dir(&root).unwrap();
        fs::write(root.join(game.executable), b"MZserver").unwrap();
        let config = if loader.il2cpp {
            IL2CPP_CONFIG
        } else {
            MONO_CONFIG
        };
        let mut immutable = vec![
            (".doorstop_version".to_string(), b"4.4.0".to_vec()),
            ("winhttp.dll".to_string(), b"MZproxy".to_vec()),
            (
                loader.expected_assembly.to_string(),
                b"MZpreloader".to_vec(),
            ),
        ];
        if loader.il2cpp {
            immutable.push((
                "dotnet/coreclr.dll".into(),
                if large_coreclr {
                    vec![0x5a; 23 * 1024 * 1024]
                } else {
                    b"MZcoreclr".to_vec()
                },
            ));
            immutable.push(("dotnet/mscorlib.dll".into(), b"MZcorlib".to_vec()));
        } else {
            immutable.push(("BepInEx/core/BepInEx.dll".into(), b"MZbepinex".to_vec()));
        }
        loader.files = immutable
            .iter()
            .map(|(path, bytes)| {
                (
                    path.clone(),
                    Sha256::digest(bytes)
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect::<String>(),
                )
            })
            .collect();
        let archive = directory.join("loader.zip");
        let mut zip = zip::ZipWriter::new(fs::File::create(&archive).unwrap());
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file("README.md", options).unwrap();
        zip.write_all(b"package documentation").unwrap();
        for (path, bytes) in immutable
            .iter()
            .map(|(path, bytes)| (path.as_str(), bytes.as_slice()))
            .chain([
                (CONFIG, config),
                (
                    BEPINEX_CONFIG,
                    b"[Chainloader]\nHideManagerGameObject=true".as_slice(),
                ),
            ])
            .chain(extra.iter().copied())
        {
            zip.start_file(format!("{}/{path}", loader.prefix), options)
                .unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
        let (hash, bytes) = fingerprint_file(fs::File::open(&archive).unwrap(), u64::MAX).unwrap();
        loader.archive_sha256 = hash;
        loader.archive_bytes = bytes;
        Self {
            directory,
            root,
            archive,
            game,
            loader,
            config,
        }
    }

    fn target(&self) -> ResolvedManualModTarget {
        ResolvedManualModTarget {
            instance_id: "loader-instance".into(),
            module_id: self.game.module_id.into(),
            source_label: "Thunderstore".into(),
            target_label: self.game.target_relative.into(),
            target_path: self.root.join(self.game.target_relative),
            accepts: vec!["zip".into(), "dll".into()],
            id_strategy: None,
        }
    }

    fn dependency(&self) -> String {
        format!("{}-{}", self.loader.project, self.loader.version)
    }

    fn source(&self, dependencies: Vec<String>) -> DownloadedManualModSource {
        let path = self.directory.join("downloaded-plugin");
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("Plugin.dll"), b"MZplugin").unwrap();
        DownloadedManualModSource {
            path,
            label: "Author-Plugin".into(),
            identity: OnlineModIdentity {
                thunderstore_intent: None,
                provider: "thunderstore".into(),
                project: "Author-Plugin".into(),
                historical_names: vec!["Author-Plugin".into()],
                version: "1.0.0".into(),
                dependencies,
            },
        }
    }

    fn install(
        &self,
        sources: &[DownloadedManualModSource],
        archive: Option<&Path>,
    ) -> Result<ManualModStageResult, String> {
        stage_with_profile(self.target(), sources, archive, self.game, &self.loader)
    }

    fn install_existing_loader(&self) {
        let mut zip = zip::ZipArchive::new(fs::File::open(&self.archive).unwrap()).unwrap();
        for relative in self.loader.files.keys() {
            let mut entry = zip
                .by_name(&format!("{}/{relative}", self.loader.prefix))
                .unwrap();
            let path = self.root.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::io::copy(&mut entry, &mut fs::File::create(path).unwrap()).unwrap();
        }
        fs::write(self.root.join(CONFIG), self.config).unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn optional_loader_fallback_requires_absence_or_a_complete_owned_disabled_loader() {
    for module in ["valheim", "vrising"] {
        let fixture = Fixture::new(module);
        verify_inactive_loader(&fixture.root, &fixture.loader).unwrap();
        fixture.install(&[], Some(&fixture.archive)).unwrap();
        let receipt = fs::read(fixture.root.join(fixture.loader.receipt)).unwrap();
        assert!(verify_inactive_loader(&fixture.root, &fixture.loader).is_err());
        let disabled = String::from_utf8(fixture.config.to_vec())
            .unwrap()
            .replace("enabled = true", "enabled = false");
        fs::write(fixture.root.join(CONFIG), &disabled).unwrap();
        verify_inactive_loader(&fixture.root, &fixture.loader).unwrap();
        assert_eq!(
            fs::read(fixture.root.join(CONFIG)).unwrap(),
            disabled.as_bytes()
        );
        assert_eq!(
            fs::read(fixture.root.join(fixture.loader.receipt)).unwrap(),
            receipt
        );
        fs::remove_file(fixture.root.join(fixture.loader.expected_assembly)).unwrap();
        assert!(verify_inactive_loader(&fixture.root, &fixture.loader).is_err());
        assert!(fixture.root.join("winhttp.dll").is_file());
    }
    for path in ["winhttp.dll", CONFIG, "BepInEx/core/BepInEx.dll"] {
        let fixture = Fixture::new("valheim");
        let file = fixture.root.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, b"operator content").unwrap();
        assert!(
            verify_inactive_loader(&fixture.root, &fixture.loader).is_err(),
            "{path}"
        );
        assert_eq!(fs::read(file).unwrap(), b"operator content");
    }
}

#[test]
fn bepinex_loader_and_plugin_publish_together_without_changing_native_files() {
    for module_id in ["valheim", "vrising"] {
        let fixture = Fixture::new(module_id);
        fs::write(fixture.root.join("world.db"), b"world data").unwrap();
        let result = fixture
            .install(
                &[fixture.source(vec![fixture.dependency()])],
                Some(&fixture.archive),
            )
            .unwrap();
        assert_eq!(result.affected_root_names, ["thunderstore-Author-Plugin"]);
        assert_eq!(
            fs::read(
                fixture
                    .target()
                    .target_path
                    .join("thunderstore-Author-Plugin/Plugin.dll")
            )
            .unwrap(),
            b"MZplugin"
        );
        assert_eq!(
            fs::read(fixture.root.join(fixture.game.executable)).unwrap(),
            b"MZserver"
        );
        assert_eq!(
            fs::read(fixture.root.join("world.db")).unwrap(),
            b"world data"
        );
        assert!(!fixture.root.join("README.md").exists());
        assert!(verified_receipt(&fixture.root, &fixture.loader).unwrap());
        crate::commands::commands_mods::mod_online::validate_runtime(
            module_id,
            "thunderstore",
            &fixture.root,
            &fixture.target().target_path,
        )
        .unwrap();
        if module_id == "vrising" {
            assert!(fixture.root.join("dotnet/coreclr.dll").is_file());
        }
        staging_tests::assert_no_staging_transaction(&fixture.directory);
    }
}

#[test]
fn bepinex_plugin_preparation_failure_does_not_enable_the_loader() {
    for module_id in ["valheim", "vrising"] {
        let fixture = Fixture::new(module_id);
        let error = fixture
            .install(
                &[fixture.source(vec!["Missing-Dependency-1.2.3".into()])],
                Some(&fixture.archive),
            )
            .unwrap_err();
        assert!(error.contains("mod_dependencies_unverified"));
        assert!(!fixture.root.join("winhttp.dll").exists());
        assert!(!fixture.root.join("BepInEx").exists());
        assert!(!fixture.root.join(fixture.loader.receipt).exists());
        assert!(!fixture.root.join("dotnet").exists());
        staging_tests::assert_no_staging_transaction(&fixture.directory);
    }
}

#[test]
fn bepinex_loader_requires_the_pinned_archive_before_any_runtime_mutation() {
    let fixture = Fixture::new("valheim");
    let mut bytes = fs::read(&fixture.archive).unwrap();
    bytes[0] ^= 1;
    fs::write(&fixture.archive, &bytes).unwrap();
    assert!(
        fixture
            .install(&[], Some(&fixture.archive))
            .unwrap_err()
            .contains("checksum")
    );
    assert!(!fixture.root.join("winhttp.dll").exists());
    assert!(
        fixture
            .install(&[], None)
            .unwrap_err()
            .contains("pinned archive")
    );
    assert!(!fixture.root.join(fixture.loader.receipt).exists());
    staging_tests::assert_no_staging_transaction(&fixture.directory);
}

#[test]
fn bepinex_loader_rejects_unknown_payload_and_preserves_partial_existing_loader() {
    let unexpected = Fixture::with_extra(
        "valheim",
        &[("BepInEx/plugins/Injected.dll", b"MZunknown")],
        false,
    );
    assert!(
        unexpected
            .install(&[], Some(&unexpected.archive))
            .unwrap_err()
            .contains("Unexpected loader payload")
    );
    assert!(!unexpected.root.join("winhttp.dll").exists());
    for module_id in ["valheim", "vrising"] {
        let fixture = Fixture::new(module_id);
        fs::write(fixture.root.join("winhttp.dll"), b"MZoperator-loader").unwrap();
        assert!(
            fixture
                .install(&[], Some(&fixture.archive))
                .unwrap_err()
                .contains("conflicts")
        );
        assert_eq!(
            fs::read(fixture.root.join("winhttp.dll")).unwrap(),
            b"MZoperator-loader"
        );
        assert!(!fixture.root.join(fixture.loader.receipt).exists());
    }
}

#[test]
fn bepinex_adopts_exact_loader_and_only_enables_doorstop_preserving_user_configuration() {
    for module_id in ["valheim", "vrising"] {
        let fixture = Fixture::new(module_id);
        fixture.install_existing_loader();
        let disabled = String::from_utf8(fixture.config.to_vec())
            .unwrap()
            .replace("enabled = true", "enabled = FALSE");
        fs::write(fixture.root.join(CONFIG), disabled).unwrap();
        let user_config = b"[Logging.Console]\r\nEnabled=false\r\n# operator's preferences\r\n";
        fs::create_dir_all(fixture.root.join("BepInEx/config")).unwrap();
        fs::write(fixture.root.join(BEPINEX_CONFIG), user_config).unwrap();
        fs::create_dir_all(fixture.root.join("BepInEx/plugins/local-plugin")).unwrap();
        fs::write(
            fixture.root.join("BepInEx/plugins/local-plugin/User.dll"),
            b"MZuser",
        )
        .unwrap();
        fixture.install(&[], Some(&fixture.archive)).unwrap();
        assert_eq!(fs::read(fixture.root.join(CONFIG)).unwrap(), fixture.config);
        assert_eq!(
            fs::read(fixture.root.join(BEPINEX_CONFIG)).unwrap(),
            user_config
        );
        assert_eq!(
            fs::read(fixture.root.join("BepInEx/plugins/local-plugin/User.dll")).unwrap(),
            b"MZuser"
        );
        let before = fs::read(fixture.root.join(fixture.loader.receipt)).unwrap();
        fixture
            .install(&[fixture.source(vec![fixture.dependency()])], None)
            .unwrap();
        assert_eq!(
            fs::read(fixture.root.join(fixture.loader.receipt)).unwrap(),
            before
        );
    }
}

#[test]
fn bepinex_doorstop_rejects_unknown_target_ambiguous_enablement_and_il2cpp_escape() {
    for module_id in ["valheim", "vrising"] {
        let fixture = Fixture::new(module_id);
        for text in [
            "[General]\nenabled=false\ntarget_assembly=../outside.dll\n",
            "[General]\nenabled=true\nenabled=false\ntarget_assembly=BepInEx/core/BepInEx.Preloader.dll\n",
            "[General]\nenabled=yes\ntarget_assembly=BepInEx/core/BepInEx.Preloader.dll\n",
        ] {
            assert!(enable_doorstop(text.as_bytes(), &fixture.loader).is_err());
        }
        assert_eq!(
            enable_doorstop(fixture.config, &fixture.loader).unwrap(),
            (fixture.config.to_vec(), false)
        );
    }
    let fixture = Fixture::new("vrising");
    for (from, to) in [
        ("dotnet\\coreclr.dll", "dotnet\\..\\outside.dll"),
        ("corlib_dir=dotnet", "corlib_dir=C:\\outside"),
        ("corlib_dir=dotnet", "corlib_dir=dotnet\ncorlib_dir=dotnet"),
    ] {
        let config = String::from_utf8(fixture.config.to_vec())
            .unwrap()
            .replace(from, to);
        assert!(enable_doorstop(config.as_bytes(), &fixture.loader).is_err());
    }
    let mono = Fixture::new("valheim");
    assert!(enable_doorstop(MONO_CONFIG, &fixture.loader).is_err());
    assert!(enable_doorstop(IL2CPP_CONFIG, &mono.loader).is_err());
}

#[test]
fn bepinex_receipt_does_not_trust_self_declared_or_modified_runtime_hashes() {
    for module_id in ["valheim", "vrising"] {
        let fixture = Fixture::new(module_id);
        fixture.install(&[], Some(&fixture.archive)).unwrap();
        fs::write(fixture.root.join("winhttp.dll"), b"MZchanged").unwrap();
        assert!(
            fixture
                .install(&[fixture.source(Vec::new())], None)
                .unwrap_err()
                .contains("conflicts")
        );
        assert_eq!(
            fs::read(fixture.root.join("winhttp.dll")).unwrap(),
            b"MZchanged"
        );
        let path = fixture.root.join(fixture.loader.receipt);
        let mut receipt: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        receipt["files"]["winhttp.dll"] = Sha256::digest(b"MZchanged")
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
            .into();
        fs::write(&path, serde_json::to_vec(&receipt).unwrap()).unwrap();
        assert!(
            fixture
                .install(&[], Some(&fixture.archive))
                .unwrap_err()
                .contains("receipt does not match")
        );
    }
}

#[cfg(windows)]
#[test]
fn bepinex_root_transaction_rolls_back_loader_when_plugin_publication_fails() {
    use std::os::windows::fs::OpenOptionsExt;
    for module_id in ["valheim", "vrising"] {
        let fixture = Fixture::new(module_id);
        let target = fixture.target().target_path;
        fs::create_dir_all(&target).unwrap();
        let manifest = target.join(ONLINE_MOD_MANIFEST);
        fs::write(&manifest, br#"{"packages":{}}"#).unwrap();
        let pin = OpenOptions::new()
            .read(true)
            .share_mode(0x1)
            .open(&manifest)
            .unwrap();
        let error = fixture
            .install(&[fixture.source(Vec::new())], Some(&fixture.archive))
            .unwrap_err();
        drop(pin);
        assert!(error.contains("rolled back"), "{error}");
        assert_eq!(fs::read(&manifest).unwrap(), br#"{"packages":{}}"#);
        for relative in [
            "winhttp.dll",
            fixture.loader.receipt,
            "BepInEx/core",
            "dotnet",
            "BepInEx/plugins/thunderstore-Author-Plugin",
        ] {
            assert!(
                !fixture.root.join(relative).exists(),
                "partial publication survived: {relative}"
            );
        }
        staging_tests::assert_no_staging_transaction(&fixture.directory);
    }
}

#[test]
fn vrising_loader_supports_large_native_runtime_files_and_rejects_wrong_game_targets() {
    let fixture = Fixture::with_extra("vrising", &[], true);
    fixture.install(&[], Some(&fixture.archive)).unwrap();
    assert_eq!(
        fs::metadata(fixture.root.join("dotnet/coreclr.dll"))
            .unwrap()
            .len(),
        23 * 1024 * 1024
    );
    assert!(verified_receipt(&fixture.root, &fixture.loader).unwrap());
    let mut target = fixture.target();
    target.target_path = fixture.root.join("wrong/plugins");
    assert!(stage_with_profile(target, &[], None, fixture.game, &fixture.loader).is_err());
    let mut target = fixture.target();
    target.module_id = "valheim".into();
    assert!(stage_with_profile(target, &[], None, fixture.game, &fixture.loader).is_err());
}

#[test]
fn reviewed_loader_profiles_keep_game_bindings_and_version_evidence_separate() {
    for module_id in ["valheim", "vrising"] {
        let game = thunderstore_game_profile(module_id).unwrap();
        let loader = game.loader.unwrap();
        assert!(loader.files.contains_key(loader.expected_assembly));
        let identity = loader.identity();
        assert!(reviewed_loader_satisfies(
            &identity,
            &loader.project,
            &loader.version
        ));
        let mut forged = identity.clone();
        forged.version = "0.0.0".into();
        assert!(!reviewed_loader_satisfies(
            &forged,
            &loader.project,
            &loader.version
        ));
        let other = thunderstore_game_profile(if module_id == "valheim" {
            "vrising"
        } else {
            "valheim"
        })
        .unwrap()
        .loader
        .unwrap();
        assert!(!reviewed_loader_satisfies(
            &identity,
            &other.project,
            &other.version
        ));
    }
    let vrising = thunderstore_game_profile("vrising")
        .unwrap()
        .loader
        .unwrap();
    assert!(vrising.supports_dependency(&vrising.project, "1.733.1"));
    assert!(!vrising.supports_dependency(&vrising.project, "1.732.9"));
    assert!(vrising.files.contains_key("dotnet/coreclr.dll"));
    assert!(
        thunderstore_game_profile("corekeeper")
            .unwrap()
            .loader
            .is_none()
    );
    assert!(thunderstore_game_profile("unknown-game").is_err());
}

#[test]
fn corekeeper_uses_native_mod_staging_without_bepinex() {
    let directory = staging_tests::staging_test_root("corekeeper-native-mods");
    let root = directory.join("runtime");
    let game = thunderstore_game_profile("corekeeper").unwrap();
    fs::create_dir_all(root.join("CoreKeeperServer_Data/StreamingAssets")).unwrap();
    fs::write(root.join(game.executable), b"MZnative-server").unwrap();
    let source = directory.join("native-plugin");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("NativeMod.dll"), b"MZnative-plugin").unwrap();
    fs::write(
        source.join("ModManifest.json"),
        br#"{"name":"NativeMod","version":"1.0.0"}"#,
    )
    .unwrap();
    let target = ResolvedManualModTarget {
        instance_id: "corekeeper-instance".into(),
        module_id: "corekeeper".into(),
        source_label: "Thunderstore".into(),
        target_label: game.target_relative.into(),
        target_path: root.join(game.target_relative),
        accepts: vec!["folder".into()],
        id_strategy: None,
    };
    let source = DownloadedManualModSource {
        path: source,
        label: "Author-NativeMod".into(),
        identity: OnlineModIdentity {
            thunderstore_intent: None,
            provider: "thunderstore".into(),
            project: "Author-NativeMod".into(),
            historical_names: vec![],
            version: "1.0.0".into(),
            dependencies: vec![],
        },
    };
    let result = stage_thunderstore_online_mod_sources(target, &[source], None);
    assert!(
        root.join(game.target_relative)
            .join("thunderstore-Author-NativeMod/NativeMod.dll")
            .is_file()
    );
    assert!(!root.join("BepInEx").exists());
    assert!(!root.join("winhttp.dll").exists());
    let _ = fs::remove_dir_all(&directory);
    result.unwrap();
}
