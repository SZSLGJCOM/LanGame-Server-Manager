use super::*;
use app_modules::ModuleStorageSpec;
use std::path::PathBuf;

struct TestRoot(PathBuf);

impl TestRoot {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "langame-install-data-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&path).expect("create isolated test root");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn descriptor(template: Option<&str>) -> ModuleDescriptor {
    ModuleDescriptor {
        root: PathBuf::new(),
        manifest_toml: String::new(),
        schema_json: None,
        default_ports: Vec::new(),
        install: None,
        process: None,
        workshop: None,
        runtime: app_core::ModuleRuntimeSpec::default(),
        storage: ModuleStorageSpec {
            saves_path_template: template.map(String::from),
            ..ModuleStorageSpec::default()
        },
        summary: app_core::ModuleSummary {
            id: String::from("astroneer"),
            name: String::from("ASTRONEER"),
            version: String::from("1"),
            description: None,
            steam_app_id: None,
            install_state: app_core::InstallState::Installed,
            instance_program_count: 0,
            archived_program_count: 0,
            supported_platforms: vec![String::from("windows")],
        },
    }
}

#[test]
fn missing_saves_do_not_protect_installer_configuration_and_logs() {
    let root = TestRoot::new();
    let saved = root.path().join("Astro/Saved");
    fs::create_dir_all(saved.join("Config")).unwrap();
    fs::create_dir_all(saved.join("Logs")).unwrap();
    fs::write(saved.join("Config/server.ini"), b"settings").unwrap();
    fs::write(saved.join("Logs/server.log"), b"log").unwrap();
    fs::write(saved.join("EXITREQUEST"), b"").unwrap();

    let protection = declared_install_root_data_path(
        &descriptor(Some("{{paths.install_root}}/Astro/Saved/SaveGames")),
        root.path(),
    )
    .unwrap();

    assert!(protection.is_none());
    assert!(!contains_preserved_data(&saved.join("SaveGames")).unwrap());
    assert!(saved.join("Config/server.ini").is_file());
}

#[test]
fn native_configuration_is_retained_without_instances_or_saves() {
    let root = TestRoot::new();
    let mut module = descriptor(None);
    module.storage.retained_paths = vec![
        "Saved/Config".into(),
        "Admins.txt".into(),
        "missing.json".into(),
    ];
    fs::create_dir_all(root.path().join("Saved/Config")).unwrap();
    fs::write(
        root.path().join("Saved/Config/Server.ini"),
        b"ServerGuid=existing",
    )
    .unwrap();
    fs::write(root.path().join("Admins.txt"), b"").unwrap();
    let protected = declared_install_root_retained_paths(&module, root.path()).unwrap();
    assert_eq!(protected.len(), 2);
    assert_eq!(
        protected[0].path.as_deref(),
        Some(root.path().join("Saved/Config").as_path())
    );
    assert_eq!(
        protected[1].path.as_deref(),
        Some(root.path().join("Admins.txt").as_path())
    );
}

#[test]
fn native_configuration_retention_rejects_paths_outside_installation() {
    let root = TestRoot::new();
    let mut module = descriptor(None);
    for relative in ["../outside", "C:/outside", "Config/../../outside", "."] {
        module.storage.retained_paths = vec![relative.into()];
        assert!(declared_install_root_retained_paths(&module, root.path()).is_err());
    }
}

#[test]
fn directories_without_files_are_not_preserved_data() {
    let root = TestRoot::new();
    fs::create_dir_all(root.path().join("SaveGames/empty/nested")).unwrap();
    fs::create_dir_all(root.path().join("SaveGames/another")).unwrap();

    assert!(!contains_preserved_data(&root.path().join("SaveGames")).unwrap());
    assert!(
        declared_install_root_data_path(
            &descriptor(Some("{{paths.install_root}}/SaveGames")),
            root.path()
        )
        .unwrap()
        .is_none()
    );
}

#[test]
fn orphaned_save_files_under_a_dynamic_prefix_are_protected_without_instances() {
    let root = TestRoot::new();
    let saves = root.path().join("server");
    fs::create_dir_all(saves.join("deleted-instance")).unwrap();
    let sentinel = saves.join("deleted-instance/world.save");
    fs::write(&sentinel, b"retained world").unwrap();

    let protection = declared_install_root_data_path(
        &descriptor(Some("{{paths.install_root}}/server/{{instance.id}}")),
        root.path(),
    )
    .unwrap()
    .expect("orphan world is still retained data");
    assert_eq!(protection.path.as_deref(), Some(saves.as_path()));
    assert_eq!(fs::read(sentinel).unwrap(), b"retained world");
}

#[test]
fn settings_leaf_uses_its_static_parent_with_windows_separator_support() {
    let root = TestRoot::new();
    let worlds = root.path().join("Saved/Worlds");
    fs::create_dir_all(worlds.join("OldWorld")).unwrap();
    fs::write(worlds.join("OldWorld/world.save"), b"world").unwrap();
    let protection = declared_install_root_data_path(
        &descriptor(Some(
            "{{ paths.install_root }}\\Saved\\Worlds\\{{settings.world_save_name}}",
        )),
        root.path(),
    )
    .unwrap()
    .expect("settings-dependent orphan must remain protected");
    assert_eq!(protection.path.as_deref(), Some(worlds.as_path()));
}

#[test]
fn a_file_at_the_save_path_is_preserved_even_when_empty() {
    let root = TestRoot::new();
    let save = root.path().join("world.save");
    fs::write(&save, b"").unwrap();
    assert!(contains_preserved_data(&save).unwrap());
    assert!(
        declared_install_root_data_path(
            &descriptor(Some("{{paths.install_root}}/world.save")),
            root.path()
        )
        .unwrap()
        .is_some()
    );
}

#[test]
fn root_wide_and_unresolvable_templates_remain_conservatively_protected() {
    let root = TestRoot::new();
    for template in [
        "{{paths.install_root}}",
        "{{paths.install_root}}/",
        "{{paths.install_root}}/{{instance.id}}",
        "{{paths.install_root}}/Saved/{{unknown.key}}",
        "{{paths.install_root}}/Saved/{{settings.}}",
        "{{paths.install_root}}/Saved/{{paths.config_dir}}",
        "prefix/{{paths.install_root}}/Saved",
        "{{paths.install_root}}suffix/Saved",
        "{{paths.install_root}}/Saved/{{instance.id",
    ] {
        let protected = declared_install_root_data_path(&descriptor(Some(template)), root.path())
            .unwrap()
            .expect(template);
        assert!(protected.path.is_none(), "{template}");
        assert!(protected.source.contains(template), "{template}");
    }
}

#[test]
fn invalid_path_segments_cannot_escape_or_hide_behind_a_dynamic_segment() {
    let root = TestRoot::new();
    for template in [
        "{{paths.install_root}}/../Saved",
        "{{paths.install_root}}/Saved/../other",
        "{{paths.install_root}}/Saved/{{instance.id}}/../../other",
        "{{paths.install_root}}/Saved/.. /other",
        "{{paths.install_root}}/Saved./other",
        "{{paths.install_root}}/D:/other",
        "{{paths.install_root}}/Saved\0/other",
    ] {
        let protected = declared_install_root_data_path(&descriptor(Some(template)), root.path())
            .unwrap()
            .expect(template);
        assert!(protected.path.is_none(), "{template}");
    }
}

#[test]
fn templates_outside_the_install_root_do_not_create_blanket_protection() {
    let root = TestRoot::new();
    for template in [None, Some("{{paths.instance_root}}/saves")] {
        assert!(
            declared_install_root_data_path(&descriptor(template), root.path())
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn path_inspection_errors_are_not_classified_as_empty_data() {
    let root = TestRoot::new();
    let file = root.path().join("not-a-directory");
    fs::write(&file, b"keep").unwrap();
    let error = contains_preserved_data(&file.join("child")).unwrap_err();
    assert!(error.contains("不是目录"));
    let error = contains_preserved_data(&root.path().join("invalid\0name")).unwrap_err();
    assert!(error.contains("已拒绝卸载"));
    assert_eq!(fs::read(file).unwrap(), b"keep");
}

#[test]
fn empty_directory_scans_enforce_entry_and_depth_limits() {
    let root = TestRoot::new();
    let broad = root.path().join("broad");
    fs::create_dir(&broad).unwrap();
    for index in 0..=MAX_DATA_ENTRIES {
        fs::create_dir(broad.join(index.to_string())).unwrap();
    }
    let error = contains_preserved_data(&broad).unwrap_err();
    assert!(error.contains(&format!("{MAX_DATA_ENTRIES} 个目录项")));

    let deep = root.path().join("deep");
    let mut leaf = deep.clone();
    for _ in 0..=MAX_DATA_DEPTH {
        leaf.push("d");
    }
    fs::create_dir_all(&leaf).unwrap();
    let error = contains_preserved_data(&deep).unwrap_err();
    assert!(error.contains(&format!("{MAX_DATA_DEPTH} 层目录")));
}

#[cfg(windows)]
#[test]
fn windows_junctions_and_their_missing_children_are_protected_without_following() {
    use std::os::windows::process::CommandExt;
    let root = TestRoot::new();
    let target = root.path().join("external-empty");
    let junction = root.path().join("saves");
    fs::create_dir(&target).unwrap();
    let output = std::process::Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(&junction)
        .arg(&target)
        .creation_flags(0x0800_0000)
        .output()
        .unwrap();
    assert!(output.status.success(), "junction fixture: {output:?}");
    assert!(contains_preserved_data(&junction).unwrap());
    assert!(contains_preserved_data(&junction.join("missing-child")).unwrap());
    fs::remove_dir(&junction).unwrap();
    assert!(target.is_dir());
}

#[cfg(unix)]
#[test]
fn symlinks_and_their_missing_children_are_protected_without_following() {
    let root = TestRoot::new();
    let target = root.path().join("external-empty");
    let link = root.path().join("saves");
    fs::create_dir(&target).unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();
    assert!(contains_preserved_data(&link).unwrap());
    assert!(contains_preserved_data(&link.join("missing-child")).unwrap());
    fs::remove_file(&link).unwrap();
    assert!(target.is_dir());
}
