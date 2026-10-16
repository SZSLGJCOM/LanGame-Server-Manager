use super::*;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("langame-mod-runtime-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn write(&self, path: &str, bytes: &[u8]) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
}

#[test]
fn online_mod_corekeeper_requires_native_manifest_evidence_instead_of_thunderstore_metadata() {
    let fixture = Fixture::new();
    fixture.write("Plugin.dll", b"MZold-bepinex-plugin");
    fixture.write("manifest.json", br#"{"name":"ThunderstoreMetadata"}"#);
    assert!(validate_corekeeper_native_payload(&fixture.0).is_err());
    for invalid in [b"{}".as_slice(), b"[]", b"not-json"] {
        fixture.write("ModManifest.json", invalid);
        assert!(validate_corekeeper_native_payload(&fixture.0).is_err());
    }
    fixture.write(
        "ModManifest.json",
        br#"{"name":"NativePlugin","version":"1.0.0"}"#,
    );
    validate_corekeeper_native_payload(&fixture.0).unwrap();
}

#[tokio::test]
async fn online_mod_corekeeper_native_layout_uses_its_unique_manifest_root_and_cleans_rejected_payload()
 {
    let fixture = Fixture::new();
    let package = format!("FixtureNative{}", uuid::Uuid::new_v4().simple());
    for valid in [true, false] {
        let archive_path = fixture
            .0
            .join(if valid { "valid.zip" } else { "invalid.zip" });
        let mut zip = zip::ZipWriter::new(fs::File::create(&archive_path).unwrap());
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (path, bytes) in [
            (
                "manifest.json",
                b"{\"name\":\"distribution metadata\"}".as_slice(),
            ),
            (
                "Native/ModManifest.json",
                if valid {
                    b"{\"name\":\"Native\"}".as_slice()
                } else {
                    b"[]"
                },
            ),
            ("Native/Plugin.dll", b"MZplugin"),
        ] {
            zip.start_file(path, options).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
        let prepared = prepare_downloaded_thunderstore_archive(
            archive_path.clone(),
            &package,
            "1.0.0",
            "corekeeper",
        )
        .await;
        if valid {
            let prepared = prepared.unwrap();
            assert!(prepared.join("ModManifest.json").is_file());
            assert!(prepared.join("Plugin.dll").is_file());
            assert!(!prepared.join("Native").exists());
            assert!(!prepared.join("manifest.json").exists());
            cleanup_downloaded_mod_source(&prepared);
        } else {
            assert!(prepared.is_err());
        }
        assert!(!archive_path.exists());
        let prefix = format!("langame-mod-{package}-1.0.0-");
        assert!(!fs::read_dir(std::env::temp_dir()).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(&prefix)
        }));
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn online_mod_loader_inspection_preflights_zip_directory_before_parser_allocation() {
    let fixture = Fixture::new();
    let mut eocd = [0u8; 22];
    eocd[..4].copy_from_slice(b"PK\x05\x06");
    eocd[12..16].copy_from_slice(&(64u32 * 1024 * 1024 + 1).to_le_bytes());
    fixture.write("oversize-directory.zip", &eocd);
    let error = reject_loader_archive(&fixture.0.join("oversize-directory.zip")).unwrap_err();
    assert!(error.contains("central directory"), "{error}");
}

#[test]
fn online_mod_runtime_requires_this_instances_enabled_loader() {
    let fixture = Fixture::new();
    let target = fixture.0.join("BepInEx/plugins");
    assert!(validate_runtime("valheim", "thunderstore", &fixture.0, &target).is_err());
    fixture.write("winhttp.dll", b"MZproxy");
    fixture.write("BepInEx/core/BepInEx.Preloader.dll", b"MZloader");
    fixture.write(
        "doorstop_config.ini",
        b"[UnityDoorstop]\nenabled=true\ntargetAssembly=BepInEx\\core\\BepInEx.Preloader.dll\n",
    );
    validate_runtime("valheim", "thunderstore", &fixture.0, &target).unwrap();
    assert!(
        validate_runtime(
            "valheim",
            "thunderstore",
            &fixture.0,
            &fixture.0.join("another/plugins")
        )
        .is_err()
    );
    fixture.write(
        "doorstop_config.ini",
        b"[UnityDoorstop]\nenabled=false\ntargetAssembly=BepInEx/core/BepInEx.Preloader.dll\n",
    );
    assert!(
        validate_runtime("valheim", "thunderstore", &fixture.0, &target)
            .unwrap_err()
            .contains("disabled")
    );
}

#[test]
fn online_mod_runtime_preserves_native_corekeeper_and_rejects_unverified_minecraft() {
    let fixture = Fixture::new();
    fixture.write("CoreKeeperServer.exe", b"MZserver");
    let target = fixture.0.join("CoreKeeperServer_Data/StreamingAssets/Mods");
    fs::create_dir_all(&target).unwrap();
    validate_runtime("corekeeper", "thunderstore", &fixture.0, &target).unwrap();
    fixture.write("server.jar", b"vanilla jar");
    let error =
        validate_runtime("minecraft", "modrinth", &fixture.0, &fixture.0.join("mods")).unwrap_err();
    let error: Value = serde_json::from_str(&error).unwrap();
    assert_eq!(error["code"], "mod_runtime_unverified");
    assert_eq!(error["reason"], "minecraft_loader");
}

#[test]
fn online_mod_doorstop_cannot_escape_its_runtime_or_ignore_ambiguous_settings() {
    for configuration in [
        "[General]\nenabled=true\ntarget_assembly=../outside.dll",
        "[General]\nenabled=true\ntarget_assembly=BepInEx/core/../../outside.dll",
        "[General]\nenabled=true\nenabled=false\ntarget_assembly=BepInEx/core/loader.dll",
        "[General]\nenabled=true\ntarget_assembly=C:/outside.dll",
    ] {
        assert!(doorstop_assembly(configuration).is_err());
    }
    assert_eq!(
        doorstop_assembly(
            "[General]\nenabled=true\ntarget_assembly=BepInEx/core/BepInEx.Unity.IL2CPP.dll"
        )
        .unwrap(),
        PathBuf::from("BepInEx/core/BepInEx.Unity.IL2CPP.dll")
    );
}

#[test]
fn online_mod_metadata_requires_dependency_evidence_and_matching_community() {
    let metadata = serde_json::json!({
        "full_name": "Author-Plugin", "community_listings": [{"community":"valheim"}],
        "latest": {"version_number":"1.2.3", "download_url":"https://thunderstore.io/package/download/Author/Plugin/1.2.3/", "dependencies":[]}
    });
    let parsed: ThunderstorePackageMetadata = serde_json::from_value(metadata.clone()).unwrap();
    validate_thunderstore_metadata(&parsed, "valheim", "Author-Plugin").unwrap();
    assert!(validate_thunderstore_metadata(&parsed, "v-rising", "Author-Plugin").is_err());
    let mut incomplete = metadata;
    incomplete["latest"]
        .as_object_mut()
        .unwrap()
        .remove("dependencies");
    assert!(serde_json::from_value::<ThunderstorePackageMetadata>(incomplete).is_err());
}

#[test]
fn online_mod_loader_archives_are_rejected_before_plugin_extraction() {
    let fixture = Fixture::new();
    for (name, allowed) in [
        ("plugins/plugin.dll", true),
        ("plugins/插件.dll", true),
        ("BepInExPack/BepInEx/core/BepInEx.dll", false),
    ] {
        let path = fixture
            .0
            .join(if allowed { "plugin.zip" } else { "loader.zip" });
        let file = fs::File::create(&path).unwrap();
        let mut archive = zip::ZipWriter::new(file);
        archive
            .start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(b"MZpayload").unwrap();
        archive.finish().unwrap();
        assert_eq!(reject_loader_archive(&path).is_ok(), allowed);
    }
}

#[test]
fn online_mod_archive_honors_declared_filename_encoding() {
    let fixture = Fixture::new();
    let path = fixture.0.join("invalid-name.zip");
    let file = fs::File::create(&path).unwrap();
    let mut archive = zip::ZipWriter::new(file);
    archive
        .start_file("plugin.dll", zip::write::SimpleFileOptions::default())
        .unwrap();
    archive.write_all(b"MZpayload").unwrap();
    archive.finish().unwrap();
    let original = fs::read(&path).unwrap();
    for declares_utf8 in [false, true] {
        let mut bytes = original.clone();
        // The same byte is valid CP437, but invalid when both headers declare UTF-8.
        for (signature, flags_offset, name_offset) in
            [(b"PK\x03\x04", 6, 30), (b"PK\x01\x02", 8, 46)]
        {
            let offset = bytes
                .windows(4)
                .position(|value| value == signature)
                .unwrap();
            if declares_utf8 {
                bytes[offset + flags_offset + 1] |= 0x08;
            }
            bytes[offset + name_offset] = 0xff;
        }
        fs::write(&path, bytes).unwrap();
        assert_eq!(reject_loader_archive(&path).is_err(), declares_utf8);
    }
}
