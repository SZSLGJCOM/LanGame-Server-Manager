use std::collections::{HashMap, HashSet};

pub(in super::super) struct IniDocument {
    has_utf8_bom: bool,
    preamble: Vec<String>,
    sections: Vec<IniSection>,
    epilogue: Vec<String>,
}

struct IniSection {
    prefix: Vec<String>,
    name: String,
    header: String,
    lines: Vec<String>,
}

pub(in super::super) fn merge_ini_documents(
    mut existing: IniDocument,
    rendered: IniDocument,
    removed_sections: &[&str],
) -> String {
    let removed = removed_sections
        .iter()
        .map(|value| canonical_name(value))
        .collect::<HashSet<_>>();
    existing
        .sections
        .retain(|section| !removed.contains(&canonical_name(&section.name)));

    let mut rendered_sections = Vec::<IniSection>::new();
    let mut rendered_indexes = HashMap::<String, usize>::new();
    for section in rendered.sections {
        let name = canonical_name(&section.name);
        if let Some(index) = rendered_indexes.get(&name).copied() {
            rendered_sections[index].lines.extend(section.prefix);
            rendered_sections[index].lines.extend(section.lines);
        } else {
            rendered_indexes.insert(name, rendered_sections.len());
            rendered_sections.push(section);
        }
    }

    for rendered_section in rendered_sections {
        let section_name = canonical_name(&rendered_section.name);
        let matching = existing
            .sections
            .iter()
            .enumerate()
            .filter_map(|(index, section)| {
                (canonical_name(&section.name) == section_name).then_some(index)
            })
            .collect::<Vec<_>>();
        if matching.is_empty() {
            existing.sections.push(rendered_section);
            continue;
        }

        let managed_keys = rendered_section
            .lines
            .iter()
            .filter_map(|line| ini_assignment_key(line).ok().flatten())
            .collect::<HashSet<_>>();
        let rendered_metadata = assignment_metadata_by_key(&rendered_section.lines);
        let target_index = matching[0];
        let mut insertion_index = None;
        for index in matching {
            let mut preserved = Vec::with_capacity(existing.sections[index].lines.len());
            let mut pending_metadata = Vec::new();
            for line in existing.sections[index].lines.drain(..) {
                match ini_assignment_key(&line).ok().flatten() {
                    None => pending_metadata.push(line),
                    Some(key) if managed_keys.contains(&key) => {
                        if let Some(candidates) = rendered_metadata.get(&key) {
                            strip_matching_metadata(&mut pending_metadata, candidates);
                        }
                        preserved.append(&mut pending_metadata);
                        if index == target_index && insertion_index.is_none() {
                            insertion_index = Some(preserved.len());
                        }
                    }
                    Some(_) => {
                        preserved.append(&mut pending_metadata);
                        preserved.push(line);
                    }
                }
            }
            preserved.append(&mut pending_metadata);
            existing.sections[index].lines = preserved;
        }

        let insertion_index =
            insertion_index.unwrap_or(existing.sections[target_index].lines.len());
        existing.sections[target_index]
            .lines
            .splice(insertion_index..insertion_index, rendered_section.lines);
    }

    render_ini_document(existing)
}

fn assignment_metadata_by_key(lines: &[String]) -> HashMap<String, Vec<Vec<String>>> {
    let mut result = HashMap::<String, Vec<Vec<String>>>::new();
    let mut pending = Vec::new();
    for line in lines {
        match ini_assignment_key(line).ok().flatten() {
            Some(key) => {
                result
                    .entry(key)
                    .or_default()
                    .push(std::mem::take(&mut pending));
            }
            None => pending.push(line.clone()),
        }
    }
    result
}

fn strip_matching_metadata(pending: &mut Vec<String>, candidates: &[Vec<String>]) {
    let best = candidates
        .iter()
        .filter(|candidate| !candidate.is_empty() && candidate.len() <= pending.len())
        .filter_map(|candidate| {
            pending
                .windows(candidate.len())
                .rposition(|window| window == candidate)
                .map(|start| (start, candidate.len()))
        })
        .max_by_key(|(start, length)| (*start, *length));
    if let Some((start, length)) = best {
        pending.drain(start..start + length);
    }
}

pub(in super::super) fn parse_ini_document(content: &str) -> Result<IniDocument, String> {
    let mut document = IniDocument {
        has_utf8_bom: content.starts_with('\u{feff}'),
        preamble: Vec::new(),
        sections: Vec::new(),
        epilogue: Vec::new(),
    };
    for (index, raw_line) in content.lines().enumerate() {
        let raw_line = if index == 0 {
            raw_line.strip_prefix('\u{feff}').unwrap_or(raw_line)
        } else {
            raw_line
        };
        let line = raw_line.trim_end_matches('\r').to_string();
        let trimmed = trim_ini_syntax(&line);
        if trimmed.starts_with('[') {
            if !trimmed.ends_with(']') || trimmed.len() < 3 {
                return Err(format!("line {} has a malformed section header", index + 1));
            }
            let name = trimmed[1..trimmed.len() - 1].trim();
            if name.is_empty() || name.contains('[') || name.contains(']') {
                return Err(format!("line {} has a malformed section name", index + 1));
            }
            let prefix = document
                .sections
                .last_mut()
                .map(take_trailing_metadata)
                .unwrap_or_default();
            document.sections.push(IniSection {
                prefix,
                name: name.to_string(),
                header: format!("[{name}]"),
                lines: Vec::new(),
            });
            continue;
        }

        let is_metadata = is_ini_metadata(&line);
        if document.sections.is_empty() {
            if !is_metadata {
                return Err(format!(
                    "line {} assigns a value outside a section",
                    index + 1
                ));
            }
            document.preamble.push(line);
            continue;
        }
        if !is_metadata {
            ini_assignment_key(&line).map_err(|message| format!("line {} {message}", index + 1))?;
        }
        document
            .sections
            .last_mut()
            .expect("section exists")
            .lines
            .push(line);
    }
    if let Some(last) = document.sections.last_mut() {
        document.epilogue = take_trailing_metadata(last);
    }
    Ok(document)
}

fn take_trailing_metadata(section: &mut IniSection) -> Vec<String> {
    let first = section
        .lines
        .iter()
        .rposition(|line| !is_ini_metadata(line))
        .map_or(0, |index| index + 1);
    section.lines.split_off(first)
}

fn is_ini_metadata(line: &str) -> bool {
    let trimmed = trim_ini_syntax(line);
    trimmed.is_empty()
        || trimmed.starts_with(';')
        || trimmed.starts_with('#')
        || trimmed.starts_with("//")
}

fn trim_ini_syntax(line: &str) -> &str {
    line.strip_prefix('\u{feff}').unwrap_or(line).trim()
}

fn ini_assignment_key(line: &str) -> Result<Option<String>, String> {
    if is_ini_metadata(line) {
        return Ok(None);
    }
    let trimmed = line.trim();
    let Some(separator) = trimmed.find('=') else {
        return Err(String::from("does not contain an assignment"));
    };
    let key = trimmed[..separator].trim();
    if key.is_empty() {
        return Err(String::from("has an empty assignment key"));
    }
    Ok(Some(canonical_name(key)))
}

fn render_ini_document(document: IniDocument) -> String {
    let has_utf8_bom = document.has_utf8_bom;
    let mut lines = document.preamble;
    for section in document.sections {
        lines.extend(section.prefix);
        lines.push(section.header);
        lines.extend(section.lines);
    }
    lines.extend(document.epilogue);
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines.push(String::new());
    let rendered = lines.join("\n");
    if has_utf8_bom {
        format!("\u{feff}{rendered}")
    } else {
        rendered
    }
}

fn canonical_name(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}
