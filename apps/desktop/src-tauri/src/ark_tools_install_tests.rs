use super::*;

pub(super) struct Fixture(pub(super) PathBuf, bool);

impl Fixture {
    pub(super) fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("lg-ark-install-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir_all(root.join("ShooterGame/Binaries/Win64")).unwrap();
        fs::write(
            root.join("ShooterGame/Binaries/Win64/ShooterGameServer.exe"),
            b"test executable boundary",
        )
        .unwrap();
        Self(root, false)
    }
    pub(super) fn finish(mut self) {
        assert_eq!(self.0.parent(), Some(std::env::temp_dir().as_path()));
        assert!(
            self.0
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("lg-ark-install-")
        );
        fs::remove_dir_all(&self.0).expect("remove only this test's exclusively created fixture");
        self.1 = true;
    }
    pub(super) fn bin(&self) -> PathBuf {
        self.0.join("ShooterGame/Binaries/Win64")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if !self.1 {
            eprintln!(
                "ARK installer fixture retained after failure: {}",
                self.0.display()
            );
        }
    }
}

pub(super) fn payload(plugin: &[u8]) -> BTreeMap<String, Vec<u8>> {
    BTreeMap::from([
        ("version.dll".into(), b"fixed verified loader".to_vec()),
        (
            "config.json".into(),
            b"fixed managed configuration".to_vec(),
        ),
        (PLUGIN.into(), plugin.to_vec()),
        (INFO.into(), b"plugin metadata".to_vec()),
    ])
}

pub(super) fn hashes(payload: &BTreeMap<String, Vec<u8>>) -> BTreeMap<String, String> {
    payload
        .iter()
        .map(|(p, b)| (p.clone(), digest(b)))
        .collect()
}

#[test]
fn ark_tools_install_publishes_owned_files_and_updates_only_owned_plugin() {
    let fixture = Fixture::new();
    let initial = payload(b"plugin release one");
    publish(&fixture.0, &ASE, initial.clone(), &hashes(&initial)).unwrap();
    publish(&fixture.0, &ASE, initial.clone(), &hashes(&initial)).unwrap();
    let next = payload(b"plugin release two");
    publish(&fixture.0, &ASE, next.clone(), &hashes(&next)).unwrap();
    let owned = files::ownership(&fixture.bin(), &ASE, &hashes(&next))
        .unwrap()
        .unwrap();
    assert_eq!(owned.files, hashes(&next));
    for (path, bytes) in next {
        assert_eq!(fs::read(fixture.bin().join(path)).unwrap(), bytes);
    }
    assert!(!fs::read_dir(fixture.bin()).unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".langame-ark-tools-")
    }));
    fixture.finish();
}

#[test]
fn ark_tools_install_preserves_unowned_loader_and_modified_configuration() {
    let fixture = Fixture::new();
    let initial = payload(b"owned plugin");
    fs::write(fixture.bin().join("version.dll"), b"user loader").unwrap();
    assert!(
        publish(&fixture.0, &ASE, initial.clone(), &hashes(&initial))
            .unwrap_err()
            .contains("unknown or modified")
    );
    assert_eq!(
        fs::read(fixture.bin().join("version.dll")).unwrap(),
        b"user loader"
    );
    fs::remove_file(fixture.bin().join("version.dll")).unwrap();
    publish(&fixture.0, &ASE, initial.clone(), &hashes(&initial)).unwrap();
    fs::write(fixture.bin().join("config.json"), b"operator settings").unwrap();
    assert!(
        publish(&fixture.0, &ASE, initial.clone(), &hashes(&initial))
            .unwrap_err()
            .contains("unknown or modified")
    );
    assert_eq!(
        fs::read(fixture.bin().join("config.json")).unwrap(),
        b"operator settings"
    );
    fixture.finish();
}

#[test]
fn ark_tools_install_rolls_back_partial_install_and_leaves_retry_possible() {
    let fixture = Fixture::new();
    let mut initial = payload(b"plugin");
    let wanted = hashes(&initial);
    let error = publish_with(&fixture.0, &ASE, &mut initial, &wanted, |index| {
        if index == 3 {
            Err("injected disk failure".into())
        } else {
            Ok(())
        }
    })
    .unwrap_err();
    assert_eq!(error, "injected disk failure");
    assert_eq!(
        fs::read_dir(fixture.bin()).unwrap().count(),
        1,
        "only original server executable survives rollback"
    );
    let initial = payload(b"plugin");
    publish(&fixture.0, &ASE, initial, &wanted).unwrap();
    fixture.finish();
}

#[test]
fn ark_tools_install_failed_upgrade_restores_previous_plugin_and_manifest() {
    let fixture = Fixture::new();
    let initial = payload(b"plugin one");
    publish(&fixture.0, &ASE, initial.clone(), &hashes(&initial)).unwrap();
    let manifest = fs::read(fixture.bin().join(OWNER)).unwrap();
    let mut next = payload(b"plugin two");
    let wanted = hashes(&next);
    assert!(
        publish_with(
            &fixture.0,
            &ASE,
            &mut next,
            &wanted,
            |index| if index == 1 {
                Err("publication failed".into())
            } else {
                Ok(())
            }
        )
        .is_err()
    );
    assert_eq!(fs::read(fixture.bin().join(OWNER)).unwrap(), manifest);
    assert_eq!(fs::read(fixture.bin().join(PLUGIN)).unwrap(), b"plugin one");
    fixture.finish();
}

#[test]
fn ark_tools_install_rollback_never_deletes_concurrent_replacement() {
    let fixture = Fixture::new();
    let path = fixture.bin().join("one.dll");
    let backup = fixture.bin().join("one.backup");
    fs::write(&path, b"external modification").unwrap();
    fs::write(&backup, b"original").unwrap();
    assert!(
        files::rollback_file(&path, &backup, Some(b"original"), b"our replacement")
            .unwrap_err()
            .contains("concurrently changed")
    );
    assert_eq!(fs::read(path).unwrap(), b"external modification");
    assert_eq!(fs::read(backup).unwrap(), b"original");
    fixture.finish();
}

fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn test_profile(bytes: &[u8]) -> Profile {
    Profile {
        archive_size: bytes.len(),
        archive_hash: Box::leak(digest(bytes).into_boxed_str()),
        payload: Box::leak(
            vec![(
                "version.dll",
                Box::leak(digest(b"framework").into_boxed_str()) as &str,
            )]
            .into_boxed_slice(),
        ),
        ..ASE
    }
}

#[test]
fn ark_tools_install_archive_is_checksum_bound_and_excludes_other_plugins() {
    let bytes = archive(&[
        ("version.dll", b"framework"),
        (
            "ArkApi/Plugins/Permissions/Permissions.dll",
            b"unrequested plugin",
        ),
    ]);
    let profile = test_profile(&bytes);
    assert_eq!(
        unpack(&profile, &bytes).unwrap(),
        BTreeMap::from([("version.dll".into(), b"framework".to_vec())])
    );
    let mut changed = bytes.clone();
    changed[0] ^= 1;
    assert!(unpack(&profile, &changed).unwrap_err().contains("checksum"));
}

#[test]
fn ark_tools_install_rejects_unsafe_even_ignored_archive_entries() {
    for name in [
        "../external",
        "C:/external",
        "ArkApi/CON.txt",
        "ArkApi/config.json:stream",
        "ArkApi/alias.",
    ] {
        let bytes = archive(&[("version.dll", b"framework"), (name, b"ignored")]);
        assert!(
            unpack(&test_profile(&bytes), &bytes)
                .unwrap_err()
                .contains("unsafe"),
            "{name}"
        );
    }
    let bytes = archive(&[("version.dll", b"framework"), ("VERSION.DLL", b"duplicate")]);
    assert!(
        unpack(&test_profile(&bytes), &bytes)
            .unwrap_err()
            .contains("duplicate")
    );
}

#[cfg(windows)]
#[test]
fn ark_tools_install_rejects_junction_parent_without_writing_its_target() {
    use std::os::windows::process::CommandExt;
    let fixture = Fixture::new();
    let target = fixture.0.join("external-fixture-target");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("sentinel"), b"preserved").unwrap();
    let link = fixture.bin().join("ArkApi");
    let result = std::process::Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(link.to_string_lossy().replace('/', "\\"))
        .arg(target.to_string_lossy().replace('/', "\\"))
        .creation_flags(0x0800_0000)
        .output()
        .unwrap();
    assert!(result.status.success(), "{result:?}");
    let initial = payload(b"plugin");
    let result = publish(&fixture.0, &ASE, initial.clone(), &hashes(&initial));
    fs::remove_dir(&link).unwrap();
    assert!(result.is_err());
    assert_eq!(fs::read_dir(&target).unwrap().count(), 1);
    assert_eq!(fs::read(target.join("sentinel")).unwrap(), b"preserved");
    fixture.finish();
}
