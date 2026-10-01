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
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
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
