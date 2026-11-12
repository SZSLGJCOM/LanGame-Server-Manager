use std::path::Path;

use super::{StorageError, invalid};

const SECTION: &str = "/Script/Dominion.DedicatedServerSettings";

fn text<'a>(bytes: &'a [u8], path: &Path) -> Result<&'a str, StorageError> {
    std::str::from_utf8(bytes)
        .map_err(|_| invalid(path, "DedicatedServer.ini is not supported UTF-8 text."))
}

pub(super) fn raw_value(
    bytes: &[u8],
    key: &str,
    path: &Path,
) -> Result<Option<String>, StorageError> {
    let mut selected = false;
    let mut found = None;
    for raw in text(bytes, path)?.trim_start_matches('\u{feff}').lines() {
        let line = raw.trim();
        if line.starts_with('[') && line.ends_with(']') {
            selected = line[1..line.len() - 1].trim().eq_ignore_ascii_case(SECTION);
        } else if selected
            && !line.starts_with([';', '#'])
            && let Some((name, value)) = raw.split_once('=')
            && name.trim().eq_ignore_ascii_case(key)
        {
            if found.is_some() {
                return Err(invalid(
                    path,
                    format!("DedicatedServer.ini contains duplicate {key} entries."),
                ));
            }
            found = Some(value.to_owned());
        }
    }
    Ok(found)
}

pub(super) fn value(bytes: &[u8], key: &str, path: &Path) -> Result<Option<String>, StorageError> {
    Ok(raw_value(bytes, key, path)?.map(|raw| {
        let value = raw.trim();
        value
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .unwrap_or(value)
            .to_owned()
    }))
}

/// Change only the password assignment; native ServerGuid, bans and newly
/// introduced settings retain their exact text, including newline style.
pub(super) fn replace_password(
    bytes: &[u8],
    replacement: Option<&str>,
    path: &Path,
) -> Result<Vec<u8>, StorageError> {
    value(bytes, "WorldPassword", path)?; // Reject ambiguous assignments first.
    let source = text(bytes, path)?;
    let mut output = String::with_capacity(source.len() + 64);
    let mut selected = false;
    let mut section_seen = false;
    let mut replaced = false;
    let mut insertion = None;
    let newline = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    for line in source.split_inclusive('\n') {
        let content = line.trim_end_matches(['\r', '\n']);
        let trimmed = content.trim_start_matches('\u{feff}').trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            selected = trimmed[1..trimmed.len() - 1]
                .trim()
                .eq_ignore_ascii_case(SECTION);
            section_seen |= selected;
            output.push_str(line);
            if selected {
                insertion = Some(output.len());
            }
            continue;
        }
        if selected
            && !trimmed.starts_with([';', '#'])
            && let Some((key, _)) = content.split_once('=')
            && key.trim().eq_ignore_ascii_case("WorldPassword")
        {
            if let Some(value) = replacement {
                output.push_str(key);
                output.push('=');
                output.push_str(value);
                output.push_str(&line[content.len()..]);
            }
            replaced = true;
            continue;
        }
        output.push_str(line);
    }
    if !section_seen {
        return Err(invalid(
            path,
            "DedicatedServer.ini has no dedicated-server settings section.",
        ));
    }
    if !replaced && let Some(value) = replacement {
        let offset = insertion.expect("the dedicated-server section was observed");
        let separator = if output[..offset].ends_with('\n') {
            ""
        } else {
            newline
        };
        output.insert_str(
            offset,
            &format!("{separator}WorldPassword={value}{newline}"),
        );
    }
    Ok(output.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_replacement_restores_exact_rhs_and_keeps_native_fields() {
        let path = Path::new("DedicatedServer.ini");
        let original = "\u{feff}[/Script/Dominion.DedicatedServerSettings]\r\nWorldPassword=  \"fixture-password\"  \r\nServerGuid=keep\r\n";
        let raw = raw_value(original.as_bytes(), "WorldPassword", path).unwrap();
        assert_eq!(raw.as_deref(), Some("  \"fixture-password\"  "));
        let temporary = replace_password(
            original.as_bytes(),
            Some("0123456789abcdef0123456789abcdef"),
            path,
        )
        .unwrap();
        assert_eq!(
            value(&temporary, "WorldPassword", path).unwrap().as_deref(),
            Some("0123456789abcdef0123456789abcdef")
        );
        let restored = replace_password(&temporary, raw.as_deref(), path).unwrap();
        assert_eq!(restored, original.as_bytes());
    }

    #[test]
    fn originally_missing_password_is_removed_without_losing_native_guid() {
        let path = Path::new("DedicatedServer.ini");
        let original = b"[/Script/Dominion.DedicatedServerSettings]\nServerGuid=keep\n";
        let temporary = replace_password(original, Some("random"), path).unwrap();
        assert_eq!(replace_password(&temporary, None, path).unwrap(), original);
    }
}
