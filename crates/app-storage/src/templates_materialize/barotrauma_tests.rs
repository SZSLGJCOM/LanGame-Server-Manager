#![cfg(test)]

use super::*;

#[test]
fn publisher_files_stay_inside_the_instance_data_directory() {
    let root = std::env::temp_dir().join(format!("barotrauma-data-{}", uuid::Uuid::new_v4()));
    let install = root.join("package");
    let instance = root.join("instance");
    let outside = root.join("outside");
    let linked_data = instance.join("Data");
    for path in [
        &install.join("Content"),
        &install.join("Data"),
        &instance,
        &outside,
    ] {
        fs::create_dir_all(path).unwrap();
    }
    fs::write(install.join("DedicatedServer.exe"), b"server").unwrap();
    fs::write(install.join("Data/dlc_catalog.xml"), b"official catalog").unwrap();
    fs::write(install.join("Data/PublicKey.dat"), b"official key").unwrap();
    fs::write(outside.join("dlc_catalog.xml"), b"outside catalog").unwrap();
    #[cfg(windows)]
    assert!(
        std::process::Command::new("cmd")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&linked_data)
            .arg(&outside)
            .output()
            .unwrap()
            .status
            .success()
    );
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, &linked_data).unwrap();
    assert!(materialize_runtime(&install, &instance).is_err());
    assert_eq!(
        fs::read(outside.join("dlc_catalog.xml")).unwrap(),
        b"outside catalog"
    );
    assert!(!outside.join("PublicKey.dat").exists());
    #[cfg(windows)]
    fs::remove_dir(&linked_data).unwrap();
    #[cfg(unix)]
    fs::remove_file(&linked_data).unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn campaign_presets_upgrade_only_exact_published_stock() {
    let root = std::env::temp_dir().join(format!("barotrauma-presets-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    let source = root.join("package.xml");
    let target = root.join("instance.xml");
    let latest = include_str!(
        "../../../../modules/barotrauma/reference-configs/campaignsettings-v1.15.33.2.xml"
    )
    .replace("\r\n", "\n");
    let (hard, abyssal) = latest
        .rsplit_once("ShipyardPriceMultiplier=\"1.0\"")
        .unwrap();
    let old = format!("{hard}ShipyardPriceMultiplier=\"1.5\"{abyssal}").replace(
        "ShipyardPriceMultiplier=\"1.2\"",
        "ShipyardPriceMultiplier=\"2.0\"",
    );
    let current = latest.replace('\n', "\r\n");
    assert_eq!(sha256(current.as_bytes()), UPDATED_CAMPAIGN_PRESETS);
    fs::write(&source, &current).unwrap();
    for (text, hash) in [old.clone(), old.replace('\n', "\r\n")]
        .iter()
        .zip(PRIOR_CAMPAIGN_PRESETS)
    {
        assert_eq!(sha256(text.as_bytes()), hash);
        fs::write(&target, text).unwrap();
        copy_file(&source, &target, FilePolicy::CampaignPresets, &mut 0).unwrap();
        assert_eq!(fs::read(&target).unwrap(), current.as_bytes());
    }
    let edited = old.replace(
        "ShipyardPriceMultiplier=\"1.5\"",
        "ShipyardPriceMultiplier=\"1.7\"",
    );
    fs::write(&target, &edited).unwrap();
    copy_file(&source, &target, FilePolicy::CampaignPresets, &mut 0).unwrap();
    assert_eq!(fs::read(&target).unwrap(), edited.as_bytes());
    fs::write(&target, &old).unwrap();
    fs::write(
        &source,
        current.replace(
            "ShipyardPriceMultiplier=\"1.2\"",
            "ShipyardPriceMultiplier=\"1.3\"",
        ),
    )
    .unwrap();
    copy_file(&source, &target, FilePolicy::CampaignPresets, &mut 0).unwrap();
    assert_eq!(fs::read(&target).unwrap(), old.as_bytes());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn runtime_is_independent_and_preserves_instance_permissions_and_saves() {
    let root = std::env::temp_dir().join(format!(
        "barotrauma-layout-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let install = root.join("package");
    let instance = root.join("instance");
    fs::create_dir_all(install.join("Content")).unwrap();
    fs::create_dir_all(install.join("Data")).unwrap();
    fs::create_dir_all(instance.join("Data")).unwrap();
    fs::create_dir_all(instance.join("Content")).unwrap();
    fs::create_dir_all(instance.join("Multiplayer")).unwrap();
    fs::write(instance.join("Content/Operator.xml"), "operator content").unwrap();
    fs::write(install.join("DedicatedServer.exe"), "executable").unwrap();
    fs::write(install.join("DedicatedServer.dll"), "assembly").unwrap();
    fs::write(install.join("DedicatedServer.runtimeconfig.json"), "{}").unwrap();
    fs::write(install.join("Content/Vanilla.xml"), "package content").unwrap();
    fs::write(install.join("Data/campaignsettings.xml"), "default").unwrap();
    fs::write(install.join("Data/dlc_catalog.xml"), "catalog1").unwrap();
    fs::write(install.join("Data/PublicKey.dat"), b"key1").unwrap();
    fs::write(
        install.join("Data/bannedplayers.xml"),
        "another server's bans",
    )
    .unwrap();
    fs::write(
        install.join("Data/bannedplayers.txt"),
        "another server's old bans",
    )
    .unwrap();
    fs::write(
        install.join("Data/clientpermissions.xml"),
        "package permissions",
    )
    .unwrap();
    fs::write(
        instance.join("Data/clientpermissions.xml"),
        "instance permissions",
    )
    .unwrap();
    fs::write(instance.join("serversettings.xml"), "instance settings").unwrap();
    fs::write(instance.join("Multiplayer/world.save"), "retained world").unwrap();
    materialize_runtime(&install, &instance).unwrap();
    assert_eq!(
        fs::read(instance.join("Data/dlc_catalog.xml")).unwrap(),
        b"catalog1"
    );
    assert_eq!(
        fs::read(instance.join("Data/PublicKey.dat")).unwrap(),
        b"key1"
    );
    assert_eq!(
        fs::read_to_string(instance.join("DedicatedServer.dll")).unwrap(),
        "assembly"
    );
    assert_eq!(
        fs::read_to_string(instance.join("Content/Vanilla.xml")).unwrap(),
        "package content"
    );
    assert_eq!(
        fs::read_to_string(instance.join("Data/clientpermissions.xml")).unwrap(),
        "instance permissions"
    );
    assert_eq!(
        fs::read_to_string(instance.join("Data/campaignsettings.xml")).unwrap(),
        "default"
    );
    assert!(!instance.join("Data/bannedplayers.xml").exists());
    assert!(!instance.join("Data/bannedplayers.txt").exists());
    assert_eq!(
        fs::read_to_string(instance.join("serversettings.xml")).unwrap(),
        "instance settings"
    );
    assert_eq!(
        fs::read_to_string(instance.join("Multiplayer/world.save")).unwrap(),
        "retained world"
    );
    fs::write(install.join("DedicatedServer.dll"), "updated assembly").unwrap();
    // Steam can deliver changed bytes with exactly the same size and timestamp.
    for (name, replacement) in [("dlc_catalog.xml", "catalog2"), ("PublicKey.dat", "key2")] {
        let path = install.join("Data").join(name);
        let target_time = fs::metadata(instance.join("Data").join(name))
            .unwrap()
            .modified()
            .unwrap();
        fs::write(&path, replacement).unwrap();
        fs::OpenOptions::new()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(target_time)
            .unwrap();
    }
    fs::write(
        instance.join("Data/campaignsettings.xml"),
        "operator campaign",
    )
    .unwrap();
    materialize_runtime(&install, &instance).unwrap();
    assert_eq!(
        fs::read(instance.join("Data/dlc_catalog.xml")).unwrap(),
        b"catalog2"
    );
    assert_eq!(
        fs::read(instance.join("Data/PublicKey.dat")).unwrap(),
        b"key2"
    );
    assert_eq!(
        fs::read_to_string(instance.join("DedicatedServer.dll")).unwrap(),
        "updated assembly"
    );
    assert_eq!(
        fs::read_to_string(instance.join("Content/Operator.xml")).unwrap(),
        "operator content"
    );
    assert_eq!(
        fs::read_to_string(instance.join("Data/campaignsettings.xml")).unwrap(),
        "operator campaign"
    );
    assert!(materialize_runtime(&install, &install).is_err());
    let fresh_instance = root.join("fresh-instance");
    materialize_runtime(&install, &fresh_instance).unwrap();
    assert!(!fresh_instance.join("Data/clientpermissions.xml").exists());
    assert!(!fresh_instance.join("Data/bannedplayers.xml").exists());
    fs::remove_dir_all(root).unwrap();
}
