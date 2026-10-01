use super::*;

#[test]
fn manual_mod_inventory_rejects_linked_payloads_instead_of_reporting_success() {
    let root = std::env::temp_dir().join(format!("langame-mod-inventory-{}", uuid::Uuid::new_v4()));
    let inventory = root.join("inventory");
    let outside = root.join("outside");
    fs::create_dir_all(&inventory).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("private.txt"), b"not mod content").unwrap();
    fs::write(outside.join("Info.json"), r#"{"PackageName":"Outside"}"#).unwrap();
    let link = inventory.join("linked");
    #[cfg(windows)]
    {
        let output = std::process::Command::new("cmd.exe")
            .args(["/D", "/C", "mklink", "/J"])
            .arg(&link)
            .arg(&outside)
            .output()
            .expect("create test junction");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    let result = read_manual_mod_inventory_items(None, &inventory);
    let inferred_id = infer_palworld_package_name(&link);
    #[cfg(windows)]
    fs::remove_dir(&link).unwrap();
    #[cfg(unix)]
    fs::remove_file(&link).unwrap();
    fs::remove_dir_all(&root).unwrap();
    let error = result.expect_err("linked payloads must not be counted as local Mod files");
    assert!(
        error.contains("reparse") || error.contains("symbolic link"),
        "{error}"
    );
    assert_eq!(inferred_id, None);
}

#[test]
fn manual_mod_inventory_bounds_palworld_metadata_and_requires_regular_files() {
    let root = std::env::temp_dir().join(format!("langame-mod-info-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let info_path = root.join("Info.json");
    fs::write(&info_path, r#"{"PackageName":"ValidPackage"}"#).unwrap();
    assert_eq!(
        infer_palworld_package_name(&root).as_deref(),
        Some("ValidPackage")
    );
    fs::OpenOptions::new()
        .write(true)
        .open(&info_path)
        .unwrap()
        .set_len(1024 * 1024 + 1)
        .unwrap();
    assert_eq!(infer_palworld_package_name(&root), None);
    fs::remove_file(&info_path).unwrap();
    fs::create_dir(&info_path).unwrap();
    assert_eq!(infer_palworld_package_name(&root), None);
    fs::remove_dir_all(&root).unwrap();
}
