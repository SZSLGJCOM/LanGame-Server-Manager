use quick_xml::Reader;
use quick_xml::events::Event;
use std::ops::Range;
use std::path::Path;

use super::{ManagedConfigMergePlan, ManagedConfigMutation, StorageError, managed_config_merge};

const MANAGED: [&str; 4] = ["users", "whitelist", "blacklist", "commands"];

struct AdminLayout {
    sections: Vec<(usize, Range<usize>)>,
    // A normal close tag begins at this empty range. For an empty root the
    // range contains its "/>" so native root attributes can stay in place.
    close: Range<usize>,
}

fn layout(document: &[u8]) -> Result<AdminLayout, &'static str> {
    // quick-xml measures offsets after the UTF-8 BOM. Keep ranges relative to
    // the original bytes used for the atomic merge.
    let xml = document.strip_prefix(b"\xef\xbb\xbf").unwrap_or(document);
    let prefix = document.len() - xml.len();
    let mut reader = Reader::from_reader(xml);
    let mut inside = false;
    let mut close = None;
    let mut sections = Vec::new();
    loop {
        let start = prefix + reader.buffer_position() as usize;
        let event = reader.read_event().map_err(|_| "invalid XML")?;
        let end = prefix + reader.buffer_position() as usize;
        let empty = matches!(event, Event::Empty(_));
        match event {
            Event::Start(node) | Event::Empty(node) => {
                if !inside {
                    if close.is_some() || node.name().as_ref() != "adminTools" {
                        return Err("expected one adminTools root");
                    }
                    if empty {
                        close = Some(end - 2..end);
                    } else {
                        inside = true;
                    }
                    continue;
                }
                let tag = node.name();
                if !empty {
                    reader.read_to_end(tag).map_err(|_| "invalid XML section")?;
                }
                if let Some(key) = MANAGED.iter().position(|&key| key == tag.as_ref()) {
                    if sections.iter().any(|(previous, _)| *previous == key) {
                        return Err("duplicate managed roster section");
                    }
                    sections.push((key, start..prefix + reader.buffer_position() as usize));
                }
            }
            Event::End(node) if inside && node.name().as_ref() == "adminTools" => {
                inside = false;
                close = Some(start..start);
            }
            Event::Comment(_) | Event::PI(_) => {}
            Event::Decl(_) if !inside && close.is_none() => {}
            Event::Text(text) if text.as_ref().chars().all(char::is_whitespace) => {}
            Event::Eof if !inside => break,
            _ => return Err("unexpected XML content"),
        }
    }
    Ok(AdminLayout {
        sections,
        close: close.ok_or("missing or incomplete adminTools root")?,
    })
}

fn merge(native: &[u8], rendered: &[u8]) -> Result<Vec<u8>, &'static str> {
    let native_layout = layout(native)?;
    let template = layout(rendered)?;
    if template.sections.len() != MANAGED.len() {
        return Err("rendered template is missing a managed roster section");
    }

    // Replace only the four direct child sections managed by the settings
    // editor. Preserve every other byte, including native Web permissions.
    let mut output = Vec::with_capacity(native.len());
    let mut cursor = 0;
    let mut replaced = [false; MANAGED.len()];
    for (key, range) in native_layout.sections {
        let (_, replacement) = template
            .sections
            .iter()
            .find(|(k, _)| *k == key)
            .ok_or("rendered roster section is missing")?;
        output.extend_from_slice(&native[cursor..range.start]);
        output.extend_from_slice(&rendered[replacement.clone()]);
        cursor = range.end;
        replaced[key] = true;
    }
    let close = native_layout.close;
    output.extend_from_slice(&native[cursor..close.start]);
    if !close.is_empty() {
        output.extend_from_slice(b">");
    }
    for (key, range) in template.sections {
        if !replaced[key] {
            output.extend_from_slice(b"\n  ");
            output.extend_from_slice(&rendered[range]);
            output.extend_from_slice(b"\n");
        }
    }
    if !close.is_empty() {
        output.extend_from_slice(b"</adminTools>");
    }
    output.extend_from_slice(&native[close.end..]);
    Ok(output)
}

pub(super) fn materialize(
    source: &Path,
    destination: &Path,
    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    let rendered = std::fs::read(source).map_err(|error| StorageError::ReadConfig {
        path: source.to_owned(),
        source: error,
    })?;
    let original = managed_config_merge::read_optional_bytes(destination)?;
    let replacement = match &original {
        Some(native) => merge(native, &rendered).map_err(|reason| {
            StorageError::ModuleSupportMaterialization {
                module_id: "sevendaystodie".into(),
                path: destination.to_owned(),
                message: format!("Could not merge serveradmin.xml: {reason}."),
            }
        })?,
        None => rendered,
    };
    if original.as_ref() == Some(&replacement) {
        return Ok(());
    }
    files.apply(vec![ManagedConfigMergePlan {
        destination_path: destination.to_owned(),
        replacement,
        original,
    }])
}

#[cfg(test)]
mod tests {
    use super::merge;

    const TEMPLATE: &[u8] = b"<adminTools><users/><whitelist/><blacklist/><commands/></adminTools>";

    #[test]
    fn sevendaystodie_admin_merge_keeps_native_metadata_and_rejects_ambiguous_rosters() {
        let original = b"\xef\xbb\xbf<?xml version=\"1.0\"?><adminTools format=\"native\"><!--keep--><users old=\"managed\"/><webusers><user name=\"fixture\"/></webusers></adminTools>";
        let merged = merge(original, TEMPLATE).unwrap();
        assert!(merged.starts_with(b"\xef\xbb\xbf<?xml version=\"1.0\"?><adminTools format=\"native\"><!--keep--><users/><webusers><user name=\"fixture\"/></webusers>"));
        assert_eq!(merge(&merged, TEMPLATE).unwrap(), merged);
        assert!(merge(b"<adminTools><users/><users/></adminTools>", TEMPLATE).is_err());
        let empty = merge(b"<adminTools format=\"native\"/>", TEMPLATE).unwrap();
        assert!(empty.starts_with(b"<adminTools format=\"native\">"));
        assert_eq!(merge(&empty, TEMPLATE).unwrap(), empty);
    }
}
