use super::*;

pub(super) fn render_barotrauma_config_player_xml(
    baseline: &str,
    regular_package_paths: &[String],
    save_root: &Path,
) -> Result<String, &'static str> {
    if baseline.trim().is_empty() {
        return Ok(render_default_barotrauma_config_player_xml(
            regular_package_paths,
            save_root,
        ));
    }
    let Some(baseline) = with_instance_save_root(baseline, save_root) else {
        return Err(
            "The existing config_player.xml has an invalid config root; it was not replaced.",
        );
    };
    let replacement = render_barotrauma_regularpackages_xml(regular_package_paths);
    let layout = content_layout(&baseline)?;
    let (start, end, replacement) = if let Some((start, end)) = layout.regular {
        (start, end, replacement)
    } else if let Some((start, end, closing)) = layout.content {
        if let Some(closing) = closing {
            (closing, closing, replacement)
        } else {
            let open = baseline[start..end]
                .strip_suffix("/>")
                .ok_or("The contentpackages element is incomplete.")?;
            (
                start,
                end,
                format!(
                    "{}>{}{}</contentpackages>",
                    open.trim_end(),
                    vanilla_core(),
                    replacement
                ),
            )
        }
    } else {
        (
            layout.root_close,
            layout.root_close,
            format!(
                "<contentpackages>{}{}</contentpackages>",
                vanilla_core(),
                replacement
            ),
        )
    };
    Ok(format!(
        "{}{}{}",
        &baseline[..start],
        replacement,
        &baseline[end..]
    ))
}

fn vanilla_core() -> &'static str {
    "<corepackage path=\"Content/ContentPackages/Vanilla.xml\" />"
}

#[derive(Default)]
struct ContentLayout {
    content: Option<(usize, usize, Option<usize>)>,
    regular: Option<(usize, usize)>,
    root_close: usize,
}

// Use native XML events only to locate the owned nodes. Keeping byte offsets
// preserves comments, unknown properties and formatting outside those nodes.
fn content_layout(document: &str) -> Result<ContentLayout, &'static str> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(document);
    let mut stack: Vec<String> = Vec::new();
    let mut layout = ContentLayout::default();
    let mut root_seen = false;
    let mut regular_start = None;
    loop {
        let start = reader.buffer_position() as usize;
        let event = reader
            .read_event()
            .map_err(|_| "The existing config_player.xml is malformed; it was not replaced.")?;
        let end = reader.buffer_position() as usize;
        match event {
            Event::Start(node) | Event::Empty(node) => {
                let empty = document[start..end].ends_with("/>");
                let name = node.name().as_ref().to_owned();
                if stack.is_empty() {
                    if root_seen || name != "config" || empty {
                        return Err("The existing config_player.xml must have one config root.");
                    }
                    root_seen = true;
                } else if stack.len() == 1 && name == "contentpackages" {
                    if layout.content.is_some() {
                        return Err("Duplicate contentpackages elements are ambiguous.");
                    }
                    layout.content = Some((start, end, None));
                } else if stack.len() == 2
                    && stack[1] == "contentpackages"
                    && name == "regularpackages"
                {
                    if layout.regular.is_some() || regular_start.is_some() {
                        return Err("Duplicate regularpackages elements are ambiguous.");
                    }
                    if empty {
                        layout.regular = Some((start, end));
                    } else {
                        regular_start = Some(start);
                    }
                }
                if !empty {
                    if stack.len() >= 64 {
                        return Err("The XML configuration nesting exceeds the limit.");
                    }
                    stack.push(name);
                }
            }
            Event::End(node) => {
                if stack.last().map(String::as_str) != Some(node.name().as_ref()) {
                    return Err("The existing config_player.xml has mismatched elements.");
                }
                if stack.len() == 3
                    && stack[1] == "contentpackages"
                    && stack[2] == "regularpackages"
                {
                    layout.regular = Some((
                        regular_start
                            .take()
                            .ok_or("Missing regularpackages opening element.")?,
                        end,
                    ));
                } else if stack.len() == 2 && stack[1] == "contentpackages" {
                    if let Some(content) = layout.content.as_mut() {
                        content.2 = Some(start);
                    }
                } else if stack.len() == 1 {
                    layout.root_close = start;
                }
                stack.pop();
            }
            Event::Text(text)
                if stack.is_empty()
                    && text
                        .as_ref()
                        .bytes()
                        .any(|byte| !byte.is_ascii_whitespace()) =>
            {
                return Err("The XML configuration has text outside its root.");
            }
            Event::CData(_) | Event::DocType(_) if stack.is_empty() => {
                return Err("The XML configuration has unsupported content outside its root.");
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !root_seen || !stack.is_empty() || layout.root_close == 0 {
        return Err("The existing config_player.xml is incomplete; it was not replaced.");
    }
    Ok(layout)
}

fn render_barotrauma_regularpackages_xml(regular_package_paths: &[String]) -> String {
    if regular_package_paths.is_empty() {
        return String::from("<regularpackages />");
    }

    let mut lines = vec![String::from("<regularpackages>")];
    let mut seen = HashSet::new();
    for path in regular_package_paths {
        let normalized = path.replace('\\', "/");
        if normalized.trim().is_empty() || !seen.insert(normalized.to_ascii_lowercase()) {
            continue;
        }
        lines.push(format!(
            "    <regularpackage path=\"{}\" />",
            escape_xml_attribute(&normalized)
        ));
    }
    lines.push(String::from("  </regularpackages>"));
    lines.join("\n")
}

fn render_default_barotrauma_config_player_xml(
    regular_package_paths: &[String],
    save_root: &Path,
) -> String {
    format!(
        "<config savepath=\"{}\">\n  <contentpackages>\n    <corepackage path=\"Content/ContentPackages/Vanilla.xml\" />\n    {}\n  </contentpackages>\n</config>\n",
        escape_xml_attribute(&save_root.to_string_lossy()),
        render_barotrauma_regularpackages_xml(regular_package_paths)
    )
}

// GameSettings reads lowercase attributes from the config root. SaveUtil adds
// Multiplayer itself, so savepath points to the instance configuration directory.
fn with_instance_save_root(baseline: &str, save_root: &Path) -> Option<String> {
    let text = baseline.trim_start_matches('\u{feff}');
    let mut offset = baseline.len() - text.len();
    loop {
        offset += baseline[offset..].len() - baseline[offset..].trim_start().len();
        let remainder = &baseline[offset..];
        if remainder.starts_with("<?") {
            offset += remainder.find("?>")? + 2;
        } else if remainder.starts_with("<!--") {
            offset += remainder.find("-->")? + 3;
        } else {
            break;
        }
    }
    let start = offset;
    if !baseline[offset..].starts_with("<config") {
        return None;
    }
    offset += "<config".len();
    let mut attributes = String::new();
    let bytes = baseline.as_bytes();
    loop {
        let whitespace_start = offset;
        while bytes.get(offset).is_some_and(u8::is_ascii_whitespace) {
            offset += 1;
        }
        match bytes.get(offset)? {
            b'>' => {
                offset += 1;
                break;
            }
            b'/' => return None,
            _ if offset == whitespace_start => return None,
            _ => {}
        }
        let name_start = offset;
        while bytes.get(offset).is_some_and(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b':' | b'.')
        }) {
            offset += 1;
        }
        if offset == name_start {
            return None;
        }
        let name = &baseline[name_start..offset];
        while bytes.get(offset).is_some_and(u8::is_ascii_whitespace) {
            offset += 1;
        }
        if bytes.get(offset)? != &b'=' {
            return None;
        }
        offset += 1;
        while bytes.get(offset).is_some_and(u8::is_ascii_whitespace) {
            offset += 1;
        }
        let quote = *bytes.get(offset)?;
        if !matches!(quote, b'\'' | b'"') {
            return None;
        }
        offset += 1;
        while *bytes.get(offset)? != quote {
            offset += 1;
        }
        offset += 1;
        if !name.eq_ignore_ascii_case("savepath") {
            attributes.push_str(&baseline[whitespace_start..offset]);
        }
    }
    Some(format!(
        "{}<config savepath=\"{}\"{}>{}",
        &baseline[..start],
        escape_xml_attribute(&save_root.to_string_lossy()),
        attributes,
        &baseline[offset..]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_save_path_overrides_shared_path_and_preserves_other_attributes_and_packages() {
        let baseline = "<?xml version=\"1.0\"?><!-- <config> --><config savepath='shared' language=\"English\" note=\"x > y\"><contentpackages><corepackage path=\"Content/Vanilla.xml\"/><regularpackages/></contentpackages></config>";
        let save_root = Path::new("D:/Instances/Host & Friends/quoted \"profile\"");
        let rendered = render_barotrauma_config_player_xml(
            baseline,
            &["LocalMods/My & Pack/filelist.xml".to_owned()],
            save_root,
        )
        .unwrap();
        assert!(
            rendered.contains(
                "savepath=\"D:/Instances/Host &amp; Friends/quoted &quot;profile&quot;\""
            )
        );
        assert!(!rendered.contains("savepath='shared'"));
        assert!(rendered.contains("language=\"English\" note=\"x > y\""));
        assert!(rendered.contains("path=\"Content/Vanilla.xml\""));
        assert!(rendered.contains("path=\"LocalMods/My &amp; Pack/filelist.xml\""));
        assert_eq!(rendered.matches("savepath=").count(), 1);
    }

    #[test]
    fn absent_baseline_gets_defaults_but_unparseable_existing_config_is_rejected() {
        let root = Path::new("D:/My Instance/config");
        let rendered = render_barotrauma_config_player_xml("", &[], root).unwrap();
        assert!(rendered.starts_with("<config savepath=\"D:/My Instance/config\">"));
        assert!(rendered.contains("<regularpackages />"));
        assert!(render_barotrauma_config_player_xml("<config broken", &[], root).is_err());
        assert!(
            render_barotrauma_config_player_xml("<config><regularpackages>", &[], root).is_err()
        );
    }

    #[test]
    fn adding_a_missing_regular_package_list_preserves_existing_core_and_attributes() {
        let baseline = "<config language=\"English\"><contentpackages><corepackage path=\"LocalMods/Core/filelist.xml\"/></contentpackages></config>";
        let rendered =
            render_barotrauma_config_player_xml(baseline, &[], Path::new("instance")).unwrap();
        assert!(rendered.contains("language=\"English\""));
        assert!(rendered.contains("LocalMods/Core/filelist.xml"));
        assert!(rendered.contains("<regularpackages /></contentpackages>"));
        let empty = render_barotrauma_config_player_xml(
            "<config language=\"English\"><contentpackages /></config>",
            &[],
            Path::new("instance"),
        )
        .unwrap();
        assert!(empty.contains("language=\"English\""));
        assert!(empty.contains("Content/ContentPackages/Vanilla.xml"));
        assert!(empty.contains("<regularpackages /></contentpackages>"));
    }

    #[test]
    fn comments_and_cdata_cannot_redirect_the_owned_package_list_update() {
        let baseline = "<config><!-- <regularpackages/> --><note><![CDATA[<regularpackages/>]]></note><contentpackages><regularpackages><regularpackage path=\"old\"/></regularpackages></contentpackages></config>";
        let rendered = render_barotrauma_config_player_xml(
            baseline,
            &["LocalMods/New/filelist.xml".into()],
            Path::new("instance"),
        )
        .unwrap();
        assert!(rendered.contains("<!-- <regularpackages/> -->"));
        assert!(rendered.contains("<![CDATA[<regularpackages/>]]>"));
        assert!(rendered.contains("LocalMods/New/filelist.xml"));
        assert!(!rendered.contains("path=\"old\""));
    }

    #[test]
    fn duplicate_native_package_lists_are_rejected_as_ambiguous() {
        let baseline = "<config><contentpackages><regularpackages/><regularpackages/></contentpackages></config>";
        assert!(render_barotrauma_config_player_xml(baseline, &[], Path::new("instance")).is_err());
    }
}
