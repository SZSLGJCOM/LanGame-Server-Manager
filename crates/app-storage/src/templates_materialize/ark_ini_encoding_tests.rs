use super::*;

const TEST_ADMIN_PASSWORD: &str = "ark-encoding-fixture-password";

#[derive(Clone, Copy, Debug)]
enum Encoding {
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
}

impl Encoding {
    fn encode(self, text: &str) -> Vec<u8> {
        match self {
            Self::Utf8 => text.as_bytes().to_vec(),
            Self::Utf8Bom => [b"\xef\xbb\xbf".as_slice(), text.as_bytes()].concat(),
            Self::Utf16Le => [0xfeff_u16]
                .into_iter()
                .chain(text.encode_utf16())
                .flat_map(u16::to_le_bytes)
                .collect(),
            Self::Utf16Be => [0xfeff_u16]
                .into_iter()
                .chain(text.encode_utf16())
                .flat_map(u16::to_be_bytes)
                .collect(),
        }
    }

    fn decode(self, bytes: &[u8]) -> String {
        let text = match self {
            Self::Utf8 => String::from_utf8(bytes.to_vec()).unwrap(),
            Self::Utf8Bom => {
                String::from_utf8(bytes.strip_prefix(b"\xef\xbb\xbf").unwrap().to_vec()).unwrap()
            }
            Self::Utf16Le | Self::Utf16Be => {
                let bom: &[u8] = if matches!(self, Self::Utf16Le) {
                    &[0xff, 0xfe]
                } else {
                    &[0xfe, 0xff]
                };
                let body = bytes.strip_prefix(bom).unwrap();
                let (pairs, remainder) = body.as_chunks::<2>();
                assert!(remainder.is_empty());
                let units = pairs
                    .iter()
                    .map(|pair| {
                        if matches!(self, Self::Utf16Le) {
                            u16::from_le_bytes(*pair)
                        } else {
                            u16::from_be_bytes(*pair)
                        }
                    })
                    .collect::<Vec<_>>();
                String::from_utf16(&units).unwrap()
            }
        };
        assert!(!text.starts_with('\u{feff}'), "duplicate BOM");
        assert!(
            self.encode(&text) == bytes,
            "encoding must round-trip exactly"
        );
        text
    }
}

fn assert_unchanged(label: &str, encoding: Encoding, before: &[u8], after: &[u8]) {
    if before == after {
        return;
    }
    let byte = before
        .iter()
        .zip(after)
        .position(|(left, right)| left != right)
        .unwrap_or(before.len().min(after.len()));
    let before_text = encoding.decode(before);
    let after_text = encoding.decode(after);
    let before_lines = before_text.lines().collect::<Vec<_>>();
    let after_lines = after_text.lines().collect::<Vec<_>>();
    let line = before_lines
        .iter()
        .zip(&after_lines)
        .position(|(left, right)| left != right)
        .unwrap_or(before_lines.len().min(after_lines.len()));
    let describe = |lines: &[&str]| match lines.get(line) {
        Some(text) => match text.split_once('=') {
            Some((key, _)) => format!(
                "assignment key {:?}",
                key.trim().chars().take(80).collect::<String>()
            ),
            None if text.trim().is_empty() => "blank line".into(),
            None if text.trim_start().starts_with('[') => "section header".into(),
            None => "comment or text".into(),
        },
        None => "<EOF>".into(),
    };
    panic!(
        "second materialization changed {label} ({encoding:?}): bytes {} -> {}, first difference at byte {byte}, line {}; before={}; after={}",
        before.len(),
        after.len(),
        line + 1,
        describe(&before_lines),
        describe(&after_lines)
    );
}

#[test]
fn ark_ini_native_encoded_rewrite_rematerializes_twice_without_changing_encoding() {
    for module_id in ["arksurvivalevolved", "arksurvivalascended"] {
        for (config_encoding, live_encoding, newline) in [
            (Encoding::Utf8, Encoding::Utf16Le, "\r\n"),
            (Encoding::Utf16Be, Encoding::Utf16Le, "\r\n"),
            (Encoding::Utf16Le, Encoding::Utf16Be, "\n"),
            (Encoding::Utf8Bom, Encoding::Utf8Bom, "\r\n"),
            (Encoding::Utf8, Encoding::Utf8, "\n"),
        ] {
            let fixture = Fixture::new(module_id);
            fixture
                .write(&json!({"server_name":"Before", "admin_password":TEST_ADMIN_PASSWORD}))
                .unwrap()
                .commit();
            for filename in ["Game.ini", "GameUserSettings.ini"] {
                let session = if filename == "GameUserSettings.ini" {
                    "[SessionSettings]\nSessionName=Before\n"
                } else {
                    ""
                };
                fs::write(
                    fixture.config.join(filename),
                    config_encoding.encode(&format!(
                        "; config comment\n{session}[Private.Mod]\nConfigOnly=配置🙂\nShared=config\n"
                    )),
                )
                .unwrap();
                let native = format!(
                    "; native comment\n{session}[Private.Mod]\nLiveOnly=原生🙂\nShared=live\nRule=one\nRule=two\n"
                )
                .replace('\n', newline);
                fs::write(fixture.live.join(filename), live_encoding.encode(&native)).unwrap();
            }
            // Real instances persist generated secrets. Pin this synthetic value
            // so repeated fixture rendering has the same complete managed input.
            let settings = json!({"server_name":"After", "admin_password":TEST_ADMIN_PASSWORD});
            fixture.write(&settings).unwrap().commit();
            let mut first = Vec::new();
            for filename in ["Game.ini", "GameUserSettings.ini"] {
                for (directory, encoding, expected_newline) in [
                    (&fixture.config, config_encoding, "\n"),
                    (&fixture.live, live_encoding, newline),
                ] {
                    let path = directory.join(filename);
                    let bytes = fs::read(&path).unwrap();
                    let text = encoding.decode(&bytes);
                    assert!(text.contains("ConfigOnly=配置🙂"));
                    if directory == &fixture.live {
                        assert!(text.contains("; native comment"));
                        assert!(text.contains("LiveOnly=原生🙂"));
                        assert!(text.contains("Shared=live"));
                        assert!(!text.contains("Shared=config"));
                        assert!(text.contains(&format!("Rule=one{newline}Rule=two")));
                    }
                    if filename == "GameUserSettings.ini" {
                        assert_eq!(text.matches("SessionName=After").count(), 1);
                        assert!(!text.contains("SessionName=Before"));
                    }
                    if expected_newline == "\r\n" {
                        assert!(!text.replace("\r\n", "").contains(['\r', '\n']));
                    } else {
                        assert!(!text.contains('\r'));
                    }
                    first.push((path, bytes, encoding));
                }
            }
            fixture.write(&settings).unwrap().commit();
            for (path, bytes, encoding) in first {
                assert_unchanged(
                    &format!(
                        "{module_id}/{}",
                        path.strip_prefix(&fixture.root).unwrap().display()
                    ),
                    encoding,
                    &bytes,
                    &fs::read(&path).unwrap(),
                );
            }
        }
    }
}

#[test]
fn ark_ini_invalid_encoding_preserves_native_and_rendered_configuration() {
    for malformed in [
        vec![0xff, 0xfe, b'['],
        vec![0xff, 0xfe, 0x00, 0xd8],
        vec![0xfe, 0xff, 0xd8, 0x00],
        vec![0x80],
        b"[\0M\0]\0\n\0K\0=\0V\0".to_vec(),
    ] {
        let fixture = Fixture::new("arksurvivalevolved");
        fixture
            .write(&json!({"server_name":"Before", "admin_password":TEST_ADMIN_PASSWORD}))
            .unwrap()
            .commit();
        let live_path = fixture.live.join("GameUserSettings.ini");
        fs::write(&live_path, &malformed).unwrap();
        let paths = [
            fixture.config.join("Game.ini"),
            fixture.config.join("GameUserSettings.ini"),
            fixture.config.join("instance.json"),
            fixture.config.join("ark-native-ownership.json"),
            fixture.live.join("Game.ini"),
            live_path,
        ];
        let before = paths
            .iter()
            .map(|path| fs::read(path).unwrap())
            .collect::<Vec<_>>();
        assert!(
            fixture
                .write(&json!({"server_name":"After", "admin_password":TEST_ADMIN_PASSWORD}))
                .is_err()
        );
        for (path, bytes) in paths.iter().zip(before) {
            assert!(
                fs::read(path).unwrap() == bytes,
                "invalid encoding must not replace bytes in {}",
                path.strip_prefix(&fixture.root).unwrap().display()
            );
        }
    }
}
