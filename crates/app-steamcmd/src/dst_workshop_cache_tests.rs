use super::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ROOT: AtomicU64 = AtomicU64::new(1);

fn test_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "lgsm-dst-workshop-{name}-{}-{}",
        std::process::id(),
        NEXT_ROOT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

fn write_item(root: &Path, id: &str, contents: &[u8]) {
    let item = root.join("content").join("322330").join(id);
    fs::create_dir_all(&item).unwrap();
    fs::write(item.join("modinfo.lua"), contents).unwrap();
}

fn item_metadata(id: &str, manifest: &str, size: usize) -> String {
    format!(
        "\"{id}\"\n{{\n\t\"manifest\"\t\"{manifest}\"\n\t\"size\"\t\"{size}\"\n\t\"timeupdated\"\t\"1700000000\"\n}}"
    )
}

fn item_details(id: &str, manifest: &str) -> String {
    format!(
        "\"{id}\"\n{{\n\t\"manifest\"\t\"{manifest}\"\n\t\"timeupdated\"\t\"1700000000\"\n\t\"timetouched\"\t\"1700000001\"\n}}"
    )
}

fn write_manifest(root: &Path, installed: &[String], details: &[String]) {
    fs::create_dir_all(root).unwrap();
    fs::write(
        root.join("appworkshop_322330.acf"),
        format!(
            "\"AppWorkshop\"\n{{\n\t\"appid\"\t\"322330\"\n\t\"SizeOnDisk\"\t\"0\"\n\t\"WorkshopItemsInstalled\"\n\t{{\n{}\n\t}}\n\t\"WorkshopItemDetails\"\n\t{{\n{}\n\t}}\n}}\n",
            installed.join("\n"),
            details.join("\n")
        ),
    )
    .unwrap();
}

#[test]
fn dst_workshop_cache_deploys_only_selected_items_and_preserves_target_entries() {
    let root = test_root("merge");
    let source = root.join("source");
    let target = root.join("target");
    write_item(&source, "111", b"one");
    write_item(&source, "222", b"four");
    write_manifest(
        &source,
        &[
            item_metadata("111", "m111", 3),
            item_metadata("222", "m222", 4),
        ],
        &[item_details("111", "m111"), item_details("222", "m222")],
    );
    write_item(&target, "333", b"three");
    write_manifest(
        &target,
        &[item_metadata("333", "m333", 5)],
        &[item_details("333", "m333")],
    );

    let result = deploy_dst_workshop_cache(&source, &target, &[String::from("111")]).unwrap();

    assert_eq!(result.item_ids, [String::from("111")]);
    assert_eq!(result.file_count, 1);
    assert_eq!(result.byte_count, 3);
    assert!(!result.unchanged);
    assert_eq!(
        fs::read(target.join("content/322330/111/modinfo.lua")).unwrap(),
        b"one"
    );
    assert!(target.join("content/322330/333/modinfo.lua").is_file());
    assert!(!target.join("content/322330/222").exists());
    let manifest = fs::read_to_string(target.join("appworkshop_322330.acf")).unwrap();
    assert!(manifest.contains("\"111\""));
    assert!(manifest.contains("\"333\""));
    assert!(!manifest.contains("\"222\""));
    assert!(!fs::read_dir(&target).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".lgsm-dst-workshop")
    }));

    let repeated = deploy_dst_workshop_cache(&source, &target, &[String::from("111")]).unwrap();
    assert!(repeated.unchanged);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn dst_workshop_cache_rejects_incomplete_source_before_touching_target() {
    let root = test_root("incomplete");
    let source = root.join("source");
    let target = root.join("target");
    write_item(&source, "111", b"one");
    write_item(&source, "222", b"bad");
    write_manifest(
        &source,
        &[
            item_metadata("111", "m111", 3),
            item_metadata("222", "m222", 99),
        ],
        &[item_details("111", "m111"), item_details("222", "m222")],
    );
    write_item(&target, "111", b"old");
    write_manifest(
        &target,
        &[item_metadata("111", "old", 3)],
        &[item_details("111", "old")],
    );
    let original_manifest = fs::read(target.join("appworkshop_322330.acf")).unwrap();

    let error = deploy_dst_workshop_cache(
        &source,
        &target,
        &[String::from("111"), String::from("222")],
    )
    .expect_err("size mismatch must reject the source cache");

    assert!(matches!(
        error,
        DstWorkshopCacheError::IncompleteSource { .. }
    ));
    assert_eq!(
        fs::read(target.join("content/322330/111/modinfo.lua")).unwrap(),
        b"old"
    );
    assert_eq!(
        fs::read(target.join("appworkshop_322330.acf")).unwrap(),
        original_manifest
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn dst_workshop_cache_validates_ids_and_same_root_is_a_verified_noop() {
    let root = test_root("noop");
    write_item(&root, "111", b"one");
    write_manifest(
        &root,
        &[item_metadata("111", "m111", 3)],
        &[item_details("111", "m111")],
    );

    let result = deploy_dst_workshop_cache(&root, &root, &[String::from("111")]).unwrap();
    assert!(result.unchanged);
    assert_eq!(result.file_count, 1);

    let error = deploy_dst_workshop_cache(&root, &root, &[String::from("111/222")])
        .expect_err("workshop ids must be decimal path components");
    assert!(matches!(error, DstWorkshopCacheError::InvalidItemId { .. }));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn dst_workshop_cache_rejects_linked_source_payloads() {
    let root = test_root("link");
    let source = root.join("source");
    let target = root.join("target");
    write_item(&source, "111", b"one");
    write_manifest(
        &source,
        &[item_metadata("111", "m111", 3)],
        &[item_details("111", "m111")],
    );
    let link_source = root.join("junction-source");
    fs::create_dir(&link_source).unwrap();
    fs::write(link_source.join("linked.lua"), b"linked").unwrap();
    let link = source.join("content/322330/111/linked-directory");
    create_directory_link(&link_source, &link)
        .expect("test environment must support a linked-directory fixture");

    let error = deploy_dst_workshop_cache(&source, &target, &[String::from("111")])
        .expect_err("linked payload entries must be rejected");
    assert!(matches!(error, DstWorkshopCacheError::UnsafePath { .. }));
    assert!(!target.exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn dst_workshop_cache_rejects_manifest_nesting_beyond_the_parser_limit() {
    let root = test_root("manifest-depth");
    let source = root.join("source");
    let target = root.join("target");
    fs::create_dir_all(&source).unwrap();
    let mut manifest = String::new();
    for depth in 0..70 {
        manifest.push_str(&format!("\"level-{depth}\"\n{{\n"));
    }
    for _ in 0..70 {
        manifest.push_str("}\n");
    }
    fs::write(source.join("appworkshop_322330.acf"), manifest).unwrap();

    let error = deploy_dst_workshop_cache(&source, &target, &[String::from("111")])
        .expect_err("deeply nested KeyValues must be rejected before recursive parsing overflows");

    assert!(matches!(
        error,
        DstWorkshopCacheError::InvalidManifest { .. }
    ));
    assert!(!target.exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn dst_workshop_cache_rolls_back_every_item_when_a_later_publish_conflicts() {
    let root = test_root("publish-rollback");
    let source = root.join("source");
    let target = root.join("target");
    write_item(&source, "111", b"new-one");
    write_item(&source, "222", b"new-two");
    write_manifest(
        &source,
        &[
            item_metadata("111", "new-111", 7),
            item_metadata("222", "new-222", 7),
        ],
        &[
            item_details("111", "new-111"),
            item_details("222", "new-222"),
        ],
    );
    write_item(&target, "111", b"old-one");
    write_item(&target, "222", b"old-two");
    write_manifest(
        &target,
        &[
            item_metadata("111", "old-111", 7),
            item_metadata("222", "old-222", 7),
        ],
        &[
            item_details("111", "old-111"),
            item_details("222", "old-222"),
        ],
    );
    let original_manifest = fs::read(target.join("appworkshop_322330.acf")).unwrap();
    crate::dst_workshop_cache_publish::fail_item_backup_for_test("222");

    let error = deploy_dst_workshop_cache(
        &source,
        &target,
        &[String::from("111"), String::from("222")],
    )
    .expect_err("a later publish conflict must fail the deployment");

    assert!(matches!(error, DstWorkshopCacheError::Io { .. }));
    assert_eq!(
        fs::read(target.join("content/322330/111/modinfo.lua")).unwrap(),
        b"old-one"
    );
    assert_eq!(
        fs::read(target.join("content/322330/222/modinfo.lua")).unwrap(),
        b"old-two"
    );
    assert_eq!(
        fs::read(target.join("appworkshop_322330.acf")).unwrap(),
        original_manifest
    );
    assert!(!fs::read_dir(&target).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".lgsm-dst-workshop")
    }));
    fs::remove_dir_all(root).unwrap();
}

#[cfg(windows)]
fn create_directory_link(source: &Path, target: &Path) -> std::io::Result<()> {
    let source_arg = source.to_string_lossy().replace('/', "\\");
    let target_arg = target.to_string_lossy().replace('/', "\\");
    let output = std::process::Command::new("cmd.exe")
        .args(["/D", "/C", "mklink", "/J"])
        .arg(target_arg)
        .arg(source_arg)
        .output()?;
    if output.status.success() {
        return Ok(());
    }
    Err(std::io::Error::other(format!(
        "mklink /J failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    )))
}

#[cfg(unix)]
fn create_directory_link(source: &Path, target: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(source, target)
}
