use super::*;
use crate::commands::commands_managed_save_control::tests::Fixture;
use serde_json::json;
use std::fs;

fn details(module: &str, root: &Path) -> InstanceDetails {
    serde_json::from_value(json!({
        "summary":{"id":"retired-save","name":"Retired save","module_id":module,
            "status":"Stopped","bind_ip":"127.0.0.1","port_count":0,"autostart":false},
        "config_file_path":root.join("config/instance.json"),"saves_path":root.join("data/saves"),
        "auto_backup_on_stop":false,"backup_retention_count":0,"settings_json":"{}",
        "ports":[],"active_run":null
    }))
    .unwrap()
}

fn record(
    root: &Path,
    module: &str,
    files: BTreeMap<String, String>,
    pending_files: BTreeMap<String, String>,
) {
    fs::write(root.join(super::super::OWNER), serde_json::to_vec(&json!({
        "protocol":1,"module_id":module,"source_sha256":"a".repeat(64),"files":files,"pending_files":pending_files
    })).unwrap()).unwrap();
}

#[test]
fn retired_managed_extensions_are_removed_without_touching_other_mods_or_worlds() {
    for module in [
        "minecraft",
        "projectzomboid",
        "terraria",
        "sevendaystodie",
        "unturned",
        "necesse",
        "romestead",
        "rimworld",
    ] {
        let fixture = Fixture::new();
        let profile = managed(module).unwrap();
        let mut files = BTreeMap::new();
        for relative in profile.files {
            let path = fixture.0.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, relative.as_bytes()).unwrap();
            files.insert((*relative).into(), io::digest(relative.as_bytes()));
        }
        fs::write(fixture.0.join("operator-mod.dll"), b"operator mod").unwrap();
        fs::write(fixture.0.join("world.sav"), b"player world").unwrap();
        record(&fixture.0, module, files, BTreeMap::new());
        cleanup_managed(&details(module, &fixture.0), &fixture.0, profile).unwrap();
        assert!(!fixture.0.join(super::super::OWNER).exists());
        for relative in managed(module).unwrap().files {
            assert!(!fixture.0.join(relative).exists());
        }
        assert_eq!(
            fs::read(fixture.0.join("operator-mod.dll")).unwrap(),
            b"operator mod"
        );
        assert_eq!(
            fs::read(fixture.0.join("world.sav")).unwrap(),
            b"player world"
        );
        cleanup_managed(
            &details(module, &fixture.0),
            &fixture.0,
            managed(module).unwrap(),
        )
        .unwrap();
    }
}

#[test]
fn retired_cleanup_preserves_changed_files_and_rejects_forged_targets_before_removing_anything() {
    let fixture = Fixture::new();
    let module = "unturned";
    let profile = managed(module).unwrap();
    let relative = profile.files[0];
    let path = fixture.0.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, b"operator changed plugin").unwrap();
    record(
        &fixture.0,
        module,
        BTreeMap::from([(relative.into(), io::digest(b"original plugin"))]),
        BTreeMap::new(),
    );
    assert!(
        cleanup_managed(
            &details(module, &fixture.0),
            &fixture.0,
            managed(module).unwrap()
        )
        .is_err()
    );
    assert_eq!(fs::read(&path).unwrap(), b"operator changed plugin");
    record(
        &fixture.0,
        module,
        BTreeMap::from([("world.sav".into(), io::digest(b"world"))]),
        BTreeMap::new(),
    );
    fs::write(fixture.0.join("world.sav"), b"world").unwrap();
    assert!(
        cleanup_managed(
            &details(module, &fixture.0),
            &fixture.0,
            managed(module).unwrap()
        )
        .is_err()
    );
    assert_eq!(fs::read(fixture.0.join("world.sav")).unwrap(), b"world");
    fs::remove_file(fixture.0.join(super::super::OWNER)).unwrap();
    assert!(
        cleanup_managed(
            &details(module, &fixture.0),
            &fixture.0,
            managed(module).unwrap()
        )
        .is_err()
    );
    assert!(path.exists());
}

#[test]
fn interrupted_retirement_resumes_from_the_original_pending_owner() {
    let fixture = Fixture::new();
    let module = "unturned";
    let profile = managed(module).unwrap();
    // A previous cleanup already removed the first leaf. Keep the owner intact.
    let relative = profile.files[1];
    let path = fixture.0.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, b"pending manifest").unwrap();
    record(
        &fixture.0,
        module,
        BTreeMap::from([(profile.files[0].into(), io::digest(b"old plugin"))]),
        BTreeMap::from([(relative.into(), io::digest(b"pending manifest"))]),
    );
    cleanup_managed(&details(module, &fixture.0), &fixture.0, profile).unwrap();
    assert!(!path.exists());
    assert!(!fixture.0.join(super::super::OWNER).exists());
}

#[test]
fn ark_retirement_preserves_framework_tools_and_foreign_plugins() {
    for (module, owner, directory, dll) in [
        (
            "arksurvivalevolved",
            ".langame-ase-save.json",
            "LgsmAseWorldSave",
            "LgsmAseWorldSave.dll",
        ),
        (
            "arksurvivalascended",
            ".langame-asa-save.json",
            "LgsmAsaWorldSave",
            "LgsmAsaWorldSave.dll",
        ),
    ] {
        let fixture = Fixture::new();
        let plugins = fixture.0.join("ShooterGame/Binaries/Win64/ArkApi/Plugins");
        let target = plugins.join(directory);
        fs::create_dir_all(&target).unwrap();
        fs::create_dir_all(plugins.join("OperatorPlugin")).unwrap();
        fs::create_dir_all(plugins.join("LgsmArkTools")).unwrap();
        fs::write(plugins.join("LgsmArkTools/LgsmArkTools.dll"), b"tools").unwrap();
        fs::write(plugins.join("OperatorPlugin/operator.dll"), b"operator").unwrap();
        let mut files = BTreeMap::new();
        for name in [dll, "PluginInfo.json", "THIRD_PARTY_NOTICES.txt"] {
            fs::write(target.join(name), name.as_bytes()).unwrap();
            files.insert(name.to_owned(), io::digest(name.as_bytes()));
        }
        fs::write(
            fixture.0.join(owner),
            serde_json::to_vec(
                &json!({"schema":1,"module":module,"files":files,"pending_files":null}),
            )
            .unwrap(),
        )
        .unwrap();
        cleanup_ark(module, &fixture.0).unwrap();
        assert!(!target.join(dll).exists());
        assert_eq!(
            fs::read(plugins.join("LgsmArkTools/LgsmArkTools.dll")).unwrap(),
            b"tools"
        );
        assert_eq!(
            fs::read(plugins.join("OperatorPlugin/operator.dll")).unwrap(),
            b"operator"
        );
        assert!(!fixture.0.join(owner).exists());
    }
}

#[test]
fn cleanup_never_mutates_a_running_instance() {
    let fixture = Fixture::new();
    let mut instance = details("unturned", &fixture.0);
    instance.summary.status = crate::commands::InstanceStatus::Running;
    assert!(stopped(&instance).is_err());
}

#[test]
fn ordinary_shared_programs_bypass_retirement_process_admission_without_old_components() {
    let fixture = Fixture::new();
    let runtime = fixture.0.join("shared-program");
    fs::create_dir_all(runtime.join("jre/bin")).unwrap();
    fs::write(
        runtime.join("jre/bin/java.exe"),
        b"shared Java image; never executed",
    )
    .unwrap();
    fs::create_dir_all(fixture.0.join("data/mods")).unwrap();
    fs::write(fixture.0.join("data/mods/operator.jar"), b"operator mod").unwrap();
    let instance = details("necesse", &fixture.0);
    // No process admission is needed when neither program nor per-instance data
    // contains a retired observer. Other servers may keep this shared image live.
    assert!(!needs_cleanup(&instance, &runtime).unwrap());
    fs::write(
        fixture.0.join("data/.langame-necesse-save.json"),
        b"owner presence selects cleanup",
    )
    .unwrap();
    assert!(needs_cleanup(&instance, &runtime).unwrap());
}
