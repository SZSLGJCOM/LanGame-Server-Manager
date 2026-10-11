use super::*;

fn target(path: PathBuf) -> ResolvedManualModTarget {
    ResolvedManualModTarget {
        instance_id: "instance".into(),
        module_id: "arksurvivalascended".into(),
        source_label: "local".into(),
        target_label: "Mods".into(),
        target_path: path,
        accepts: vec!["zip".into(), "folder".into(), "pak".into()],
        id_strategy: Some("numeric_prefix".into()),
    }
}

#[test]
fn manual_mod_zip_affected_root_names_exclude_untouched_inventory() {
    let root = staging_tests::staging_test_root("affected-zip-roots");
    let target_path = root.join("mods");
    fs::create_dir_all(target_path.join("999999-Unrelated")).unwrap();
    fs::write(target_path.join("999999-Unrelated/keep.pak"), "keep").unwrap();
    let archive = root.join("bundle.zip");
    staging_tests::write_staging_test_zip(
        &archive,
        &[
            ("222222-Second/mod.pak", b"second"),
            ("111111-First/nested/mod.pak", b"first"),
            ("222222-Second/config.json", b"settings"),
        ],
    );
    let result = stage_manual_mod_sources(target(target_path.clone()), vec![archive]).unwrap();
    let serialized = serde_json::to_value(&result).unwrap();
    let unrelated = fs::read_to_string(target_path.join("999999-Unrelated/keep.pak")).unwrap();
    staging_tests::assert_no_staging_transaction(&root);
    fs::remove_dir_all(root).unwrap();
    assert_eq!(unrelated, "keep");
    assert_eq!(result.copied_file_count, 3);
    assert_eq!(
        serialized["affected_root_names"],
        serde_json::json!(["111111-First", "222222-Second"])
    );
}

#[test]
fn manual_mod_file_and_folder_affected_root_names_preserve_exact_names() {
    let root = staging_tests::staging_test_root("affected-file-folder-roots");
    let target_path = root.join("mods");
    let folder = root.join("222222-Folder");
    fs::create_dir_all(folder.join("nested")).unwrap();
    fs::write(folder.join("nested/mod.pak"), "folder").unwrap();
    let file = root.join("111111-File.pak");
    fs::write(&file, "file").unwrap();
    let result = stage_manual_mod_sources(target(target_path), vec![folder, file]).unwrap();
    let serialized = serde_json::to_value(&result).unwrap();
    staging_tests::assert_no_staging_transaction(&root);
    fs::remove_dir_all(root).unwrap();
    assert_eq!(result.copied_file_count, 2);
    assert_eq!(
        serialized["affected_root_names"],
        serde_json::json!(["111111-File.pak", "222222-Folder"])
    );
}

#[test]
fn manual_mod_dropped_folder_retains_trailing_unicode_space() {
    let root = staging_tests::staging_test_root("dropped-folder-unicode-space");
    let plain = root.join("222222-Folder");
    let selected = root.join("222222-Folder\u{3000}");
    fs::create_dir_all(&plain).unwrap();
    fs::create_dir_all(&selected).unwrap();
    fs::write(plain.join("mod.pak"), "unselected-folder").unwrap();
    fs::write(selected.join("mod.pak"), "selected-folder").unwrap();
    let normalized = crate::commands::commands_mods::normalize_manual_mod_source_paths(vec![
        selected.to_string_lossy().into_owned(),
        String::from(" \t\u{3000}"),
    ])
    .unwrap();
    let target_path = root.join("mods");
    let result = stage_manual_mod_sources(target(target_path.clone()), normalized.clone()).unwrap();
    let selected_contents = fs::read(target_path.join("222222-Folder\u{3000}/mod.pak")).ok();
    let wrong_folder_exists = target_path.join("222222-Folder").exists();
    staging_tests::assert_no_staging_transaction(&root);
    fs::remove_dir_all(&root).unwrap();
    assert_eq!(normalized, [selected]);
    assert_eq!(
        selected_contents.as_deref(),
        Some(b"selected-folder".as_slice())
    );
    assert!(
        !wrong_folder_exists,
        "the similarly named unselected folder must not be imported"
    );
    assert_eq!(result.affected_root_names, ["222222-Folder\u{3000}"]);
}

#[test]
fn manual_mod_dropped_file_retains_trailing_unicode_space() {
    let root = staging_tests::staging_test_root("dropped-file-unicode-space");
    let plain = root.join("111111-File.pak");
    let selected = root.join("111111-File.pak\u{3000}");
    fs::write(&plain, "unselected-file").unwrap();
    fs::write(&selected, "selected-file").unwrap();
    let normalized = crate::commands::commands_mods::normalize_manual_mod_source_paths(vec![
        selected.to_string_lossy().into_owned(),
    ])
    .unwrap();
    let target_path = root.join("mods");
    let result = stage_manual_mod_sources(target(target_path.clone()), normalized.clone()).unwrap();
    let selected_contents = fs::read(target_path.join("111111-File.pak\u{3000}")).ok();
    let wrong_file_exists = target_path.join("111111-File.pak").exists();
    staging_tests::assert_no_staging_transaction(&root);
    fs::remove_dir_all(&root).unwrap();
    assert_eq!(normalized, [selected]);
    assert_eq!(
        selected_contents.as_deref(),
        Some(b"selected-file".as_slice())
    );
    assert!(
        !wrong_file_exists,
        "the similarly named unselected file must not be imported"
    );
    assert_eq!(result.affected_root_names, ["111111-File.pak\u{3000}"]);
}
