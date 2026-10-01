use super::*;

struct Fixture(InstanceWorkspace);

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("lgsm-workspace-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        Self(InstanceWorkspace { root })
    }

    fn write(&self, file: &str, content: &[u8]) {
        let path = self.0.root.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0.root);
    }
}

#[test]
fn workspace_lists_cross_game_text_with_explicit_edit_permissions() {
    let fixture = Fixture::new();
    for file in [
        "config/server.properties",
        "runtime/oxide/plugins/Example.cs",
        "data/plugins/custom/config.yml",
        "data/scripts/maintenance.lua",
        "runtime/BepInEx/plugins/Example/plugin.json",
    ] {
        fixture.write(file, b"setting = true");
    }
    fixture.write("runtime/.langame-private-runtime", b"managed\n");
    fixture.write("config/cluster_token.txt", b"synthetic-test-only");
    fixture.write("data/.langame/file-patches/old/original.txt", b"retained");
    let page = fixture.0.list_files("", 0).unwrap();
    assert_eq!(page.files.len(), 5);
    assert!(!page.scan_truncated);
    for entry in &page.files {
        assert_eq!(
            entry.editable,
            entry.file != "config/server.properties",
            "{}",
            entry.file
        );
    }
    assert!(
        page.files
            .iter()
            .all(|entry| !entry.file.contains("token") && !entry.file.contains(".langame"))
    );
    let document = fixture
        .0
        .read_file("runtime/oxide/plugins/Example.cs")
        .unwrap();
    assert_eq!(document.content, "setting = true");
    assert_eq!(document.source_sha256, sha256(b"setting = true"));
}

#[test]
fn workspace_refuses_escape_credentials_binary_and_manager_private_paths() {
    let fixture = Fixture::new();
    for file in [
        "../outside.txt",
        "/absolute.txt",
        "config/a.txt:stream",
        "config\\a.txt",
        "config/CON.txt",
        "config/cluster_token.txt",
        "data/.langame/state.txt",
        "backups/save.txt",
        "runtime/server.exe",
    ] {
        assert!(fixture.0.read_file(file).is_err(), "accepted {file}");
    }
    for directory in ["../outside", "config/..", "data/.langame", "backups"] {
        assert!(
            fixture.0.list_files(directory, 0).is_err(),
            "accepted {directory}"
        );
    }
    fixture.write("config/invalid.txt", b"hello\0world");
    assert!(fixture.0.read_file("config/invalid.txt").is_err());
}

#[test]
fn workspace_stops_at_budget_and_can_narrow_a_directory() {
    let fixture = Fixture::new();
    for index in 0..70 {
        fixture.write(&format!("data/scripts/file-{index:03}.lua"), b"return true");
    }
    let first = fixture.0.list_files("data/scripts", 0).unwrap();
    assert_eq!(first.files.len(), PAGE_SIZE);
    let second = fixture
        .0
        .list_files("data/scripts", first.next_offset.unwrap())
        .unwrap();
    assert_eq!(second.files.len(), 6);
    assert!(second.next_offset.is_none());
    assert_ne!(first.files[0].file, second.files[0].file);
    fixture.write(
        "data/scripts/oversized.lua",
        &vec![b'x'; MAX_FILE_BYTES + 1],
    );
    assert!(fixture.0.read_file("data/scripts/oversized.lua").is_err());
    assert!(fixture.0.list_files("", MAX_SCAN_ENTRIES + 1).is_err());
}

#[test]
fn workspace_remaining_read_budget_is_enforced_on_current_file_bytes() {
    let fixture = Fixture::new();
    fixture.write("data/scripts/changed.lua", b"");
    assert_eq!(
        fixture.0.list_files("data/scripts", 0).unwrap().files[0].byte_length,
        0
    );
    fixture.write("data/scripts/changed.lua", b"return true");
    assert!(
        fixture
            .0
            .read_file_with_limit("data/scripts/changed.lua", 10)
            .is_err()
    );
    assert_eq!(
        fixture
            .0
            .read_file_with_limit("data/scripts/changed.lua", 11)
            .unwrap()
            .content,
        "return true"
    );
}

#[test]
fn workspace_directory_page_reports_omitted_directories() {
    let fixture = Fixture::new();
    for index in 0..=PAGE_SIZE {
        fs::create_dir(fixture.0.root.join(format!("directory-{index}"))).unwrap();
    }
    let result = fixture.0.list_files("", 0).unwrap();
    assert_eq!(result.directories.len(), PAGE_SIZE);
    assert!(!result.scan_truncated);
    assert!(result.files.is_empty());
    assert_eq!(result.next_directory_offset, Some(PAGE_SIZE));
    assert_eq!(result.next_file_offset, None);
    let second = fixture
        .0
        .list_files("", result.next_offset.unwrap())
        .unwrap();
    assert_eq!(second.directories.len(), 1);
    assert!(!result.directories.contains(&second.directories[0]));
    assert_eq!(second.listing_sha256, result.listing_sha256);
    assert!(second.next_offset.is_none());
}

#[test]
fn workspace_long_paths_page_both_streams_without_losing_entries() {
    let fixture = Fixture::new();
    let mut expected = std::collections::BTreeSet::new();
    for index in 0..80 {
        let file = format!("directory-{index:03}-{}/plugin.lua", "x".repeat(170));
        expected.insert(file.clone());
        fixture.write(&file, b"return true");
    }
    fixture.write("credentials/hidden.txt", b"synthetic-test-only");
    let mut files = std::collections::BTreeSet::new();
    let mut directories = std::collections::BTreeSet::new();
    let mut offset = 0;
    let first_hash = fixture.0.list_files("", offset).unwrap().listing_sha256;
    for _ in 0..160 {
        let page = fixture.0.list_files("", offset).unwrap();
        assert_eq!(page.listing_sha256, first_hash);
        assert!(!page.scan_truncated);
        assert!(serde_json::to_vec(&page).unwrap().len() < 9 * 1024);
        files.extend(page.files.into_iter().map(|entry| entry.file));
        directories.extend(page.directories);
        match page.next_offset {
            Some(next) => {
                assert!(next > offset);
                offset = next;
            }
            None => break,
        }
    }
    assert_eq!(files, expected);
    assert_eq!(directories.len(), 80);
    assert!(directories.iter().all(|path| !path.contains("credentials")));
    fixture.write("directory-added/new.lua", b"return true");
    assert_ne!(
        fixture.0.list_files("", 0).unwrap().listing_sha256,
        first_hash
    );
}

#[test]
fn workspace_listing_identity_binds_instance_directory_and_file_metadata() {
    let first = Fixture::new();
    let second = Fixture::new();
    for fixture in [&first, &second] {
        fixture.write("data/scripts/example.lua", b"return true");
    }
    let original = first.0.list_files("", 0).unwrap().listing_sha256;
    assert_ne!(original, second.0.list_files("", 0).unwrap().listing_sha256);
    assert_ne!(
        original,
        first
            .0
            .list_files("data/scripts", 0)
            .unwrap()
            .listing_sha256
    );
    first.write("data/scripts/example.lua", b"return false");
    assert_ne!(original, first.0.list_files("", 0).unwrap().listing_sha256);
}

#[test]
fn workspace_runtime_write_permission_requires_the_private_marker() {
    let fixture = Fixture::new();
    fixture.write("runtime/plugins/Plugin.cs", b"class Plugin {}");
    assert!(
        !fixture
            .0
            .read_file("runtime/plugins/Plugin.cs")
            .unwrap()
            .entry
            .editable
    );
    fixture.write("runtime/.langame-private-runtime", b"not managed");
    assert!(
        !fixture
            .0
            .read_file("runtime/plugins/Plugin.cs")
            .unwrap()
            .entry
            .editable
    );
    fixture.write("runtime/.langame-private-runtime", b"managed\n");
    assert!(
        fixture
            .0
            .read_file("runtime/plugins/Plugin.cs")
            .unwrap()
            .entry
            .editable
    );
    for file in [
        "runtime/Game/Saved/Config/server.ini",
        "data/saves/world.json",
        "runtime/mods/modoverrides.lua",
        "data/scripts/secret.txt",
    ] {
        assert!(edit_protection(file).is_some(), "accepted {file}");
    }
}

#[test]
fn workspace_all_four_dst_ugc_shards_allow_mod_sources_and_protect_world_configuration() {
    let fixture = Fixture::new();
    for shard in ["Master", "Caves", "Islands", "Volcano"] {
        let file = format!("data/ugc/{shard}/content/322330/1467214795/modmain.lua");
        fixture.write(&file, b"return true");
        assert!(
            fixture.0.read_file(&file).unwrap().entry.editable,
            "{shard}"
        );
        assert!(
            edit_protection(&format!(
                "data/ugc/{shard}/content/322330/1467214795/modoverrides.lua"
            ))
            .is_some()
        );
        assert!(edit_protection(&format!("config/clusters/main/{shard}/server.ini")).is_some());
    }
    assert!(edit_protection("data/ugc/Moon/content/322330/1467214795/modmain.lua").is_some());
}

#[cfg(unix)]
#[test]
fn workspace_does_not_follow_links_out_of_the_instance() {
    let fixture = Fixture::new();
    let outside = Fixture::new();
    outside.write("private.txt", b"outside");
    std::os::unix::fs::symlink(&outside.0.root, fixture.0.root.join("redirect")).unwrap();
    assert!(fixture.0.read_file("redirect/private.txt").is_err());
    assert!(fixture.0.list_files("redirect", 0).is_err());
    let page = fixture.0.list_files("", 0).unwrap();
    assert!(page.files.is_empty());
    assert!(page.scan_truncated);
}

#[cfg(windows)]
#[test]
fn workspace_case_variants_cannot_bypass_private_runtime_ownership() {
    let fixture = Fixture::new();
    fixture.write("runtime/plugins/Plugin.cs", b"class Plugin {}");
    assert!(
        !fixture
            .0
            .read_file("RUNTIME/plugins/Plugin.cs")
            .unwrap()
            .entry
            .editable
    );
}

#[cfg(windows)]
#[test]
fn workspace_junctions_are_not_read_or_traversed() {
    use std::os::windows::process::CommandExt;
    let fixture = Fixture::new();
    let outside = Fixture::new();
    outside.write("private.txt", b"outside");
    let link = fixture.0.root.join("redirect");
    let status = std::process::Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(link.to_string_lossy().replace('/', "\\"))
        .arg(outside.0.root.to_string_lossy().replace('/', "\\"))
        .creation_flags(0x0800_0000)
        .output()
        .unwrap();
    assert!(status.status.success(), "junction fixture failed");
    let read = fixture.0.read_file("redirect/private.txt");
    let direct = fixture.0.list_files("redirect", 0);
    let root = fixture.0.list_files("", 0);
    fs::remove_dir(&link).unwrap();
    assert!(read.is_err());
    assert!(direct.is_err());
    let root = root.unwrap();
    assert!(root.files.is_empty());
    assert!(root.scan_truncated);
    assert_eq!(
        fs::read(outside.0.root.join("private.txt")).unwrap(),
        b"outside"
    );
}
