use super::*;
use serde_json::json;

#[test]
fn dst_mod_error_names_preserves_native_log_order_and_deduplicates() {
    let lines = [
        "[00:00:09]: MOD ERROR: workshop-123456: failed initialization",
        "MOD ERROR: local addon.v1: failure: additional context",
        "[123:59:59]: MOD ERROR: workshop-123456: repeated failure",
        "[00:01:00]: MOD ERROR: 汉化 包.v1:",
    ]
    .map(String::from);
    assert_eq!(
        dst_mod_error_names(&lines),
        ["workshop-123456", "local addon.v1", "汉化 包.v1"]
    );
}

#[test]
fn dst_mod_error_names_ignores_display_names_and_their_path_like_content() {
    let lines = [
        "[00:00:09]: MOD ERROR: workshop-123456 (Display Name): failed initialization",
        "MOD ERROR: 汉化 包.v1 (Label: C:\\outside\\evil.lua ../other): failure",
        "MOD ERROR: local addon.v1 (Display (nested) Name): failed",
    ]
    .map(String::from);
    assert_eq!(
        dst_mod_error_names(&lines),
        ["workshop-123456", "汉化 包.v1", "local addon.v1"]
    );
}

#[test]
fn dst_mod_error_names_rejects_paths_streams_devices_and_oversized_names() {
    for name in [
        "..",
        ".",
        "../outside",
        "C:\\mods",
        "folder/name",
        "folder\\name",
        "name:stream",
        "name.",
        "name ",
        "workshop-12x",
        "",
        "CON",
        "nul.txt",
        "COM1",
        "LPT9.lua",
        "CONIN$",
        "COM¹",
        "bad\nname",
        "bad\tname",
        &"a".repeat(129),
    ] {
        for suffix in [": failure", " (Display Name): failure"] {
            let lines = [format!("MOD ERROR: {name}{suffix}")];
            assert!(dst_mod_error_names(&lines).is_empty(), "{name:?}{suffix}");
        }
    }
}

#[test]
fn dst_mod_error_names_requires_an_exact_native_marker_and_complete_header() {
    let lines = [
        "[00:00:09]: Could not find modworldgenmain.lua for local_mod, skipping.",
        "[00:00:09]: Mod: local_mod has no modworldgenmain.lua. Skipping.",
        "RemoteCommandInput: print('MOD ERROR: local_mod: forged')",
        "[player]: MOD ERROR: local_mod: forged",
        "[00:99:00]: MOD ERROR: local_mod: invalid timestamp",
        "MOD ERROR: local_mod",
        "MOD ERROR: local_mod (Unclosed: failure",
        "MOD ERROR: local_mod (Closed) extra: failure",
        "mod error: local_mod: failure",
        "prefix MOD ERROR: local_mod: failure",
    ]
    .map(String::from);
    assert!(dst_mod_error_names(&lines).is_empty());
}

#[test]
fn dst_mod_error_names_limits_candidates_to_the_first_five_unique_directories() {
    let lines = [
        "third", "first", "third", "second", "fourth", "fifth", "sixth",
    ]
    .map(|name| format!("MOD ERROR: {name}: failed"));
    assert_eq!(
        dst_mod_error_names(&lines),
        ["third", "first", "second", "fourth", "fifth"]
    );
}

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let base = std::env::var_os("LANGAME_ASSISTANT_TEST_WORK_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let path = base.join(format!(
            "dst-mod-evidence-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(path.join("install/mods")).unwrap();
        Self(path)
    }

    fn install(&self) -> PathBuf {
        self.0.join("install")
    }

    fn write_mod(&self, relative: &str, source: &str) -> PathBuf {
        let root = self.0.join(relative);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("modinfo.lua"), source).unwrap();
        fs::write(root.join("modmain.lua"), "error('modmain must never run')").unwrap();
        root
    }

    fn read(&self, names: &[&str], offset: usize) -> Result<DstInstalledModEvidencePage, String> {
        read_dst_installed_mod_evidence(
            &self.install(),
            &[],
            &names
                .iter()
                .map(|name| String::from(*name))
                .collect::<Vec<_>>(),
            offset,
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let parent = self.0.parent().unwrap().canonicalize().unwrap();
        let resolved = self.0.canonicalize().unwrap();
        assert_eq!(resolved.parent(), Some(parent.as_path()));
        let name = resolved.file_name().unwrap().to_str().unwrap();
        assert!(uuid::Uuid::parse_str(name.strip_prefix("dst-mod-evidence-").unwrap()).is_ok());
        fs::remove_dir_all(&resolved).expect("remove isolated Mod evidence fixture");
    }
}

#[test]
fn installed_mod_evidence_reads_local_metadata_and_preserves_dependency_candidates() {
    let fixture = Fixture::new();
    fixture.write_mod(
        "install/mods/local_library",
        r#"name='Local Library';version='1.2.3';api_version=10;api_version_dst=10;priority='10';
dst_compatible=true;dont_starve_together_compatible=true;client_only_mod=false;server_only_mod=true;
mod_dependencies={{['local_dep']=false,['Display Name']=true,workshop='workshop-123456'}}"#,
    );
    let page = fixture.read(&["local_library"], 0).unwrap();
    assert_eq!(page.total_matches, 1);
    assert_eq!(page.next_offset, None);
    let entry = &page.entries[0];
    assert_eq!(entry.status, "read");
    assert_eq!(entry.source.as_deref(), Some("install/mods/local_library"));
    assert_eq!(
        entry.files,
        json!({"modinfo": "present", "modmain": "present"})
    );
    assert_eq!(entry.metadata["version"], "1.2.3");
    assert_eq!(entry.metadata["api_version"], 10);
    assert_eq!(entry.metadata["api_version_dst"], 10);
    assert_eq!(entry.metadata["priority"], "10");
    assert_eq!(entry.metadata["dont_starve_together_compatible"], true);
    assert_eq!(entry.metadata["dst_compatible"], true);
    assert_eq!(entry.metadata["client_only_mod"], false);
    assert_eq!(entry.metadata["server_only_mod"], true);
    assert_eq!(
        entry.metadata["mod_dependencies"],
        json!([{"local_dep":false,"Display Name":true,"workshop":"workshop-123456"}])
    );
    assert!(
        !serde_json::to_string(&page)
            .unwrap()
            .contains(&fixture.0.to_string_lossy().to_string())
    );
}

#[test]
fn installed_mod_evidence_keeps_missing_files_and_parse_failures_explicit() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.install().join("mods/incomplete")).unwrap();
    fixture.write_mod(
        "install/mods/broken",
        "name='Partial';error('invalid metadata')",
    );
    let page = fixture
        .read(&["absent", "incomplete", "broken"], 0)
        .unwrap();
    for (name, status) in [
        ("absent", "missing"),
        ("incomplete", "missing_modinfo"),
        ("broken", "parse_error"),
    ] {
        let entry = page
            .entries
            .iter()
            .find(|entry| entry.folder_name == name)
            .unwrap();
        assert_eq!(entry.status, status);
        assert!(entry.metadata.is_null());
    }
    assert_eq!(page.entries[0].files["modinfo"], "missing");
}

#[test]
fn installed_mod_evidence_paginates_and_exact_queries_do_not_scan_every_mod() {
    let fixture = Fixture::new();
    fs::write(fixture.install().join("mods/modsettings.lua"), "return {}").unwrap();
    fs::write(
        fixture
            .install()
            .join("mods/dedicated_server_mods_setup.lua"),
        "return {}",
    )
    .unwrap();
    for index in 0..13 {
        fixture.write_mod(
            &format!("install/mods/local_{index:02}"),
            "name='Local';version='1'",
        );
    }
    let first = fixture.read(&[], 0).unwrap();
    assert_eq!(first.entries.len(), 5);
    assert_eq!(first.total_matches, 13);
    assert_eq!(first.next_offset, Some(5));
    let second = fixture.read(&[], 5).unwrap();
    assert_eq!(second.entries.len(), 5);
    assert_eq!(second.next_offset, Some(10));
    let last = fixture.read(&[], 10).unwrap();
    assert_eq!(last.entries.len(), 3);
    assert_eq!(last.next_offset, None);
    assert_eq!(last.entries[2].folder_name, "local_12");
    assert!(fixture.read(&[], usize::MAX).unwrap().entries.is_empty());
    assert_eq!(
        fixture.read(&["local_12"], 0).unwrap().entries[0].folder_name,
        "local_12"
    );
}

#[test]
fn installed_mod_evidence_supports_local_names_with_spaces_dots_and_unicode() {
    let fixture = Fixture::new();
    for name in ["local addon.v1", "本地模组"] {
        fixture.write_mod(&format!("install/mods/{name}"), "name='Local Addon'");
        let page = fixture.read(&[name], 0).unwrap();
        assert_eq!(page.entries[0].folder_name, name);
        assert_eq!(page.entries[0].status, "read");
    }
    assert_eq!(fixture.read(&[], 0).unwrap().entries.len(), 2);
}

#[test]
fn installed_mod_evidence_does_not_borrow_other_instances_ugc() {
    let fixture = Fixture::new();
    fixture.write_mod(
        "install/ugc_mods/other/Master/content/322330/123456",
        "name='Other instance'",
    );
    let absent = fixture.read(&["workshop-123456"], 0).unwrap();
    assert_eq!(absent.entries[0].status, "missing");
    fixture.write_mod(
        "instance/ugc/Master/content/322330/123456",
        "name='This instance';version='2'",
    );
    let page = read_dst_installed_mod_evidence(
        &fixture.install(),
        &[fixture.0.join("instance")],
        &[String::from("workshop-123456")],
        0,
    )
    .unwrap();
    assert_eq!(page.entries[0].metadata["name"], "This instance");
    assert!(
        page.entries[0]
            .source
            .as_ref()
            .unwrap()
            .contains("ugc/Master/content/322330/123456")
    );
}

#[test]
fn installed_mod_evidence_reports_each_copy_without_claiming_the_loaded_version() {
    let fixture = Fixture::new();
    fixture.write_mod("install/mods/workshop-123456", "version='1'");
    fixture.write_mod(
        "install/steamapps/workshop/content/322330/123456",
        "version='2'",
    );
    let page = fixture.read(&["workshop-123456"], 0).unwrap();
    assert_eq!(page.total_matches, 2);
    assert_eq!(page.entries.len(), 2);
    assert_ne!(page.entries[0].source, page.entries[1].source);
    assert_ne!(
        page.entries[0].metadata["version"],
        page.entries[1].metadata["version"]
    );
}

#[test]
fn installed_mod_evidence_rejects_paths_and_excessive_query_count() {
    let fixture = Fixture::new();
    for name in [
        "../escape",
        "..",
        ".",
        "C:\\mods",
        "folder/name",
        "folder\\name",
        "name:stream",
        "name.",
        "name ",
        "workshop-12x",
        "",
        "CON",
        "nul.txt",
        "COM1",
        "LPT9.lua",
    ] {
        let error = fixture.read(&[name], 0).unwrap_err();
        assert!(error.contains("directory name"), "{name}: {error}");
    }
    let error = fixture.read(&["valid"; 11], 0).unwrap_err();
    assert!(error.contains("10"));
}

#[test]
fn installed_mod_evidence_bounds_scan_without_blocking_exact_queries() {
    let fixture = Fixture::new();
    for index in 0..513 {
        fs::create_dir(fixture.install().join(format!("mods/local_{index:04}"))).unwrap();
    }
    assert!(fixture.read(&[], 0).unwrap_err().contains("scan limit"));
    let exact = fixture.read(&["local_0512"], 0).unwrap();
    assert_eq!(exact.entries[0].status, "missing_modinfo");
}

#[test]
fn installed_mod_evidence_bounds_lua_file_execution_and_response() {
    let fixture = Fixture::new();
    for (name, source) in [
        ("looping", String::from("while true do end")),
        ("large_file", " ".repeat(MAX_MODINFO_BYTES + 1)),
        ("large_value", String::from("name=string.rep('x',8192)")),
        (
            "cyclic",
            String::from("mod_dependencies={};mod_dependencies[1]=mod_dependencies"),
        ),
        (
            "allocation",
            String::from("name=string.rep('x',64*1024*1024)"),
        ),
    ] {
        fixture.write_mod(&format!("install/mods/{name}"), &source);
        let page = fixture.read(&[name], 0).unwrap();
        assert_ne!(page.entries[0].status, "read", "{name}");
        assert!(page.entries[0].message.is_some());
        assert!(serde_json::to_vec(&page).unwrap().len() <= 10 * 1024);
    }
}

#[test]
fn installed_mod_evidence_reuses_safe_modinfo_helpers_but_never_modmain() {
    let fixture = Fixture::new();
    let root = fixture.write_mod(
        "install/mods/importing",
        "modimport('metadata');name=helper_name",
    );
    fs::write(root.join("metadata.lua"), "helper_name='Imported Metadata'").unwrap();
    assert_eq!(
        fixture.read(&["importing"], 0).unwrap().entries[0].metadata["name"],
        "Imported Metadata"
    );
    fs::write(
        root.join("modinfo.lua"),
        "modimport('modmain');name='Must not reach'",
    )
    .unwrap();
    let page = fixture.read(&["importing"], 0).unwrap();
    assert_eq!(page.entries[0].status, "parse_error");
    assert!(
        page.entries[0]
            .message
            .as_ref()
            .unwrap()
            .contains("modmain")
    );
    fs::write(
        root.join("modinfo.lua"),
        "assert(os == nil and io == nil and package == nil);name='Sandboxed'",
    )
    .unwrap();
    assert_eq!(
        fixture.read(&["importing"], 0).unwrap().entries[0].metadata["name"],
        "Sandboxed"
    );
}

#[test]
fn installed_mod_evidence_rejects_native_pattern_matchers_without_executing_them() {
    let fixture = Fixture::new();
    for function in ["match", "find", "gmatch", "gsub"] {
        fixture.write_mod(
            "install/mods/patterns",
            &format!("name=string.{function}('abc','a','z')"),
        );
        let page = fixture.read(&["patterns"], 0).unwrap();
        assert_eq!(page.entries[0].status, "parse_error");
        assert!(
            page.entries[0]
                .message
                .as_ref()
                .unwrap()
                .contains("pattern matching is disabled")
        );
    }
}

#[cfg(windows)]
fn create_junction(link: &Path, target: &Path) {
    use std::os::windows::process::CommandExt;
    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", "$ErrorActionPreference='Stop'; New-Item -ItemType Junction -Path $env:LGSM_EVIDENCE_TEST_LINK -Target $env:LGSM_EVIDENCE_TEST_TARGET | Out-Null"])
        .env("LGSM_EVIDENCE_TEST_LINK", link)
        .env("LGSM_EVIDENCE_TEST_TARGET", target)
        .creation_flags(0x08000000)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "junction creation failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(windows)]
#[test]
fn installed_mod_evidence_rejects_junction_ancestors_and_imports() {
    let fixture = Fixture::new();
    let outside = fixture.write_mod("outside", "name='Outside Mod'");
    let junction = fixture.install().join("mods/linked");
    create_junction(&junction, &outside);
    assert!(
        fixture
            .read(&["linked"], 0)
            .unwrap_err()
            .contains("reparse")
    );
    let root = fixture.write_mod("install/mods/importing", "modimport('linked/modinfo')");
    let import_link = root.join("linked");
    create_junction(&import_link, &outside);
    let page = fixture.read(&["importing"], 0).unwrap();
    assert_eq!(page.entries[0].status, "parse_error");
    assert!(
        page.entries[0]
            .message
            .as_ref()
            .unwrap()
            .contains("reparse")
    );
    fs::remove_dir(&import_link).unwrap();
    fs::remove_dir(&junction).unwrap();
}
