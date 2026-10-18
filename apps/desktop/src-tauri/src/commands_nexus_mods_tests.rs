use super::*;
use std::io::Write;

struct StagingFixture(PathBuf);

impl StagingFixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "langame-nexus-stage-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }

    fn target(&self, module: &str) -> ResolvedManualModTarget {
        let (relative, accepts): (&str, &[&str]) = match module {
            "abioticfactor" => (
                "AbioticFactor/Binaries/Win64/ue4ss/Mods",
                &["folder", "zip", "lua", "pak"],
            ),
            "humanitz" => (
                "HumanitZServer/Content/Paks/~mods",
                &["pak", "zip", "folder"],
            ),
            "enshrouded" => ("mods", &["folder", "zip", "dll"]),
            "sevendaystodie" => ("Mods", &["folder", "zip"]),
            _ => panic!("undeclared Nexus staging fixture"),
        };
        ResolvedManualModTarget {
            instance_id: "synthetic-nexus-instance".into(),
            module_id: module.into(),
            source_label: "Nexus Mods".into(),
            target_label: relative.into(),
            target_path: self.0.join("install").join(relative),
            accepts: accepts.iter().map(|value| (*value).into()).collect(),
            id_strategy: None,
        }
    }

    fn zip(&self, name: &str, entries: &[(&str, &[u8])]) -> PathBuf {
        let path = self.0.join(name);
        let mut archive = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        for (name, contents) in entries {
            archive
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            archive.write_all(contents).unwrap();
        }
        archive.finish().unwrap();
        path
    }

    fn stage(
        &self,
        target: ResolvedManualModTarget,
        paths: Vec<PathBuf>,
    ) -> Result<ManualModStageResult, String> {
        super::super::commands_mod_staging::stage_manual_mod_sources(target, paths)
    }
}

impl Drop for StagingFixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn nexus_credentials_are_local_runtime_commands_with_owned_installation() {
    use crate::command_registry::{Completion, Execution, LanAccess, find};
    for name in [
        "nexus_account_status",
        "nexus_store_api_key",
        "nexus_clear_api_key",
        "review_nexus_mod_references",
        "install_nexus_mod_files",
    ] {
        let command = find(name).expect("Nexus command must be registered");
        assert_eq!(command.execution, Execution::Runtime, "{name}");
        assert_eq!(command.lan, LanAccess::Denied, "{name}");
        assert_eq!(
            command.completion,
            if name == "install_nexus_mod_files" {
                Completion::Owned
            } else {
                Completion::Bounded
            },
            "{name}"
        );
    }
}

#[test]
fn api_key_header_validation_rejects_controls_unicode_and_oversized_input_without_echo() {
    let synthetic = "synthetic-only-api-key";
    assert_eq!(
        normalize_api_key(&format!("  {synthetic}\r\n")).unwrap(),
        synthetic
    );
    for invalid in [
        String::new(),
        " ".into(),
        format!("{synthetic}\r\nInjected: header"),
        format!("{synthetic}\0suffix"),
        format!("{synthetic} internal-space"),
        format!("{synthetic}\u{a0}suffix"),
        synthetic.repeat(300),
    ] {
        let error = normalize_api_key(&invalid).unwrap_err();
        assert!(!error.contains(synthetic));
        assert!(!error.contains("Injected"));
        assert!(!error.contains('\r') && !error.contains('\n') && !error.contains('\0'));
    }
    let error: Value = serde_json::from_str(&auth_required()).unwrap();
    assert_eq!(error["code"], "nexus_auth_required");
    assert_eq!(error.as_object().unwrap().len(), 2);
}

#[test]
fn nexus_zip_downloads_use_the_existing_game_target_layout_and_local_ownership() {
    for (module, relative, contents) in [
        (
            "sevendaystodie",
            "FixtureMod/ModInfo.xml",
            b"<xml><ModInfo/></xml>".as_slice(),
        ),
        (
            "abioticfactor",
            "FixtureMod/Scripts/main.lua",
            b"print('synthetic fixture')".as_slice(),
        ),
    ] {
        let fixture = StagingFixture::new();
        let target = fixture.target(module);
        let destination = target.target_path.join(relative);
        let ownership = target
            .target_path
            .join(super::super::commands_mod_staging::ONLINE_MOD_MANIFEST);
        let archive = fixture.zip("Official-name.zip", &[(relative, contents)]);
        let result = fixture.stage(target, vec![archive]).unwrap();
        assert_eq!(result.copied_file_count, 1);
        assert_eq!(fs::read(destination).unwrap(), contents);
        assert!(ownership.is_file());
    }
}

#[test]
fn nexus_raw_pak_and_dll_keep_their_official_filename_in_the_declared_target() {
    for (module, name) in [
        ("humanitz", "FixtureMod.pak"),
        ("enshrouded", "FixtureMod.dll"),
    ] {
        let fixture = StagingFixture::new();
        let target = fixture.target(module);
        let destination = target.target_path.join(name);
        let source = fixture.0.join(name);
        fs::write(&source, b"synthetic package bytes").unwrap();
        let result = fixture.stage(target, vec![source]).unwrap();
        assert_eq!(result.copied_file_count, 1);
        assert_eq!(fs::read(destination).unwrap(), b"synthetic package bytes");
    }
}

#[test]
fn nexus_staging_refuses_archive_traversal_and_unsupported_formats_without_publication() {
    let fixture = StagingFixture::new();
    let target = fixture.target("enshrouded");
    let destination = target.target_path.clone();
    let archive = fixture.zip("Traversal.zip", &[("../outside.dll", b"unsafe")]);
    assert!(fixture.stage(target, vec![archive]).is_err());
    assert!(!destination.exists());
    assert!(!fixture.0.join("outside.dll").exists());
    let source = fixture.0.join("Unsupported.7z");
    fs::write(&source, b"unsupported archive fixture").unwrap();
    assert!(
        fixture
            .stage(fixture.target("enshrouded"), vec![source])
            .is_err()
    );
    assert!(!destination.exists());
}

#[cfg(windows)]
#[test]
fn nexus_staging_rolls_back_published_files_when_an_owned_destination_is_locked() {
    use std::os::windows::fs::OpenOptionsExt;
    let fixture = StagingFixture::new();
    let target = fixture.target("enshrouded");
    let destination = target.target_path.clone();
    let initial = fixture.zip("Initial.zip", &[("FixtureMod/z.dll", b"old package")]);
    fixture.stage(target, vec![initial]).unwrap();
    let manifest = destination.join(super::super::commands_mod_staging::ONLINE_MOD_MANIFEST);
    let before_manifest = fs::read(&manifest).unwrap();
    fs::write(
        destination.join("FixtureMod/operator.cfg"),
        b"operator settings",
    )
    .unwrap();
    // Fingerprinting can read the old payload, but publication cannot move it.
    // The alphabetically earlier ownership record and a.dll must roll back.
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(destination.join("FixtureMod/z.dll"))
        .unwrap();
    let replacement = fixture.zip(
        "Replacement.zip",
        &[
            ("FixtureMod/a.dll", b"new file"),
            ("FixtureMod/z.dll", b"new package"),
        ],
    );
    let error = fixture
        .stage(fixture.target("enshrouded"), vec![replacement])
        .unwrap_err();
    drop(held);
    assert!(error.contains("rolled back"), "{error}");
    assert_eq!(
        fs::read(destination.join("FixtureMod/z.dll")).unwrap(),
        b"old package"
    );
    assert!(!destination.join("FixtureMod/a.dll").exists());
    assert_eq!(fs::read(manifest).unwrap(), before_manifest);
    assert_eq!(
        fs::read(destination.join("FixtureMod/operator.cfg")).unwrap(),
        b"operator settings"
    );
}
