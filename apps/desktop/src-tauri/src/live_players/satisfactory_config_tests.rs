use super::*;

struct NativeFixture(std::path::PathBuf);

impl NativeFixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("langame-frm-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir(&root).expect("native configuration fixture");
        Self(root)
    }
}

impl Drop for NativeFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn native_configuration_reads_are_instance_scoped_and_bounded() {
    let fixture = NativeFixture::new();
    for (name, port) in [("first", 18080), ("second", 18081)] {
        let root = fixture.0.join(name);
        let config = root.join("config/instance.json");
        assert_eq!(
            read_web_port(config.to_str().expect("fixture path")),
            Ok(None)
        );
        let native = root.join("data/Saved/Config/WindowsServer/GameUserSettings.ini");
        std::fs::create_dir_all(native.parent().expect("native parent")).expect("native directory");
        std::fs::write(&native, native_options("1", &port.to_string())).expect("native options");
        assert_eq!(
            read_web_port(config.to_str().expect("fixture path")),
            Ok(Some(port))
        );
        std::fs::write(&native, [0xff, 0xfe]).expect("invalid encoding");
        assert!(read_web_port(config.to_str().expect("fixture path")).is_err());
        std::fs::write(&native, vec![b' '; MAX_CONFIG_BYTES as usize + 1])
            .expect("oversize options");
        assert!(read_web_port(config.to_str().expect("fixture path")).is_err());
    }
}

fn native_options(autostart: &str, port: &str) -> String {
    format!(
        "[{SETTINGS_SECTION}]\nmIntValues=((\"{AUTOSTART_KEY}\", {autostart}))\nmStringValues=((\"{PORT_KEY}\", \"{port}\"))\n"
    )
}

#[test]
fn native_api_written_options_select_the_configured_http_port() {
    assert_eq!(
        parse_web_port(&native_options("1", "18080")),
        Ok(Some(18080))
    );
    let config = format!(
        "\u{feff}[{SETTINGS_SECTION}]\r\nmIntValues=((\"{PORT_KEY}\",8081),(\"{AUTOSTART_KEY}\",1))\r\n"
    );
    assert_eq!(parse_web_port(&config), Ok(Some(8081)));
    let default_port = format!("[{SETTINGS_SECTION}]\nmIntValues=((\"{AUTOSTART_KEY}\",1))");
    assert_eq!(parse_web_port(&default_port), Ok(Some(8080)));
}

#[test]
fn disabled_or_absent_http_settings_keep_the_game_api_transport() {
    for config in ["", "[Other]\nmIntValues=()", "[Other]\nmIntValues=broken"] {
        assert_eq!(parse_web_port(config), Ok(None));
    }
    for disabled in ["0", "false", "False"] {
        assert_eq!(parse_web_port(&native_options(disabled, "18080")), Ok(None));
    }
}

#[test]
fn endpoint_options_in_comments_or_another_string_cannot_select_a_port() {
    let config = format!(
        "[{SETTINGS_SECTION}]\n;mIntValues=((\"{AUTOSTART_KEY}\",1))\nmStringValues=((\"Description\",\"mentions \\\"{AUTOSTART_KEY}\\\",1 and \\\"{PORT_KEY}\\\",1234\"))\n"
    );
    assert_eq!(parse_web_port(&config), Ok(None));
    assert_eq!(
        parse_web_port(&native_options("1", "18080").replace(SETTINGS_SECTION, "Other")),
        Ok(None)
    );
}

#[test]
fn duplicate_or_invalid_endpoint_options_are_not_silently_accepted() {
    for port in [
        "0",
        "65536",
        "-1",
        "1.5",
        "127.0.0.1:8080",
        "",
        " 8080",
        "+8080",
    ] {
        assert!(
            parse_web_port(&native_options("1", port)).is_err(),
            "{port}"
        );
    }
    assert!(parse_web_port(&native_options("2", "18080")).is_err());
    let duplicate = format!(
        "{}mIntValues=((\"{PORT_KEY}\",8080))\n",
        native_options("1", "18080")
    );
    assert!(parse_web_port(&duplicate).is_err());
    for malformed in [
        "",
        "(",
        "((\"key\",1)",
        "((\"key\",1),)",
        "((\"key\",1))garbage",
    ] {
        let config = format!("[{SETTINGS_SECTION}]\nmIntValues={malformed}");
        assert!(parse_web_port(&config).is_err(), "{malformed}");
    }
}

#[test]
fn scalar_maps_preserve_quoted_delimiters_without_treating_them_as_entries() {
    let pairs = scalar_pairs(r#"(("Name", "中文,(text)\"quoted\""), ("Count", 3))"#)
        .expect("native scalar map");
    assert_eq!(
        pairs,
        [("Name", r#"中文,(text)\"quoted\""#), ("Count", "3")]
    );
}
