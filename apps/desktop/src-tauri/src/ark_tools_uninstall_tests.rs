use super::super::tests::{Fixture, hashes, payload};
use super::*;

fn installed(fixture: &Fixture) -> BTreeMap<String, Vec<u8>> {
    let mut content = payload(b"installed plugin");
    content.insert(NOTICES.into(), b"installed notices".to_vec());
    publish(&fixture.0, &ASE, content.clone(), &hashes(&content)).unwrap();
    content
}

fn assert_content(fixture: &Fixture, content: &BTreeMap<String, Vec<u8>>) {
    for (path, bytes) in content {
        assert_eq!(
            fs::read(fixture.bin().join(path)).unwrap(),
            *bytes,
            "{path}"
        );
    }
}

fn embedded() -> EmbeddedArkTools {
    static IMAGE: [u8; 70] = {
        let mut image = [0; 70];
        image[0] = b'M';
        image[1] = b'Z';
        image[60] = 64;
        image[64] = b'P';
        image[65] = b'E';
        image[68] = 0x64;
        image[69] = 0x86;
        image
    };
    EmbeddedArkTools {
        plugin: &IMAGE,
        proxy: Some(&IMAGE),
    }
}

#[test]
fn ark_tools_uninstall_removes_only_owned_files_and_allows_reinstallation() {
    let fixture = Fixture::new();
    let content = installed(&fixture);
    let wanted = hashes(&content);
    let owner = fs::read(fixture.bin().join(OWNER)).unwrap();
    let retained = [
        fixture.bin().join("ArkApi/cache.bin"),
        fixture
            .bin()
            .join("ArkApi/Plugins/LgsmArkTools/operator.txt"),
        fixture.0.join("ShooterGame/Saved/world.ark"),
    ];
    for path in &retained {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"user data").unwrap();
    }
    assert!(!remove(&fixture.0, &ASE, &wanted).unwrap());
    for path in wanted.keys() {
        assert!(!fixture.bin().join(path).exists(), "{path}");
    }
    for path in &retained {
        assert_eq!(fs::read(path).unwrap(), b"user data");
    }
    assert_eq!(fs::read(fixture.bin().join(OWNER)).unwrap(), owner);
    let owned = files::ownership(&fixture.bin(), &ASE, &wanted)
        .unwrap()
        .unwrap();
    assert!(!available(&fixture.bin(), &owned).unwrap());
    assert!(!pending(&fixture.bin()).unwrap());
    ensure_can_start(&fixture.0).unwrap();
    publish(&fixture.0, &ASE, content.clone(), &wanted).unwrap();
    assert_content(&fixture, &content);
    assert!(available(&fixture.bin(), &owned).unwrap());
    fixture.finish();
}

#[test]
fn ark_tools_uninstall_retains_shared_framework_for_other_plugins() {
    let fixture = Fixture::new();
    let content = installed(&fixture);
    let other = fixture
        .bin()
        .join("ArkApi/Plugins/Permissions/Permissions.dll");
    fs::create_dir_all(other.parent().unwrap()).unwrap();
    fs::write(&other, b"other plugin").unwrap();
    fs::write(fixture.bin().join("config.json"), b"operator settings").unwrap();
    assert!(remove(&fixture.0, &ASE, &hashes(&content)).unwrap());
    assert_eq!(fs::read(other).unwrap(), b"other plugin");
    assert_eq!(
        fs::read(fixture.bin().join("config.json")).unwrap(),
        b"operator settings"
    );
    assert_eq!(
        fs::read(fixture.bin().join("version.dll")).unwrap(),
        content["version.dll"]
    );
    for path in [PLUGIN, INFO, NOTICES] {
        assert!(!fixture.bin().join(path).exists());
    }
    fixture.finish();
}

#[test]
fn ark_tools_uninstall_restores_every_file_when_another_plugin_appears_during_removal() {
    let fixture = Fixture::new();
    let content = installed(&fixture);
    let wanted = hashes(&content);
    let owner = fs::read(fixture.bin().join(OWNER)).unwrap();
    let other = fixture
        .bin()
        .join("ArkApi/Plugins/Permissions/Permissions.dll");
    let error = remove_with(&fixture.0, &ASE, &wanted, |index| {
        if index == 0 {
            fs::create_dir_all(other.parent().unwrap()).unwrap();
            fs::write(&other, b"new third-party plugin").unwrap();
        }
        if index == wanted.len() - 1 {
            for path in wanted.keys() {
                assert!(!fixture.bin().join(path).exists());
            }
        }
        Ok(())
    })
    .unwrap_err();
    assert!(error.contains("restoring the shared framework"));
    assert_content(&fixture, &content);
    assert_eq!(fs::read(fixture.bin().join(OWNER)).unwrap(), owner);
    assert_eq!(fs::read(&other).unwrap(), b"new third-party plugin");
    assert!(!pending(&fixture.bin()).unwrap());
    ensure_can_start(&fixture.0).unwrap();
    assert!(remove(&fixture.0, &ASE, &wanted).unwrap());
    for path in ["config.json", "version.dll"] {
        assert_eq!(fs::read(fixture.bin().join(path)).unwrap(), content[path]);
    }
    assert_eq!(fs::read(other).unwrap(), b"new third-party plugin");
    fixture.finish();
}

#[test]
fn ark_tools_uninstall_accepts_previous_plugin_hash_and_missing_managed_files() {
    let fixture = Fixture::new();
    let content = installed(&fixture);
    let mut next = content.clone();
    next.insert(PLUGIN.into(), b"new embedded plugin".to_vec());
    next.insert(INFO.into(), b"new embedded metadata".to_vec());
    fs::remove_file(fixture.bin().join(NOTICES)).unwrap();
    fs::remove_file(fixture.bin().join("config.json")).unwrap();
    assert!(!remove(&fixture.0, &ASE, &hashes(&next)).unwrap());
    for path in content.keys() {
        assert!(!fixture.bin().join(path).exists());
    }
    fixture.finish();
}

#[test]
fn ark_tools_uninstall_status_offers_removal_for_an_outdated_owned_plugin() {
    let fixture = Fixture::new();
    let mut wanted = expected(&ASE, embedded()).unwrap();
    wanted.insert(PLUGIN.into(), digest(b"old plugin"));
    fs::create_dir_all(fixture.bin().join("ArkApi/Plugins/LgsmArkTools")).unwrap();
    fs::write(fixture.bin().join(PLUGIN), b"old plugin").unwrap();
    fs::write(
        fixture.bin().join(OWNER),
        serde_json::to_vec(&Ownership {
            schema: 1,
            module_id: ASE.module_id.into(),
            files: wanted,
        })
        .unwrap(),
    )
    .unwrap();
    let status = inspect(&fixture.0, ASE.module_id, embedded()).unwrap();
    assert!(!status.ready);
    assert!(status.can_uninstall);
    assert!(!uninstall(&fixture.0, ASE.module_id, embedded()).unwrap());
    let status = inspect(&fixture.0, ASE.module_id, embedded()).unwrap();
    assert!(!status.ready);
    assert!(!status.can_uninstall);
    assert_eq!(
        status.issue.as_deref(),
        Some("Prepare the ARK server extension while this instance is stopped")
    );
    fixture.finish();
}

#[test]
fn ark_tools_uninstall_removes_a_previous_asa_proxy_without_touching_native_dependencies() {
    let fixture = Fixture::new();
    fs::write(fixture.bin().join(ASA.executable), b"ASA executable").unwrap();
    fs::write(fixture.bin().join("msvcp140.dll"), b"native dependency").unwrap();
    let mut wanted = expected(&ASA, embedded()).unwrap();
    fs::create_dir_all(fixture.bin().join("ArkApi/Plugins/LgsmArkTools")).unwrap();
    for (path, bytes) in [
        (PLUGIN, b"old plugin".as_slice()),
        ("version.dll", b"old ASA proxy".as_slice()),
    ] {
        fs::write(fixture.bin().join(path), bytes).unwrap();
        wanted.insert(path.into(), digest(bytes));
    }
    fs::write(
        fixture.bin().join(OWNER),
        serde_json::to_vec(&Ownership {
            schema: 1,
            module_id: ASA.module_id.into(),
            files: wanted,
        })
        .unwrap(),
    )
    .unwrap();
    assert!(!uninstall(&fixture.0, ASA.module_id, embedded()).unwrap());
    assert!(!fixture.bin().join(PLUGIN).exists());
    assert!(!fixture.bin().join("version.dll").exists());
    assert_eq!(
        fs::read(fixture.bin().join("msvcp140.dll")).unwrap(),
        b"native dependency"
    );
    fixture.finish();
}

#[test]
fn ark_tools_uninstall_rejects_modified_files_before_deleting_anything() {
    for changed in [PLUGIN, INFO, NOTICES, "config.json", "version.dll"] {
        let fixture = Fixture::new();
        let mut content = installed(&fixture);
        let wanted = hashes(&content);
        fs::write(fixture.bin().join(changed), b"operator changes").unwrap();
        content.insert(changed.into(), b"operator changes".to_vec());
        assert!(
            remove(&fixture.0, &ASE, &wanted)
                .unwrap_err()
                .contains("modified")
        );
        assert_content(&fixture, &content);
        assert!(!pending(&fixture.bin()).unwrap());
        fixture.finish();
    }
}

#[test]
fn ark_tools_uninstall_restores_every_file_after_a_deletion_failure() {
    let fixture = Fixture::new();
    let content = installed(&fixture);
    let wanted = hashes(&content);
    let owner = fs::read(fixture.bin().join(OWNER)).unwrap();
    let error = remove_with(&fixture.0, &ASE, &wanted, |index| {
        if index == 1 {
            Err("injected deletion failure".into())
        } else {
            Ok(())
        }
    })
    .unwrap_err();
    assert_eq!(error, "injected deletion failure");
    assert_content(&fixture, &content);
    assert_eq!(fs::read(fixture.bin().join(OWNER)).unwrap(), owner);
    assert!(!pending(&fixture.bin()).unwrap());
    ensure_can_start(&fixture.0).unwrap();
    remove(&fixture.0, &ASE, &wanted).unwrap();
    fixture.finish();
}

#[test]
fn ark_tools_uninstall_preserves_a_concurrent_replacement_when_rollback_is_required() {
    let fixture = Fixture::new();
    let content = installed(&fixture);
    let wanted = hashes(&content);
    let error = remove_with(&fixture.0, &ASE, &wanted, |index| {
        if index == 0 {
            fs::write(fixture.bin().join(PLUGIN), b"external replacement").unwrap();
            return Err("injected deletion failure".into());
        }
        Ok(())
    })
    .unwrap_err();
    assert!(error.contains("retained for recovery"));
    assert_eq!(
        fs::read(fixture.bin().join(PLUGIN)).unwrap(),
        b"external replacement"
    );
    assert!(pending(&fixture.bin()).unwrap());
    assert!(ensure_can_start(&fixture.0).is_err());
    fixture.finish();
}

#[test]
fn ark_tools_uninstall_rejects_unowned_manifests_and_relative_roots() {
    let fixture = Fixture::new();
    let content = installed(&fixture);
    let wanted = hashes(&content);
    assert!(remove(Path::new("relative-runtime"), &ASE, &wanted).is_err());
    let mut manifest: Ownership =
        serde_json::from_slice(&fs::read(fixture.bin().join(OWNER)).unwrap()).unwrap();
    manifest
        .files
        .insert("../operator.txt".into(), digest(b"operator data"));
    fs::write(
        fixture.bin().join(OWNER),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    assert!(remove(&fixture.0, &ASE, &wanted).is_err());
    assert_content(&fixture, &content);
    assert!(!pending(&fixture.bin()).unwrap());
    fixture.finish();
}

#[cfg(windows)]
#[test]
fn ark_tools_uninstall_rejects_junction_parents_without_deleting_target_files() {
    use std::os::windows::process::CommandExt;
    let fixture = Fixture::new();
    let content = installed(&fixture);
    let link = fixture.bin().join("ArkApi");
    let target = fixture.0.join("retained-plugin-files");
    fs::rename(&link, &target).unwrap();
    let result = std::process::Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(link.to_string_lossy().replace('/', "\\"))
        .arg(target.to_string_lossy().replace('/', "\\"))
        .creation_flags(0x0800_0000)
        .output()
        .unwrap();
    assert!(result.status.success(), "{result:?}");
    let result = remove(&fixture.0, &ASE, &hashes(&content));
    fs::remove_dir(&link).unwrap();
    assert!(result.is_err());
    assert_eq!(
        fs::read(target.join("Plugins/LgsmArkTools/LgsmArkTools.dll")).unwrap(),
        content[PLUGIN]
    );
    assert_eq!(
        fs::read(fixture.bin().join("version.dll")).unwrap(),
        content["version.dll"]
    );
    fixture.finish();
}

#[path = "ark_tools_uninstall_recovery_tests.rs"]
mod recovery_tests;
