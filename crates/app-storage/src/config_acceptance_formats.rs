use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Map, Value};

use super::config_acceptance_support::{
    ExpectedConfigEntry, ExpectedConfigFile, ExpectedOutputRoot,
};

pub(super) fn verify_expected_file(
    expected: &ExpectedConfigFile,
    output_roots: &AcceptanceOutputRoots<'_>,
    fixture_path: &Path,
) -> Result<(), String> {
    let root = match expected.root {
        ExpectedOutputRoot::Config => output_roots.config,
        ExpectedOutputRoot::Install => output_roots.install,
        ExpectedOutputRoot::Saves => output_roots.saves,
        ExpectedOutputRoot::Instance => output_roots.instance,
    };
    let candidate = root.join(&expected.path);
    if !candidate.is_file() {
        return Err(format!(
            "expected {:?} output {} was not generated for fixture {}",
            expected.root,
            expected.path.display(),
            fixture_path.display()
        ));
    }

    let contents = std::fs::read_to_string(&candidate).map_err(|source| {
        format!(
            "failed to read generated output {}: {source}",
            candidate.display()
        )
    })?;
    if let Some(keys) = &expected.keys {
        verify_parsed_keys(&expected.format, &contents, keys, &candidate, fixture_path)?;
    }
    if let Some(entries) = &expected.entries {
        verify_ordered_entries(
            &expected.format,
            &contents,
            entries,
            &candidate,
            fixture_path,
        )?;
    }
    if let Some(fragments) = &expected.fragments {
        for fragment in fragments {
            if !contents.contains(fragment) {
                return Err(format!(
                    "generated output {} is missing expected fragment {:?} from fixture {}",
                    candidate.display(),
                    fragment,
                    fixture_path.display()
                ));
            }
        }
    }

    Ok(())
}

pub(super) struct AcceptanceOutputRoots<'a> {
    pub(super) config: &'a Path,
    pub(super) install: &'a Path,
    pub(super) saves: &'a Path,
    pub(super) instance: &'a Path,
}

fn verify_parsed_keys(
    format: &str,
    contents: &str,
    expected_keys: &Map<String, Value>,
    output_path: &Path,
    fixture_path: &Path,
) -> Result<(), String> {
    match format.trim().to_ascii_lowercase().as_str() {
        "json" => verify_json_keys(contents, expected_keys, output_path, fixture_path),
        "properties" => verify_unique_text_keys(
            parse_properties(contents),
            expected_keys,
            output_path,
            fixture_path,
        ),
        "ini" => verify_unique_text_keys(
            parse_ini(contents),
            expected_keys,
            output_path,
            fixture_path,
        ),
        unsupported => Err(format!(
            "parsed key verification is not implemented for format {unsupported:?} at {}",
            fixture_path.display()
        )),
    }
}

fn verify_ordered_entries(
    format: &str,
    contents: &str,
    expected: &[ExpectedConfigEntry],
    output_path: &Path,
    fixture_path: &Path,
) -> Result<(), String> {
    let actual = match format.trim().to_ascii_lowercase().as_str() {
        "properties" => parse_properties(contents),
        "ini" => parse_ini(contents),
        unsupported => {
            return Err(format!(
                "ordered entry verification is not implemented for format {unsupported:?} at {}",
                fixture_path.display()
            ));
        }
    };
    let expected = expected
        .iter()
        .map(|entry| (entry.key.clone(), text_value(&entry.value)))
        .collect::<Vec<_>>();
    if actual != expected {
        return Err(format!(
            "generated output {} has ordered entries {:?}, expected {:?} from fixture {}",
            output_path.display(),
            actual,
            expected,
            fixture_path.display()
        ));
    }
    Ok(())
}

fn verify_json_keys(
    contents: &str,
    keys: &Map<String, Value>,
    output_path: &Path,
    fixture_path: &Path,
) -> Result<(), String> {
    let document = serde_json::from_str::<Value>(contents).map_err(|source| {
        format!(
            "generated output {} is not valid JSON for fixture {}: {source}",
            output_path.display(),
            fixture_path.display()
        )
    })?;

    for (key, expected_value) in keys {
        let actual_value = document
            .as_object()
            .and_then(|object| object.get(key))
            .or_else(|| resolve_json_path(&document, key));
        if actual_value != Some(expected_value) {
            return Err(expected_key_mismatch(
                output_path,
                key,
                actual_value.map(Value::to_string).as_deref(),
                &expected_value.to_string(),
                fixture_path,
            ));
        }
    }

    Ok(())
}

fn verify_unique_text_keys(
    entries: Vec<(String, String)>,
    keys: &Map<String, Value>,
    output_path: &Path,
    fixture_path: &Path,
) -> Result<(), String> {
    let mut actual = BTreeMap::new();
    for (key, value) in entries {
        if actual.insert(key.clone(), value).is_some() {
            return Err(format!(
                "generated output {} repeats key {key:?}; fixture {} must use ordered entries",
                output_path.display(),
                fixture_path.display()
            ));
        }
    }
    for (key, expected_value) in keys {
        let expected_value = text_value(expected_value);
        let actual_value = actual.get(key).map(String::as_str);
        if actual_value != Some(expected_value.as_str()) {
            return Err(expected_key_mismatch(
                output_path,
                key,
                actual_value,
                &expected_value,
                fixture_path,
            ));
        }
    }
    Ok(())
}

fn expected_key_mismatch(
    output_path: &Path,
    key: &str,
    actual: Option<&str>,
    expected: &str,
    fixture_path: &Path,
) -> String {
    format!(
        "generated output {} has {:?} for key {key:?}, expected {:?} from fixture {}",
        output_path.display(),
        actual,
        expected,
        fixture_path.display()
    )
}

fn resolve_json_path<'a>(document: &'a Value, path: &str) -> Option<&'a Value> {
    if path.starts_with('/') {
        return document.pointer(path);
    }
    path.split('.')
        .try_fold(document, |current, segment| current.get(segment))
}

fn text_value(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => value.clone(),
        value => value.to_string(),
    }
}

fn parse_properties(contents: &str) -> Vec<(String, String)> {
    contents
        .lines()
        .filter_map(|line| {
            let line = line.trim_start();
            if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
                return None;
            }
            split_property(line)
                .map(|(key, value)| (unescape_property_key(key), value.trim().into()))
        })
        .collect()
}

fn split_property(line: &str) -> Option<(&str, &str)> {
    let mut escaped = false;
    for (index, character) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
            continue;
        }
        if character == '=' || character == ':' || character.is_whitespace() {
            let value_start = property_value_start(line, index, character.len_utf8());
            return Some((line[..index].trim(), &line[value_start..]));
        }
    }

    Some((line.trim(), ""))
}

fn property_value_start(line: &str, separator_index: usize, separator_len: usize) -> usize {
    let mut cursor = separator_index + separator_len;
    cursor = skip_whitespace(line, cursor);
    if line[cursor..]
        .chars()
        .next()
        .is_some_and(|character| character == '=' || character == ':')
    {
        cursor += 1;
        cursor = skip_whitespace(line, cursor);
    }
    cursor
}

fn skip_whitespace(text: &str, mut cursor: usize) -> usize {
    while let Some(character) = text[cursor..].chars().next() {
        if !character.is_whitespace() {
            break;
        }
        cursor += character.len_utf8();
    }
    cursor
}

fn unescape_property_key(key: &str) -> String {
    let mut result = String::with_capacity(key.len());
    let mut characters = key.chars();
    while let Some(character) = characters.next() {
        if character == '\\' {
            if let Some(escaped) = characters.next() {
                result.push(escaped);
            }
        } else {
            result.push(character);
        }
    }
    result
}

fn parse_ini(contents: &str) -> Vec<(String, String)> {
    let mut section = String::new();
    let mut parsed = Vec::new();

    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].trim().into();
            continue;
        }
        let Some((key, value)) = split_ini_entry(line) else {
            continue;
        };
        let qualified = if section.is_empty() {
            key.trim().to_string()
        } else {
            format!("{section}.{}", key.trim())
        };
        parsed.push((qualified, value.trim().into()));
    }

    parsed
}

fn split_ini_entry(line: &str) -> Option<(&str, &str)> {
    let mut escaped = false;
    for (index, character) in line.char_indices() {
        if escaped {
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == '=' || character == ':' {
            return Some((&line[..index], &line[index + character.len_utf8()..]));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::super::config_acceptance_support::{ExpectedConfigFile, ExpectedOutputRoot};
    use super::*;

    #[test]
    fn properties_parser_uses_first_unescaped_separator_and_skips_comments() {
        let parsed = parse_properties(
            "# generated\nserver-port=25565\nname: LanGame\nescaped\\=key=value\nwhite space value\n",
        );

        assert_eq!(
            parsed,
            vec![
                (String::from("server-port"), String::from("25565")),
                (String::from("name"), String::from("LanGame")),
                (String::from("escaped=key"), String::from("value")),
                (String::from("white"), String::from("space value")),
            ]
        );
    }

    #[test]
    fn ini_parser_qualifies_keys_with_the_current_section() {
        let parsed = parse_ini(
            "; generated\n[/Script/Game.Server]\nPort=7777\nOption=first\nOption=second\nName: LanGame\n[Access]\nPassword=\n",
        );

        assert_eq!(
            parsed,
            vec![
                (
                    String::from("/Script/Game.Server.Port"),
                    String::from("7777")
                ),
                (
                    String::from("/Script/Game.Server.Option"),
                    String::from("first")
                ),
                (
                    String::from("/Script/Game.Server.Option"),
                    String::from("second")
                ),
                (
                    String::from("/Script/Game.Server.Name"),
                    String::from("LanGame")
                ),
                (String::from("Access.Password"), String::new()),
            ]
        );
    }

    #[test]
    fn expected_files_are_resolved_only_against_the_declared_root() {
        let temp = super::super::config_acceptance_support::unique_system_temp_root(
            "declared-output-root",
        );
        let config = temp.join("config");
        let install = temp.join("install");
        let saves = temp.join("saves");
        let instance = temp.join("instance");
        for root in [&config, &install, &saves, &instance] {
            std::fs::create_dir_all(root).unwrap();
        }
        std::fs::write(config.join("server.ini"), "Enabled=true\n").unwrap();
        let expected = ExpectedConfigFile {
            root: ExpectedOutputRoot::Install,
            path: "server.ini".into(),
            format: String::from("ini"),
            keys: Some(Map::from_iter([(
                String::from("Enabled"),
                Value::Bool(true),
            )])),
            entries: None,
            fragments: None,
        };

        let error = verify_expected_file(
            &expected,
            &AcceptanceOutputRoots {
                config: &config,
                install: &install,
                saves: &saves,
                instance: &instance,
            },
            Path::new("fixture.json"),
        )
        .unwrap_err();
        assert!(error.contains("Install output"), "{error}");
        std::fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn keys_mode_rejects_duplicate_keys_and_ordered_mode_detects_wrong_order() {
        let temp =
            super::super::config_acceptance_support::unique_system_temp_root("ordered-entries");
        for name in ["config", "install", "saves", "instance"] {
            std::fs::create_dir_all(temp.join(name)).unwrap();
        }
        std::fs::write(
            temp.join("config").join("Game.ini"),
            "[Server]\nMod=first\nMod=second\n",
        )
        .unwrap();
        let config = temp.join("config");
        let install = temp.join("install");
        let saves = temp.join("saves");
        let instance = temp.join("instance");
        let roots = AcceptanceOutputRoots {
            config: &config,
            install: &install,
            saves: &saves,
            instance: &instance,
        };
        let keys = ExpectedConfigFile {
            root: ExpectedOutputRoot::Config,
            path: "Game.ini".into(),
            format: String::from("ini"),
            keys: Some(Map::from_iter([(
                String::from("Server.Mod"),
                Value::String(String::from("second")),
            )])),
            entries: None,
            fragments: None,
        };
        assert!(
            verify_expected_file(&keys, &roots, Path::new("fixture.json"))
                .unwrap_err()
                .contains("must use ordered entries")
        );

        let ordered = ExpectedConfigFile {
            keys: None,
            entries: Some(vec![
                ExpectedConfigEntry {
                    key: String::from("Server.Mod"),
                    value: Value::String(String::from("second")),
                },
                ExpectedConfigEntry {
                    key: String::from("Server.Mod"),
                    value: Value::String(String::from("first")),
                },
            ]),
            ..keys
        };
        assert!(
            verify_expected_file(&ordered, &roots, Path::new("fixture.json"))
                .unwrap_err()
                .contains("ordered entries")
        );
        std::fs::remove_dir_all(temp).unwrap();
    }
}
