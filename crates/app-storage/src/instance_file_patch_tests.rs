use super::*;

struct Fixture {
    root: PathBuf,
    file: String,
}

impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("lgsm-instance-patch-{}", uuid::Uuid::new_v4()));
        let file = String::from("runtime/mods/local_mod/modmain.lua");
        fs::create_dir_all(root.join("runtime/mods/local_mod")).unwrap();
        fs::write(root.join("runtime/.langame-private-runtime"), b"managed\n").unwrap();
        fs::write(
            root.join(&file),
            b"local priority = 1\r\nreturn priority\r\n",
        )
        .unwrap();
        Self { root, file }
    }

    fn prepare(&self) -> PreparedInstanceTextPatch {
        self.prepare_for("priority = 1", "priority = 5").unwrap()
    }

    fn prepare_for(
        &self,
        before: &str,
        after: &str,
    ) -> Result<PreparedInstanceTextPatch, StorageError> {
        let source = read_file(&self.root, &self.file)?;
        prepare_patch(
            &self.root,
            "test-instance",
            InstanceTextPatch {
                file: self.file.clone(),
                source_sha256: source.source_sha256,
                before: before.to_owned(),
                after: after.to_owned(),
            },
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn patch_preserves_other_bytes_and_publishes_original_backup() {
    let fixture = Fixture::new();
    let original = fs::read(fixture.root.join(&fixture.file)).unwrap();
    let prepared = fixture.prepare();
    assert_eq!(prepared.preview().before, "priority = 1");
    assert_eq!(prepared.preview().after, "priority = 5");
    let result = io::apply_patch(&prepared).unwrap();
    assert!(result.read_back_verified);
    assert_eq!(
        fs::read(fixture.root.join(&fixture.file)).unwrap(),
        b"local priority = 5\r\nreturn priority\r\n"
    );
    let backup = fixture
        .root
        .join("data/.langame/file-patches")
        .join(&result.backup_id);
    assert_eq!(fs::read(backup.join("original")).unwrap(), original);
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(backup.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["file"], fixture.file);
    assert_eq!(manifest["sourceSha256"], sha256(&original));
    assert_eq!(
        result.result_sha256,
        sha256(&fs::read(fixture.root.join(&fixture.file)).unwrap())
    );
}

#[test]
fn patch_rejects_stale_hash_and_post_preview_change_without_writing() {
    let fixture = Fixture::new();
    let prepared = fixture.prepare();
    fs::write(fixture.root.join(&fixture.file), b"external edit").unwrap();
    assert!(
        io::apply_patch(&prepared)
            .unwrap_err()
            .to_string()
            .contains("changed after")
    );
    assert_eq!(
        fs::read(fixture.root.join(&fixture.file)).unwrap(),
        b"external edit"
    );
    assert!(!fixture.root.join("data/.langame/file-patches").exists());
    let patch = InstanceTextPatch {
        file: fixture.file.clone(),
        source_sha256: prepared.preview.source_sha256,
        before: String::from("external"),
        after: String::from("updated"),
    };
    assert!(prepare_patch(&fixture.root, "test-instance", patch).is_err());
}

#[test]
fn patch_requires_unique_nonempty_match_including_overlapping_matches() {
    let fixture = Fixture::new();
    for (content, before) in [("aaa", "aa"), ("你好你好", "你好"), ("same\nsame", "same")] {
        fs::write(fixture.root.join(&fixture.file), content).unwrap();
        assert!(
            fixture
                .prepare_for(before, "replacement")
                .unwrap_err()
                .to_string()
                .contains("exactly once")
        );
    }
    assert!(fixture.prepare_for("", "replacement").is_err());
    assert!(fixture.prepare_for("same", "same").is_err());
    assert!(fixture.prepare_for("missing", "replacement").is_err());
}

#[test]
fn path_policy_rejects_generated_shared_traversal_and_stream_targets() {
    for file in [
        "config/instance.json",
        "config/Master/modoverrides.lua",
        "runtime/mods/dedicated_server_mods_setup.lua",
        "mods/local/modmain.lua",
        "runtime/mods/local/../modmain.lua",
        "runtime/mods/local/modmain.lua:stream.txt",
        "runtime/mods/local./modmain.lua",
        "runtime\\mods\\local\\modmain.lua",
        "data/ugc/other/content/322330/123/modmain.lua",
        "data/ugc/Master/content/322330/not-numeric/modmain.lua",
        "runtime/mods/local/server.exe",
    ] {
        assert!(validate_relative_file(file).is_err(), "accepted {file}");
    }
    assert!(validate_relative_file("data/ugc/Master/content/322330/123/modmain.lua").is_ok());
    assert!(validate_relative_file("runtime/mods/local/scripts/component.lua").is_ok());
}

#[test]
fn private_runtime_marker_is_required_again_when_applying() {
    let fixture = Fixture::new();
    let prepared = fixture.prepare();
    fs::remove_file(fixture.root.join("runtime/.langame-private-runtime")).unwrap();
    assert!(read_file(&fixture.root, &fixture.file).is_err());
    assert!(io::apply_patch(&prepared).is_err());
}

#[test]
fn strict_utf8_size_and_patch_limits_preserve_bom_and_line_endings() {
    let fixture = Fixture::new();
    fs::write(
        fixture.root.join(&fixture.file),
        b"\xef\xbb\xbfpriority = 1\r\n",
    )
    .unwrap();
    io::apply_patch(&fixture.prepare()).unwrap();
    assert_eq!(
        fs::read(fixture.root.join(&fixture.file)).unwrap(),
        b"\xef\xbb\xbfpriority = 5\r\n"
    );
    for invalid_bytes in [
        vec![0xff, 0xfe, 1, 0],
        b"nul\0text".to_vec(),
        vec![b'x'; MAX_FILE_BYTES + 1],
    ] {
        fs::write(fixture.root.join(&fixture.file), invalid_bytes).unwrap();
        assert!(read_file(&fixture.root, &fixture.file).is_err());
    }
    fs::write(fixture.root.join(&fixture.file), b"unique").unwrap();
    assert!(
        fixture
            .prepare_for("unique", &"x".repeat(MAX_PATCH_BYTES))
            .is_err()
    );
    assert!(fixture.prepare_for("unique", "").is_err());
}

#[test]
fn failed_atomic_write_retains_original_and_reports_backup() {
    let fixture = Fixture::new();
    let prepared = fixture.prepare();
    crate::atomic_file::fail_next_atomic_write_for_test(&fixture.root.join(&fixture.file));
    let error = io::apply_patch(&prepared).unwrap_err().to_string();
    assert!(error.contains("backupId="));
    assert_eq!(
        fs::read(fixture.root.join(&fixture.file)).unwrap(),
        prepared.original
    );
    assert_eq!(
        fs::read_dir(fixture.root.join("data/.langame/file-patches"))
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn failed_readback_does_not_claim_success_and_retains_recovery_id() {
    let fixture = Fixture::new();
    let prepared = fixture.prepare();
    let error = io::apply_with_failed_readback(&prepared)
        .unwrap_err()
        .to_string();
    assert!(error.contains("read-back failed"));
    assert!(error.contains("backupId="));
    assert_eq!(
        fs::read(fixture.root.join(&fixture.file)).unwrap(),
        prepared.replacement
    );
}

#[test]
fn backups_are_bounded_without_deleting_recovery_material() {
    let fixture = Fixture::new();
    let parent = fixture.root.join("data/.langame/file-patches");
    fs::create_dir_all(&parent).unwrap();
    for index in 0..64 {
        fs::write(parent.join(format!("preserved-{index}")), b"retained").unwrap();
    }
    let prepared = fixture.prepare();
    assert!(
        io::apply_patch(&prepared)
            .unwrap_err()
            .to_string()
            .contains("capacity (64)")
    );
    assert_eq!(fs::read_dir(&parent).unwrap().count(), 64);
    assert_eq!(
        fs::read(fixture.root.join(&fixture.file)).unwrap(),
        prepared.original
    );
}

#[test]
fn list_is_bounded_and_only_returns_supported_instance_files() {
    let fixture = Fixture::new();
    let ugc = fixture.root.join("data/ugc/Master/content/322330/123");
    fs::create_dir_all(&ugc).unwrap();
    fs::write(ugc.join("modmain.lua"), b"return true").unwrap();
    fs::write(ugc.join("server.exe"), b"binary").unwrap();
    let list = list_files(&fixture.root).unwrap();
    assert_eq!(list.files.len(), 2);
    assert!(!list.truncated);
    for index in 0..65 {
        fs::write(ugc.join(format!("file-{index}.lua")), b"return true").unwrap();
    }
    let list = list_files(&fixture.root).unwrap();
    assert!(list.truncated);
    assert_eq!(list.files.len(), MAX_LIST_FILES);
    assert!(list.scanned_entries <= MAX_SCAN_ENTRIES);
}

#[test]
fn replacing_private_hardlink_does_not_modify_shared_original() {
    let fixture = Fixture::new();
    let shared = fixture.root.join("shared-reference.lua");
    fs::hard_link(fixture.root.join(&fixture.file), &shared).unwrap();
    let original = fs::read(&shared).unwrap();
    io::apply_patch(&fixture.prepare()).unwrap();
    assert_eq!(fs::read(shared).unwrap(), original);
    assert_ne!(
        fs::read(fixture.root.join(&fixture.file)).unwrap(),
        original
    );
}

#[cfg(windows)]
#[test]
fn directory_guards_block_replacement_until_io_finishes() {
    let fixture = Fixture::new();
    let parent = fixture.root.join("runtime/mods/local_mod");
    let guards = io::guard_directories(&parent).unwrap();
    assert!(fs::rename(&parent, fixture.root.join("replaced")).is_err());
    drop(guards);
    fs::rename(&parent, fixture.root.join("replaced")).unwrap();
}

#[test]
fn hash_uses_sha256_and_debug_omits_file_contents() {
    assert_eq!(
        sha256(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    let fixture = Fixture::new();
    let debug = format!("{:?}", fixture.prepare());
    assert!(!debug.contains("priority"));
    assert!(!debug.contains("original"));
}

#[tokio::test]
async fn public_operations_bind_managed_instance_and_recheck_state_and_lease() {
    let fixture = Fixture::new();
    let paths = StoragePaths {
        app_data_root: fixture.root.join("appdata"),
        settings_path: fixture.root.join("appdata/settings.json"),
        database_path: fixture.root.join("appdata/db/lgs.db"),
        logs_root: fixture.root.join("logs"),
        modules_root: fixture.root.join("modules"),
        migrations_root: fixture.root.join("migrations"),
        steamcmd_root: fixture.root.join("steamcmd"),
        games_root: fixture.root.join("games"),
        instances_root: fixture.root.join("instances"),
        archives_root: fixture.root.join("instances").join(".trash"),
    };
    let root = paths.instances_root.join("one");
    fs::create_dir_all(root.join("config")).unwrap();
    fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
    fs::rename(fixture.root.join("runtime"), root.join("runtime")).unwrap();
    crate::initialize_database(&paths).await.unwrap();
    let pool = connect_pool(&paths).await.unwrap();
    sqlx::query("INSERT INTO modules (id, name, version) VALUES ('dontstarve', 'DST', '1'), ('other', 'Other', '1')").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO instances (id, name, module_id, data_path, config_path, logs_path, saves_path) VALUES ('one', 'One', 'dontstarve', ?1, ?2, ?3, ?4)")
        .bind(root.join("data").to_string_lossy().as_ref())
        .bind(root.join("config").to_string_lossy().as_ref())
        .bind(root.join("logs").to_string_lossy().as_ref())
        .bind(root.join("saves").to_string_lossy().as_ref())
        .execute(&pool).await.unwrap();
    assert_eq!(
        list_instance_patch_files(&paths, "one")
            .await
            .unwrap()
            .files
            .as_slice(),
        std::slice::from_ref(&fixture.file)
    );
    let file = read_instance_patch_file(&paths, "one", &fixture.file)
        .await
        .unwrap();
    let prepared = prepare_instance_file_patch(
        &paths,
        "one",
        InstanceTextPatch {
            file: fixture.file.clone(),
            source_sha256: file.source_sha256,
            before: String::from("priority = 1"),
            after: String::from("priority = 5"),
        },
    )
    .await
    .unwrap();
    sqlx::query("UPDATE instances SET status = 'running' WHERE id = 'one'")
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        apply_instance_file_patch(&paths, "one", prepared.clone())
            .await
            .unwrap_err()
            .to_string()
            .contains("Stop the instance")
    );
    sqlx::query("UPDATE instances SET status = 'stopped' WHERE id = 'one'")
        .execute(&pool)
        .await
        .unwrap();
    let lease = acquire_instance_settings_mutation_lock(&paths, "one").unwrap();
    assert!(matches!(
        apply_instance_file_patch(&paths, "one", prepared.clone()).await,
        Err(StorageError::InstanceSettingsLocked { .. })
    ));
    drop(lease);
    assert!(
        apply_instance_file_patch(&paths, "one", prepared)
            .await
            .unwrap()
            .read_back_verified
    );
    sqlx::query("UPDATE instances SET module_id = 'other' WHERE id = 'one'")
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        read_instance_patch_file(&paths, "one", &fixture.file)
            .await
            .is_ok()
    );
    let plugin = "runtime/plugins/Example.cs";
    fs::create_dir_all(root.join("runtime/plugins")).unwrap();
    fs::write(root.join(plugin), b"class Example { const int Limit = 1; }").unwrap();
    let workspace = crate::open_instance_workspace(&paths, "one").await.unwrap();
    let source = workspace.read_file(plugin).unwrap();
    assert!(source.entry.editable);
    let prepared = prepare_instance_file_patch(
        &paths,
        "one",
        InstanceTextPatch {
            file: plugin.into(),
            source_sha256: source.source_sha256,
            before: "Limit = 1".into(),
            after: "Limit = 2".into(),
        },
    )
    .await
    .unwrap();
    assert!(
        apply_instance_file_patch(&paths, "one", prepared)
            .await
            .unwrap()
            .read_back_verified
    );
    assert!(
        fs::read_to_string(root.join(plugin))
            .unwrap()
            .contains("Limit = 2")
    );
    sqlx::query("UPDATE instances SET module_id = 'dontstarve', config_path = ?1 WHERE id = 'one'")
        .bind(fixture.root.join("config").to_string_lossy().as_ref())
        .execute(&pool)
        .await
        .unwrap();
    assert!(list_instance_patch_files(&paths, "one").await.is_err());
    pool.close().await;
}

#[test]
fn list_bounds_unmatched_entries_and_deep_directories() {
    let fixture = Fixture::new();
    let directory = fixture.root.join("runtime/mods/local_mod");
    fs::remove_file(fixture.root.join(&fixture.file)).unwrap();
    for index in 0..MAX_SCAN_ENTRIES + 1 {
        fs::write(directory.join(format!("file-{index}.bin")), b"excluded").unwrap();
    }
    let result = list_files(&fixture.root).unwrap();
    assert!(result.truncated);
    assert_eq!(result.scanned_entries, MAX_SCAN_ENTRIES);
    assert!(result.files.is_empty());

    let fixture = Fixture::new();
    let mut directory = fixture.root.join("runtime/mods/local_mod");
    for _ in 0..MAX_DEPTH {
        directory.push("nested");
    }
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join("deep.lua"), b"unvisited").unwrap();
    let result = list_files(&fixture.root).unwrap();
    assert!(result.truncated);
    assert!(!result.files.iter().any(|file| file.ends_with("deep.lua")));
}

#[cfg(windows)]
#[test]
fn junction_target_is_rejected_on_read_and_apply() {
    use std::os::windows::process::CommandExt;

    let fixture = Fixture::new();
    let prepared = fixture.prepare();
    let link = fixture.root.join("runtime/mods/local_mod");
    let outside = fixture.root.join("outside-mod");
    fs::rename(&link, &outside).unwrap();
    let output = std::process::Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(link.to_string_lossy().replace('/', "\\"))
        .arg(outside.to_string_lossy().replace('/', "\\"))
        .creation_flags(0x0800_0000)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "junction fixture failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let read = read_file(&fixture.root, &fixture.file);
    let applied = io::apply_patch(&prepared);
    fs::remove_dir(&link).unwrap();
    assert!(read.is_err());
    assert!(applied.is_err());
    assert_eq!(
        fs::read(outside.join("modmain.lua")).unwrap(),
        prepared.original
    );
}
