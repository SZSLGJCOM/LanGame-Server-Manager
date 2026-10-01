use super::*;
use managed_config_merge::{ManagedConfigMergePlan, materialization_error, read_optional_bytes};
use std::collections::{HashMap, HashSet};

#[path = "ark_ini_ownership.rs"]
mod ownership;
use ownership::{GAME_MODE, NativeKey, canonical_key, key_family, owned_keys};
#[path = "ark_ini_history.rs"]
mod history;
pub(super) use history::capture_previous_ownership;
use history::{Snapshot, Target};

#[derive(Clone, Copy, Default)]
enum Encoding {
    #[default]
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
}

impl Encoding {
    fn decode(bytes: &[u8]) -> Result<(Self, String), String> {
        let (encoding, body) = if let Some(body) = bytes.strip_prefix(&[0xff, 0xfe]) {
            (Self::Utf16Le, body)
        } else if let Some(body) = bytes.strip_prefix(&[0xfe, 0xff]) {
            (Self::Utf16Be, body)
        } else if let Some(body) = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]) {
            (Self::Utf8Bom, body)
        } else {
            (Self::Utf8, bytes)
        };
        let text = match encoding {
            Self::Utf16Le | Self::Utf16Be => {
                let (pairs, remainder) = body.as_chunks::<2>();
                if !remainder.is_empty() {
                    return Err("native INI contains an incomplete UTF-16 code unit".into());
                }
                let units = pairs
                    .iter()
                    .map(|pair| {
                        if matches!(encoding, Self::Utf16Le) {
                            u16::from_le_bytes(*pair)
                        } else {
                            u16::from_be_bytes(*pair)
                        }
                    })
                    .collect::<Vec<_>>();
                String::from_utf16(&units)
                    .map_err(|_| "native INI contains invalid UTF-16".to_owned())?
            }
            _ => std::str::from_utf8(body)
                .map_err(|_| "native INI must use UTF-8 or UTF-16 with a BOM".to_owned())?
                .to_owned(),
        };
        if text.contains('\0') {
            return Err("native INI cannot contain NUL characters".into());
        }
        Ok((encoding, text))
    }

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
}

#[derive(Default)]
struct Document {
    encoding: Encoding,
    crlf: bool,
    preamble: Vec<String>,
    sections: Vec<Section>,
}

struct Section {
    name: String,
    header: String,
    lines: Vec<String>,
}

impl Document {
    fn parse(content: &str) -> Result<Self, String> {
        let mut document = Self {
            encoding: if content.starts_with('\u{feff}') {
                Encoding::Utf8Bom
            } else {
                Encoding::Utf8
            },
            crlf: content.contains("\r\n"),
            ..Self::default()
        };
        for (index, line) in content.trim_start_matches('\u{feff}').lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with('[') {
                let Some(end) = trimmed.find(']') else {
                    return Err(format!(
                        "line {} has an incomplete section header",
                        index + 1
                    ));
                };
                let name = trimmed[1..end].trim();
                if name.is_empty() || name.contains('[') || !metadata(&trimmed[end + 1..]) {
                    return Err(format!("line {} has an invalid section header", index + 1));
                }
                document.sections.push(Section {
                    name: name.to_ascii_lowercase(),
                    header: line.to_owned(),
                    lines: Vec::new(),
                });
            } else {
                if !metadata(line) && (assignment(line).is_none() || document.sections.is_empty()) {
                    return Err(format!(
                        "line {} must assign a key inside a section",
                        index + 1
                    ));
                }
                match document.sections.last_mut() {
                    Some(section) => section.lines.push(line.to_owned()),
                    None => document.preamble.push(line.to_owned()),
                }
            }
        }
        Ok(document)
    }

    fn keys(&self) -> HashSet<NativeKey> {
        self.sections
            .iter()
            .flat_map(|section| {
                section
                    .lines
                    .iter()
                    .filter_map(|line| assignment(line).map(|key| (section.name.clone(), key)))
            })
            .collect()
    }

    fn render(self) -> Vec<u8> {
        let mut lines = self.preamble;
        for section in self.sections {
            lines.push(section.header);
            lines.extend(section.lines);
        }
        while lines.last().is_some_and(String::is_empty) {
            lines.pop();
        }
        lines.push(String::new());
        self.encoding
            .encode(&lines.join(if self.crlf { "\r\n" } else { "\n" }))
    }
}

fn metadata(line: &str) -> bool {
    let line = line.trim();
    line.is_empty() || line.starts_with(';') || line.starts_with('#') || line.starts_with("//")
}

fn assignment(line: &str) -> Option<String> {
    if metadata(line) {
        return None;
    }
    let (key, _) = line.split_once('=')?;
    (!key.trim().is_empty()).then(|| canonical_key(key))
}

fn append_metadata(target: &mut Vec<String>, source: impl IntoIterator<Item = String>) {
    let mut existing = target
        .iter()
        .filter(|line| metadata(line))
        .cloned()
        .collect::<HashSet<_>>();
    for line in source {
        if !metadata(&line) || existing.insert(line.clone()) {
            target.push(line);
        }
    }
}

// `owned` uses families to remove stale indexed entries even when a setting is cleared.
// Unmanaged repeated assignments are sequences, never a key/value dictionary.
fn merge(
    mut existing: Document,
    source: Document,
    owned: &HashSet<NativeKey>,
    exact: &HashSet<NativeKey>,
    live: bool,
) -> Document {
    let original_keys = existing.keys();
    let source_keys = source.keys();
    let mut insertion_points = HashMap::new();
    for (index, section) in existing.sections.iter_mut().enumerate() {
        let mut retained = Vec::new();
        for line in section.lines.drain(..) {
            let removed = assignment(&line).is_some_and(|key| {
                owned.contains(&(section.name.clone(), key_family(&key)))
                    || exact.contains(&(section.name.clone(), key.clone()))
                    || (!live && source_keys.contains(&(section.name.clone(), key)))
            });
            if removed {
                insertion_points
                    .entry(section.name.clone())
                    .or_insert((index, retained.len()));
            } else {
                retained.push(line);
            }
        }
        section.lines = retained;
    }
    append_metadata(&mut existing.preamble, source.preamble);
    for section in source.sections {
        let name = section.name;
        let lines = section
            .lines
            .into_iter()
            .filter(|line| {
                assignment(line).is_none_or(|key| {
                    !live
                        || owned.contains(&(name.clone(), key_family(&key)))
                        || exact.contains(&(name.clone(), key.clone()))
                        || !original_keys.contains(&(name.clone(), key))
                })
            })
            .collect::<Vec<_>>();
        let position = insertion_points.get(&name).copied().or_else(|| {
            existing
                .sections
                .iter()
                .position(|target| target.name == name)
                .map(|index| (index, existing.sections[index].lines.len()))
        });
        if let Some((index, offset)) = position {
            let target = &mut existing.sections[index];
            let metadata = target
                .lines
                .iter()
                .filter(|line| metadata(line))
                .collect::<HashSet<_>>();
            let additions = lines
                .into_iter()
                .filter(|line| !metadata.contains(line))
                .collect::<Vec<_>>();
            let inserted = additions.len();
            target.lines.splice(offset..offset, additions);
            insertion_points.insert(name, (index, offset + inserted));
        } else {
            existing.sections.push(Section {
                name,
                header: section.header,
                lines,
            });
        }
    }
    existing
}

fn extra_document(settings: &Map<String, Value>, game_ini: bool) -> Result<Document, String> {
    let (field, section) = if game_ini {
        ("game_ini_extra", GAME_MODE)
    } else {
        ("game_user_settings_extra", "messageoftheday")
    };
    let raw = settings
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or_default();
    Document::parse(&format!("[{section}]\n{raw}"))
}

fn prepare_rendered(
    rendered: &str,
    settings: &Map<String, Value>,
    game_ini: bool,
) -> Result<Document, String> {
    let mut document = Document::parse(rendered)?;
    let extra = extra_document(settings, game_ini)?;
    let keys = extra.keys();
    // Raw extra is an explicit final override. Replace the entire repeated sequence
    // for its exact native key, retaining every row supplied in that sequence.
    for section in &mut document.sections {
        section.lines.retain(|line| {
            assignment(line).is_none_or(|key| !keys.contains(&(section.name.clone(), key)))
        });
    }
    Ok(merge(
        document,
        extra,
        &HashSet::new(),
        &HashSet::new(),
        false,
    ))
}

fn read_document(
    path: &Path,
    module_id: &str,
) -> Result<(Option<Vec<u8>>, Document), StorageError> {
    let original = read_optional_bytes(path)?;
    // Unreal may rewrite native INI files with a UTF-16 BOM. Keep that explicit
    // encoding through the merge; guessing an unmarked code page could lose keys.
    let (encoding, text) = Encoding::decode(original.as_deref().unwrap_or_default())
        .map_err(|error| materialization_error(module_id, path, error))?;
    let mut document =
        Document::parse(&text).map_err(|error| materialization_error(module_id, path, error))?;
    document.encoding = encoding;
    Ok((original, document))
}

pub(in crate::templates) fn write_rendered(
    input: &ModuleTemplateRenderInput<'_>,
    path: &Path,
    rendered: String,
    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    let game_ini = path
        .file_name()
        .is_some_and(|name| name == ARK_GAME_INI_FILE);
    if !matches!(
        input.module_id,
        "arksurvivalascended" | "arksurvivalevolved"
    ) || (!game_ini
        && !path
            .file_name()
            .is_some_and(|name| name == ARK_GAME_USER_SETTINGS_FILE))
    {
        return files.write(path, rendered.as_bytes());
    }
    let filename = if game_ini {
        ARK_GAME_INI_FILE
    } else {
        ARK_GAME_USER_SETTINGS_FILE
    };
    let mut history = Snapshot::load(
        input.config_dir,
        &input.config_dir.join("instance.json"),
        input.module_id,
    )?;
    let (original, mut existing) = read_document(path, input.module_id)?;
    let source = prepare_rendered(&rendered, input.settings, game_ini)
        .map_err(|error| materialization_error(input.module_id, path, error))?;
    if original.is_none() {
        existing.encoding = source.encoding;
        existing.crlf = source.crlf;
    }
    // Track the actual template output, including raw extra, before preservation
    // adds unknown native keys. Custom templates are authoritative for their keys too.
    let current_keys = source.keys();
    let mut exact = history.keys(filename, Target::Config);
    exact.extend(current_keys.clone());
    let replacement = merge(
        existing,
        source,
        &owned_keys(input.module_id, game_ini),
        &exact,
        false,
    )
    .render();
    history.record(filename, Target::Config, current_keys);
    files.apply(vec![
        ManagedConfigMergePlan {
            destination_path: path.to_owned(),
            replacement,
            original,
        },
        history.plan()?,
    ])
}

pub(super) fn materialize(
    context: &ModuleSupportMaterializationContext<'_>,
    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    let live = context
        .install_root
        .join("ShooterGame/Saved/Config/WindowsServer");
    let mut plans = Vec::new();
    let mut history = Snapshot::load(
        context.config_dir,
        &context.config_dir.join("instance.json"),
        context.module_id,
    )?;
    for (filename, game_ini) in [
        (ARK_GAME_INI_FILE, true),
        (ARK_GAME_USER_SETTINGS_FILE, false),
    ] {
        let source_path = context.config_dir.join(filename);
        let (source_bytes, source) = read_document(&source_path, context.module_id)?;
        if source_bytes.is_none() {
            return Err(materialization_error(
                context.module_id,
                &source_path,
                "rendered support file is missing".into(),
            ));
        }
        let destination_path = live.join(filename);
        let (original, mut existing) = read_document(&destination_path, context.module_id)?;
        if original.is_none() {
            existing.encoding = source.encoding;
            existing.crlf = source.crlf;
        }
        let owned = owned_keys(context.module_id, game_ini);
        let extra = extra_document(context.settings, game_ini)
            .map_err(|error| materialization_error(context.module_id, &source_path, error))?;
        let mut current_keys = history.keys(filename, Target::Config);
        current_keys.extend(extra.keys());
        let mut exact = history.keys(filename, Target::Live);
        exact.extend(current_keys.clone());
        let replacement = merge(existing, source, &owned, &exact, true).render();
        history.record(filename, Target::Live, current_keys);
        plans.push(ManagedConfigMergePlan {
            destination_path,
            replacement,
            original,
        });
    }
    plans.push(history.plan()?);
    files.apply(plans)
}

pub(super) fn prepare_cluster_directory(
    context: &ModuleSupportMaterializationContext<'_>,
) -> Result<(), StorageError> {
    let directory = app_core::ark_cluster::resolve_cluster_directory(
        context
            .settings
            .get("cluster_id")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        context
            .settings
            .get("cluster_directory")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        &app_core::ark_maps::primary_saves_dir(context.instance_id, context.saves_dir),
    )
    .map_err(|error| StorageError::InvalidModuleSetting {
        module_id: context.module_id.to_owned(),
        field: error.field.to_owned(),
        message: error.message.to_owned(),
    })?;
    if let Some(path) = directory {
        fs::create_dir_all(&path).map_err(|source| StorageError::CreatePath { path, source })?;
    }
    Ok(())
}
