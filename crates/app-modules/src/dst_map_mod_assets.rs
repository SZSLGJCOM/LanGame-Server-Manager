use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::{AtlasIcons, ReadBudget, atlas, checked_root, encode_png};

const MAX_MOD_SOURCES: usize = 128;
pub(super) const MAX_REGISTERED_ATLASES: usize = 512;
const MAX_MOD_TEXTURE_BYTES: usize = 4 * 1024 * 1024;
const MAX_MOD_ATLAS_BYTES: usize = 16 * 1024 * 1024;
const MAX_MOD_XML_BYTES: usize = 1024 * 1024;

/// Constructed by the host from the selected running shard's owned installation
/// and loaded Mod list. Never deserialize filesystem roots from an IPC request.
#[derive(Debug, Clone)]
pub struct DstMapModSource {
    pub name: String,
    pub root: PathBuf,
}

pub(super) struct ModAssets {
    pub(super) roots: BTreeMap<String, Result<PathBuf, String>>,
}

impl ModAssets {
    pub(super) fn new(sources: &[DstMapModSource]) -> Result<Self, String> {
        let mut bindings = BTreeMap::new();
        for source in sources {
            if !safe_component(&source.name) || source.name.len() > 128 {
                return Err(String::from("Invalid DST map Mod source name"));
            }
            if let Some(previous) = bindings.get_mut(&source.name) {
                // Even an accidental duplicate host binding must not pick one
                // of two possibly different deployed copies by iteration order.
                if *previous != Some(&source.root) {
                    *previous = None;
                }
            } else {
                bindings.insert(source.name.clone(), Some(&source.root));
            }
            if bindings.len() > MAX_MOD_SOURCES {
                return Err(String::from("DST map Mod sources exceed their item limit"));
            }
        }
        let roots = bindings
            .into_iter()
            .map(|(name, root)| {
                let checked = root
                    .ok_or_else(|| String::from("Mod has conflicting authorized source roots"))
                    .and_then(|root| checked_root(root));
                (name, checked)
            })
            .collect();
        Ok(Self { roots })
    }

    pub(super) fn resolve<'a>(
        &'a self,
        path: &'a str,
        extension: &str,
    ) -> Result<(&'a Path, &'a str), String> {
        // ../mods is an engine namespace, not an OS traversal operation. Strip
        // it before selecting the host-authorized exact Mod root.
        let (name, relative) = path
            .strip_prefix("../mods/")
            .and_then(|value| value.split_once('/'))
            .ok_or_else(|| String::from("Unsupported DST map Mod asset namespace"))?;
        if path.len() > 512 || !safe_component(name) || !safe_relative(relative, extension) {
            return Err(String::from("Unsafe DST map Mod asset path"));
        }
        let root = self
            .roots
            .get(name)
            .ok_or_else(|| {
                String::from("DST map asset belongs to a Mod not enabled for this shard")
            })?
            .as_ref()
            .map_err(Clone::clone)?;
        Ok((root, relative))
    }

    pub(super) fn texture(
        &self,
        path: &str,
        budget: &mut ReadBudget,
    ) -> Result<Option<Vec<u8>>, String> {
        let (root, relative) = self.resolve(path, ".tex")?;
        let Some(bytes) = budget.read(root, relative, MAX_MOD_TEXTURE_BYTES)? else {
            return Ok(None);
        };
        let pixels = budget.decode_noise(&bytes)?;
        encode_png(atlas::resize_image(pixels, 128)).map(Some)
    }

    pub(super) fn base_color(
        &self,
        path: &str,
        budget: &mut ReadBudget,
    ) -> Result<[u8; 4], String> {
        let (root, relative) = self.resolve(path, ".tex")?;
        let bytes = budget
            .read(root, relative, MAX_MOD_ATLAS_BYTES)?
            .ok_or_else(|| String::from("Mod base texture is not installed"))?;
        let pixels = budget.decode(&bytes)?;
        super::majority_base_color(&pixels)
    }

    pub(super) fn icons(
        &self,
        path: &str,
        requested: &BTreeSet<&str>,
        budget: &mut ReadBudget,
    ) -> Result<AtlasIcons, String> {
        let (root, relative) = self.resolve(path, ".xml")?;
        let xml = budget
            .read(root, relative, MAX_MOD_XML_BYTES)?
            .ok_or_else(|| String::from("registered Mod minimap atlas is not installed"))?;
        let requested_keys: BTreeMap<_, _> = requested
            .iter()
            .map(|name| (*name, atlas::map_element_key(name)))
            .collect();
        let wanted = requested_keys.values().cloned().collect();
        let (parsed, issues) = atlas::parse_selected_map_elements(&xml, &wanted)?;
        let mut result = AtlasIcons {
            icons: BTreeMap::new(),
            issues,
            declared: requested_keys
                .iter()
                .filter(|(_, key)| parsed.elements.contains_key(*key))
                .map(|(name, _)| (*name).to_string())
                .collect(),
        };
        if !safe_component(&parsed.texture_filename) || !parsed.texture_filename.ends_with(".tex") {
            result
                .issues
                .push("Mod atlas references an unsafe texture filename".into());
            return Ok(result);
        }
        if !requested_keys
            .values()
            .any(|key| parsed.elements.contains_key(key))
        {
            return Ok(result);
        }
        // Native atlases declare the TEX basename adjacent to their XML. The
        // existing XML parser rejects directories, URLs and external entities.
        let texture_path = relative.rsplit_once('/').map_or_else(
            || parsed.texture_filename.clone(),
            |(parent, _)| format!("{parent}/{}", parsed.texture_filename),
        );
        // Claim XML declarations before decoding: a broken selected atlas
        // must not borrow a later atlas with a different meaning for this key.
        let pixels = match budget
            .read(root, &texture_path, MAX_MOD_ATLAS_BYTES)
            .and_then(|bytes| {
                bytes.ok_or_else(|| String::from("registered Mod minimap texture is not installed"))
            })
            .and_then(|bytes| budget.decode_with_alpha(&bytes, parsed.straight_alpha))
        {
            Ok(pixels) => pixels,
            Err(error) => {
                result.issues.push(error);
                return Ok(result);
            }
        };
        for (name, key) in requested_keys {
            let Some(rectangle) = parsed.elements.get(&key) else {
                continue;
            };
            let icon = budget.crop_icon(&pixels, *rectangle).and_then(encode_png);
            match icon {
                Ok(icon) => {
                    result.icons.insert(name.to_string(), icon);
                }
                Err(error) => result
                    .issues
                    .push(format!("Mod atlas element {name}: {error}")),
            }
        }
        Ok(result)
    }
}

fn safe_relative(path: &str, extension: &str) -> bool {
    path.ends_with(extension)
        && path.split('/').count() <= 16
        && path.split('/').all(safe_component)
}

pub(super) fn safe_component(name: &str) -> bool {
    let device = name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    !name.is_empty()
        && name.len() <= 128
        && !matches!(name, "." | "..")
        && !name.ends_with(['.', ' '])
        && !name.chars().any(|ch| {
            ch.is_control() || matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
        })
        && !matches!(
            device.as_str(),
            "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
        )
        && !device
            .strip_prefix("COM")
            .or_else(|| device.strip_prefix("LPT"))
            .is_some_and(|suffix| {
                matches!(
                    suffix,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
}

#[cfg(test)]
#[path = "dst_map_mod_assets_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "dst_map_mod_assets_native_tests.rs"]
mod native_tests;
