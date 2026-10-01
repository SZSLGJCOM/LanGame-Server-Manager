use super::*;

struct ConfigFixture(PathBuf);

impl ConfigFixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("lgsm-config-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).expect("create isolated config fixture");
        Self(root)
    }
}

impl Drop for ConfigFixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("remove owned config fixture");
    }
}

#[test]
fn config_documents_include_native_lua_and_xml_settings() {
    let fixture = ConfigFixture::new();
    fs::write(
        fixture.0.join("modoverrides.lua"),
        "return { example = true }",
    )
    .unwrap();
    fs::write(fixture.0.join("serverconfig.xml"), "<ServerSettings />").unwrap();
    fs::write(fixture.0.join("runtime.log"), "not a configuration file").unwrap();

    let documents = read_assistant_instance_config_documents(&fixture.0.to_string_lossy()).unwrap();
    let names = documents
        .iter()
        .map(|document| {
            Path::new(&document.path)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect::<Vec<_>>();
    assert_eq!(names, ["modoverrides.lua", "serverconfig.xml"]);
}

#[test]
fn config_file_listing_pages_relative_paths_in_stable_order() {
    let fixture = ConfigFixture::new();
    fs::create_dir(fixture.0.join("Master")).unwrap();
    fs::write(fixture.0.join("Master/modoverrides.lua"), "return {}").unwrap();
    fs::write(fixture.0.join("cluster.ini"), "[NETWORK]").unwrap();
    fs::write(fixture.0.join("serverconfig.xml"), "<ServerSettings />").unwrap();
    let root = fixture.0.to_string_lossy();

    let first = list_assistant_instance_config_files(&root, 0, 2).unwrap();
    assert_eq!(first.files, ["Master/modoverrides.lua", "cluster.ini"]);
    assert_eq!(first.next_offset, Some(2));
    assert!(!first.scan_truncated);
    let second = list_assistant_instance_config_files(&root, 2, 2).unwrap();
    assert_eq!(second.files, ["serverconfig.xml"]);
    assert_eq!(second.next_offset, None);
}

#[test]
fn config_file_pages_redact_before_slicing_and_preserve_utf8() {
    let fixture = ConfigFixture::new();
    let credential = ["private", "page", "secret"].join("-");
    fs::write(
        fixture.0.join("server.ini"),
        format!("server_name=配置\nserver_password={credential}\nmax_players=8\n"),
    )
    .unwrap();
    let root = fixture.0.to_string_lossy();
    let mut offset = 0;
    let mut combined = String::new();
    loop {
        let page = read_assistant_instance_config_file(&root, "server.ini", offset, 7).unwrap();
        assert_eq!(page.path, "server.ini");
        assert_eq!(page.offset_bytes, offset);
        assert!(page.content.len() <= 7);
        combined.push_str(&page.content);
        match page.next_offset_bytes {
            Some(next) => {
                assert!(next > offset);
                offset = next;
            }
            None => break,
        }
    }
    assert!(combined.contains("配置"));
    assert!(combined.contains("[REDACTED]"));
    assert!(!combined.contains(&credential));
    assert!(combined.contains("max_players=8"));
}

#[test]
fn config_file_reader_decodes_utf16_bom_before_redaction_and_pagination() {
    let credential = ["private", "utf16", "secret"].join("-");
    let text = format!("server_name=配置🎮\nserver_password={credential}\nmax_players=8\n");
    for little_endian in [true, false] {
        let fixture = ConfigFixture::new();
        let mut bytes = if little_endian {
            vec![0xff, 0xfe]
        } else {
            vec![0xfe, 0xff]
        };
        for unit in text.encode_utf16() {
            bytes.extend(if little_endian {
                unit.to_le_bytes()
            } else {
                unit.to_be_bytes()
            });
        }
        fs::write(fixture.0.join("server.ini"), bytes).unwrap();
        let root = fixture.0.to_string_lossy();
        let mut combined = String::new();
        let mut offset = 0;
        loop {
            let page = read_assistant_instance_config_file(&root, "server.ini", offset, 7).unwrap();
            assert!(page.content.len() <= 7);
            combined.push_str(&page.content);
            match page.next_offset_bytes {
                Some(next) => {
                    assert!(next > offset);
                    offset = next;
                }
                None => break,
            }
        }
        assert!(combined.contains("server_name=配置🎮"));
        assert!(combined.contains("max_players=8"));
        assert!(combined.contains("[REDACTED]"));
        assert!(!combined.contains(&credential));
        assert!(!combined.contains(['\0', '\u{fffd}', '\u{feff}']));
        let documents = read_assistant_instance_config_documents(&root).unwrap();
        assert_eq!(documents.len(), 1);
        assert_eq!(documents[0].content, combined);
    }
}

#[test]
fn config_file_reader_rejects_ambiguous_or_invalid_text_encodings() {
    let text = concat!("server_password=", "synthetic-fixture\n");
    let utf16_le_without_bom = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let utf16_be_without_bom = text.encode_utf16().flat_map(u16::to_be_bytes).collect();
    for bytes in [
        utf16_le_without_bom,
        utf16_be_without_bom,
        vec![0xff, 0xfe, 0x61],
        vec![0xfe, 0xff, 0x61],
        vec![0xff, 0xfe, 0x00, 0xd8],
        vec![0xfe, 0xff, 0xd8, 0x00],
        vec![0xff, 0xfe, 0x00, 0x00],
        vec![0xfe, 0xff, 0x00, 0x00],
        vec![0x61, 0xff],
        b"key=value\0other=value".to_vec(),
    ] {
        let fixture = ConfigFixture::new();
        fs::write(fixture.0.join("server.ini"), bytes).unwrap();
        let root = fixture.0.to_string_lossy();
        let error = read_assistant_instance_config_file(&root, "server.ini", 0, 80).unwrap_err();
        assert!(error.contains("encoding") || error.contains("NUL"));
        assert!(read_assistant_instance_config_documents(&root).is_err());
    }
}

#[test]
fn config_file_reader_accepts_utf8_bom_without_exposing_it() {
    let fixture = ConfigFixture::new();
    fs::write(fixture.0.join("server.ini"), "\u{feff}max_players=8\n").unwrap();
    let page =
        read_assistant_instance_config_file(&fixture.0.to_string_lossy(), "server.ini", 0, 80)
            .unwrap();
    assert_eq!(page.content, "max_players=8\n");
}

#[test]
fn config_file_reader_redacts_secret_only_files_using_their_filename() {
    let fixture = ConfigFixture::new();
    fs::write(
        fixture.0.join("cluster_token.txt"),
        "private-cluster-credential",
    )
    .unwrap();
    let page = read_assistant_instance_config_file(
        &fixture.0.to_string_lossy(),
        "cluster_token.txt",
        0,
        80,
    )
    .unwrap();
    assert_eq!(page.content, "[REDACTED]");
    let documents = read_assistant_instance_config_documents(&fixture.0.to_string_lossy()).unwrap();
    assert_eq!(documents[0].content, "[REDACTED]");
}

#[test]
fn config_file_reader_rejects_non_relative_and_stream_paths() {
    let fixture = ConfigFixture::new();
    fs::write(fixture.0.join("server.ini"), "max_players=8").unwrap();
    let root = fixture.0.to_string_lossy();
    for relative_path in [
        "../server.ini",
        "sub/../server.ini",
        "sub\\..\\server.ini",
        "/server.ini",
        "C:\\server.ini",
        "C:server.ini",
        "server.ini:private",
        "\\\\host\\server.ini",
        "./server.ini",
        "server.ini.",
        "server.ini ",
        "",
    ] {
        assert!(
            read_assistant_instance_config_file(&root, relative_path, 0, 80).is_err(),
            "unsafe path: {relative_path}"
        );
    }
}

#[test]
fn config_file_reader_refuses_oversized_files_before_allocating_contents() {
    let fixture = ConfigFixture::new();
    let path = fixture.0.join("oversized.ini");
    let file = fs::File::create(path).unwrap();
    file.set_len(8 * 1024 * 1024).unwrap();
    drop(file);
    let error =
        read_assistant_instance_config_file(&fixture.0.to_string_lossy(), "oversized.ini", 0, 80)
            .unwrap_err();
    assert!(error.contains("size limit"));
}

#[test]
fn config_file_listing_reports_depth_limit_without_following_unbounded_tree() {
    let fixture = ConfigFixture::new();
    let mut directory = fixture.0.clone();
    for _ in 0..20 {
        directory = directory.join("n");
        fs::create_dir(&directory).unwrap();
    }
    fs::write(directory.join("deep.ini"), "value=true").unwrap();
    let page = list_assistant_instance_config_files(&fixture.0.to_string_lossy(), 0, 10).unwrap();
    assert!(page.files.is_empty());
    assert!(page.scan_truncated);
}

#[cfg(windows)]
#[test]
fn config_file_reader_rejects_child_junction_and_keeps_listing_inside_root() {
    let fixture = ConfigFixture::new();
    let config = fixture.0.join("config");
    let external = fixture.0.join("external");
    let linked = config.join("linked");
    fs::create_dir(&config).unwrap();
    fs::create_dir(&external).unwrap();
    fs::write(external.join("private.ini"), "private-data").unwrap();
    fs::write(config.join("server.ini"), "max_players=8").unwrap();
    let output = std::process::Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(&linked)
        .arg(&external)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "create junction fixture: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let read_result =
        read_assistant_instance_config_file(&config.to_string_lossy(), "linked/private.ini", 0, 80);
    let listing = list_assistant_instance_config_files(&config.to_string_lossy(), 0, 10);
    fs::remove_dir(&linked).unwrap();
    assert!(read_result.is_err());
    let listing = listing.unwrap();
    assert_eq!(listing.files, ["server.ini"]);
    assert!(listing.scan_truncated);
}

#[cfg(windows)]
#[test]
fn config_documents_reject_root_junction_to_unrelated_files() {
    let fixture = ConfigFixture::new();
    let external = fixture.0.join("external");
    let linked = fixture.0.join("linked");
    fs::create_dir(&external).unwrap();
    fs::write(
        external.join("private.ini"),
        "PrivateValue=outside-instance",
    )
    .unwrap();
    let output = std::process::Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(&linked)
        .arg(&external)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "create junction fixture: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let result = read_assistant_instance_config_documents(&linked.to_string_lossy());
    fs::remove_dir(&linked).unwrap();
    assert!(
        result.is_err(),
        "linked config root must not read external files"
    );
}
