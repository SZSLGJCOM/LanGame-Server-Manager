use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use image::{ExtendedColorType, ImageEncoder, RgbaImage};

use crate::dst_configuration_icons::{atlas, resources, texture};

#[path = "dst_map_mod_assets.rs"]
mod mod_assets;
pub use mod_assets::DstMapModSource;
use mod_assets::ModAssets;

#[path = "dst_map_terrain_edges.rs"]
mod terrain_edges;
pub use terrain_edges::DstMapTerrainEdge;

#[path = "dst_map_icon_fallback.rs"]
mod icon_fallback;
pub use icon_fallback::{DstMapIconFallback, DstMapIconFallbackKind};

/// An actual entity's declared inventory image, approved by the host only when
/// every landmark sharing this icon agrees and no player avatar uses the key.
#[derive(Debug, Clone)]
pub struct DstMapEntityInventoryIcon {
    pub icon: String,
    pub atlas: String,
    pub element: String,
}

#[derive(Clone, Copy, Default)]
pub struct DstMapIconSources<'a> {
    pub registered_atlases: &'a [String],
    pub entity_inventory: &'a [DstMapEntityInventoryIcon],
}

const MAX_TEXTURES: usize = 128;
const MAX_ICONS: usize = 512;
const MAX_TEXTURE_BYTES: usize = 1024 * 1024;
const MAX_ATLAS_BYTES: usize = 8 * 1024 * 1024;
const MAX_XML_BYTES: usize = 1024 * 1024;
const MAX_PNG_BYTES: usize = 128 * 1024;
const MAX_OUTPUT_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Default)]
pub struct DstMapAssets {
    pub textures: BTreeMap<String, Vec<u8>>,
    pub icons: BTreeMap<String, Vec<u8>>,
    pub terrain_base_color: Option<[u8; 4]>,
    pub terrain_base_colors: BTreeMap<String, [u8; 4]>,
    pub terrain_edges: Vec<DstMapTerrainEdge>,
    pub issues: Vec<String>,
    pub icon_fallbacks: Vec<DstMapIconFallback>,
}

#[derive(Default)]
struct AtlasIcons {
    icons: BTreeMap<String, Vec<u8>>,
    issues: Vec<String>,
    declared: BTreeSet<String>,
}

/// Decode only requested installed artwork. No game artwork is embedded,
/// downloaded, extracted to disk, or reused across installation versions.
/// Call on a blocking worker; noise/icon PNG dimensions are 128/64 pixels.
/// Edge atlases retain their bounded source resolution and 512 KiB PNG cap.
pub fn load_dst_map_assets(
    install_root: &Path,
    mod_sources: &[DstMapModSource],
    icon_sources: DstMapIconSources<'_>,
    texture_paths: &[String],
    base_paths: &[String],
    icon_names: &[String],
    edge_requests: &[(String, String)],
) -> Result<DstMapAssets, String> {
    let registered_atlases = icon_sources.registered_atlases;
    let requested_textures: BTreeSet<_> = texture_paths.iter().map(String::as_str).collect();
    let requested_icons: BTreeSet<_> = icon_names.iter().map(String::as_str).collect();
    let requested_bases: BTreeSet<_> = base_paths.iter().map(String::as_str).collect();
    let atlas_count = registered_atlases.iter().collect::<BTreeSet<_>>().len();
    let root = checked_root(install_root)?;
    let mods = ModAssets::new(mod_sources)?;
    let mut budget = ReadBudget::default();
    let mut output = DstMapAssets::default();
    // Artwork limits bound each category. A larger world may lose some art,
    // but must retain successful material from other categories.
    if requested_textures.len() > MAX_TEXTURES {
        output.issues.push(format!(
            "DST map terrain texture item limit ({MAX_TEXTURES}) reached; additional textures were omitted."
        ));
    }
    if requested_icons.len() > MAX_ICONS {
        output.issues.push(format!(
            "DST map icon item limit ({MAX_ICONS}) reached; additional icons were omitted."
        ));
    }
    if requested_bases.len() > MAX_TEXTURES {
        output.issues.push(format!(
            "DST map base texture item limit ({MAX_TEXTURES}) reached; additional bases use the native base."
        ));
    }
    match read_terrain_base_color(&root, &mut budget) {
        Ok(color) => output.terrain_base_color = Some(color),
        Err(error) => output
            .issues
            .push(format!("DST map base material: {error}")),
    }
    let mut output_bytes = 0usize;
    for path in requested_textures.into_iter().take(MAX_TEXTURES) {
        let result = if path.starts_with("../mods/") {
            mods.texture(path, &mut budget)
        } else if valid_texture_path(path) {
            read_texture(&root, path, &mut budget)
        } else {
            Err(String::from(
                "Unsupported DST map texture source; only native or enabled Mod map textures can be read.",
            ))
        };
        match result {
            Ok(Some(png)) => {
                if let Err(error) = add_output_bytes(&mut output_bytes, png.len()) {
                    output
                        .issues
                        .push(format!("DST map texture {path}: {error}"));
                    continue;
                }
                output.textures.insert(path.to_string(), png);
            }
            Ok(None) => output
                .issues
                .push(format!("DST map texture is not installed: {path}")),
            Err(error) => output
                .issues
                .push(format!("DST map texture {path}: {error}")),
        }
    }
    for path in requested_bases.into_iter().take(MAX_TEXTURES) {
        let result = if path == "levels/tiles/map_edge.tex" {
            // The official base was already decoded under the same budget.
            output
                .terrain_base_color
                .ok_or_else(|| String::from("native base material is unavailable"))
        } else if path.starts_with("../mods/") {
            mods.base_color(path, &mut budget)
        } else if path
            .strip_prefix("levels/tiles/")
            .is_some_and(|name| atlas::valid_asset_name(name) && name.ends_with(".tex"))
        {
            read_resource(&root, path, MAX_ATLAS_BYTES, &mut budget)
                .and_then(|bytes| {
                    bytes.ok_or_else(|| String::from("base texture is not installed"))
                })
                .and_then(|bytes| budget.decode(&bytes))
                .and_then(|pixels| majority_base_color(&pixels))
        } else {
            Err(String::from("Unsupported DST map base texture source"))
        };
        match result {
            Ok(color) => {
                output.terrain_base_colors.insert(path.to_string(), color);
            }
            Err(error) => output.issues.push(format!(
                "DST map base {path}: {error}; using the native base."
            )),
        }
    }
    if atlas_count > mod_assets::MAX_REGISTERED_ATLASES {
        // Truncating atlas registration would invent a winner for later keys.
        // Omit only icon artwork; already decoded terrain remains usable.
        output.issues.push(format!(
            "DST minimap atlas item limit ({}) exceeded; icon artwork was omitted.",
            mod_assets::MAX_REGISTERED_ATLASES
        ));
        terrain_edges::load(
            &root,
            &mods,
            edge_requests,
            &mut budget,
            &mut output_bytes,
            &mut output,
        );
        return Ok(output);
    }
    let mut remaining = BTreeSet::new();
    for name in requested_icons.into_iter().take(MAX_ICONS) {
        if atlas::valid_element_name(name) && {
            let key = atlas::map_element_key(name);
            key.ends_with(".png") || key.ends_with(".tex")
        } {
            remaining.insert(name);
        } else {
            output.issues.push(String::from(
                "Unsupported DST map icon name; only native atlas element names can be read.",
            ));
        }
    }
    let mut unclaimed = remaining.clone();
    let mut declared = BTreeSet::new();
    let mut uncertain = false;
    let mut visited = BTreeSet::new();
    // Native MiniMap keeps the first registered declaration. Select metadata
    // before decoding; PNG/TEX AB/BA controls confirm both exact namespaces.
    for path in icon_atlas_paths(registered_atlases) {
        if unclaimed.is_empty() {
            break;
        }
        if !visited.insert(path) {
            continue;
        }
        let result = if let Some(suffix) = native_atlas_suffix(path) {
            let required = registered_atlases
                .iter()
                .any(|registered| registered == path);
            read_icons(&root, suffix, &unclaimed, required, &mut budget)
        } else {
            mods.icons(path, &unclaimed, &mut budget)
        };
        match result {
            Ok(atlas_icons) => {
                // Reserve the winning declaration even if its texture, crop or
                // budget fails. Later atlases cannot change the selected source.
                for name in &atlas_icons.declared {
                    unclaimed.remove(name.as_str());
                }
                declared.extend(atlas_icons.declared);
                output.issues.extend(
                    atlas_icons
                        .issues
                        .into_iter()
                        .map(|issue| format!("DST registered minimap atlas {path}: {issue}")),
                );
                for (name, png) in atlas_icons.icons {
                    if let Err(error) = add_output_bytes(&mut output_bytes, png.len()) {
                        output.issues.push(format!("DST map icon {name}: {error}"));
                        continue;
                    }
                    remaining.remove(name.as_str());
                    output.icons.insert(name, png);
                }
            }
            Err(error) => {
                uncertain = true;
                output
                    .issues
                    .push(format!("DST registered minimap atlas {path}: {error}"));
                // An unreadable earlier XML may contain any remaining key.
                // Treating it as absent would silently guess a later winner.
                break;
            }
        }
    }
    if !uncertain {
        let pending: BTreeSet<_> = remaining
            .iter()
            .copied()
            .filter(|name| !declared.contains(*name))
            .collect();
        let completed = icon_fallback::load(&root, &mods, icon_sources, &pending, &mut budget);
        output.issues.extend(completed.issues);
        for (name, png) in completed.icons {
            if let Err(error) = add_output_bytes(&mut output_bytes, png.len()) {
                output.issues.push(format!("DST map icon {name}: {error}"));
                continue;
            }
            remaining.remove(name.as_str());
            output.icons.insert(name, png);
        }
        output.icon_fallbacks = completed
            .provenance
            .into_iter()
            .filter(|source| output.icons.contains_key(&source.icon))
            .collect();
    }
    for name in remaining {
        output
            .issues
            .push(format!("DST map icon is unavailable: {name}"));
    }
    // New edge artwork may consume only the shared budget left after the
    // previously supported texture/icon categories and their native winners.
    terrain_edges::load(
        &root,
        &mods,
        edge_requests,
        &mut budget,
        &mut output_bytes,
        &mut output,
    );
    Ok(output)
}

fn native_atlas_suffix(path: &str) -> Option<&'static str> {
    match path {
        "minimap/minimap_data1.xml" => Some("1"),
        "minimap/minimap_data2.xml" => Some("2"),
        "minimap/minimap_data.xml" => Some(""),
        _ => None,
    }
}

fn icon_atlas_paths(registered: &[String]) -> impl Iterator<Item = &str> {
    // An observed native registration makes the complete observed order
    // authoritative. Older callers without that report retain the official
    // data1/data2 defaults and the existing single-atlas installation fallback.
    let defaults: &[&str] = if registered
        .iter()
        .any(|path| native_atlas_suffix(path).is_some())
    {
        &[]
    } else {
        &[
            "minimap/minimap_data1.xml",
            "minimap/minimap_data2.xml",
            "minimap/minimap_data.xml",
        ]
    };
    defaults
        .iter()
        .copied()
        .chain(registered.iter().map(String::as_str))
}

fn valid_texture_path(path: &str) -> bool {
    if path == "images/minimap_paper.tex" {
        return true;
    }
    let Some(name) = path.strip_prefix("levels/textures/") else {
        return false;
    };
    atlas::valid_asset_name(name)
        && name.ends_with(".tex")
        && (name.starts_with("mini_") || name.ends_with("_mini.tex"))
}

fn checked_root(install_root: &Path) -> Result<PathBuf, String> {
    if !install_root.is_absolute() {
        return Err(String::from(
            "DST map artwork requires an absolute installation path",
        ));
    }
    for ancestor in install_root.ancestors() {
        let metadata = fs::symlink_metadata(ancestor)
            .map_err(|error| format!("failed to inspect DST artwork installation: {error}"))?;
        if !metadata.is_dir() || resources::is_reparse(&metadata) {
            return Err(String::from(
                "DST map artwork installation contains a reparse point or is not a directory",
            ));
        }
    }
    fs::canonicalize(install_root)
        .map_err(|error| format!("failed to resolve DST artwork installation: {error}"))
}

#[derive(Default)]
struct ReadBudget {
    source_bytes: usize,
    decoded_pixels: usize,
    crop_source_pixels: usize,
}

impl ReadBudget {
    const MAX_SOURCE_BYTES: usize = 128 * 1024 * 1024;
    const MAX_DECODED_PIXELS: usize = 64 * 1024 * 1024;
    const MAX_CROP_SOURCE_PIXELS: usize = 64 * 1024 * 1024;

    fn read(
        &mut self,
        root: &Path,
        relative: &str,
        limit: usize,
    ) -> Result<Option<Vec<u8>>, String> {
        let remaining = Self::MAX_SOURCE_BYTES.saturating_sub(self.source_bytes);
        if remaining <= 1 {
            return Err(String::from(
                "DST map artwork exceeds its cumulative source byte limit",
            ));
        }
        // The bounded reader probes one extra byte to detect a growing file.
        let limit = limit.min(remaining - 1);
        let path = root.join(relative);
        let Some(file) = resources::open_checked_file(root, &path, limit as u64)? else {
            return Ok(None);
        };
        self.source_bytes += limit + 1;
        // Retain the reservation on failure: a concurrently growing or failing
        // source may already have consumed this entire I/O allowance.
        let bytes = resources::read_limited(file, limit)?;
        self.source_bytes -= limit + 1 - bytes.len();
        Ok(Some(bytes))
    }

    fn decode(&mut self, bytes: &[u8]) -> Result<RgbaImage, String> {
        self.decode_with_alpha(bytes, false)
    }

    fn decode_with_alpha(
        &mut self,
        bytes: &[u8],
        straight_alpha: bool,
    ) -> Result<RgbaImage, String> {
        self.reserve_decode(bytes)?;
        texture::decode_texture_with_alpha(bytes, straight_alpha)
    }

    fn decode_noise(&mut self, bytes: &[u8]) -> Result<RgbaImage, String> {
        self.reserve_decode(bytes)?;
        texture::decode_noise_texture(bytes)
    }

    fn reserve_decode(&mut self, bytes: &[u8]) -> Result<(), String> {
        if bytes.get(..4) == Some(b"KTEX")
            && let Some(header) = bytes.get(8..12)
        {
            let pixels = usize::from(u16::from_le_bytes([header[0], header[1]]))
                * usize::from(u16::from_le_bytes([header[2], header[3]]));
            self.decoded_pixels = self
                .decoded_pixels
                .checked_add(pixels)
                .filter(|value| *value <= Self::MAX_DECODED_PIXELS)
                .ok_or_else(|| {
                    String::from("DST map artwork exceeds its cumulative decoded pixel limit")
                })?;
        }
        Ok(())
    }

    fn crop_icon(
        &mut self,
        pixels: &RgbaImage,
        rectangle: atlas::Rectangle,
    ) -> Result<RgbaImage, String> {
        let (_, _, width, height) = atlas::map_icon_rectangle(pixels, rectangle)?;
        let source_pixels = (width as usize)
            .checked_mul(height as usize)
            .ok_or_else(|| String::from("DST map icon source pixel count overflows"))?;
        let total = self
            .crop_source_pixels
            .checked_add(source_pixels)
            .filter(|value| *value <= Self::MAX_CROP_SOURCE_PIXELS)
            .ok_or_else(|| {
                String::from("DST map artwork exceeds its cumulative crop source pixel limit")
            })?;
        // Cached/decoded atlases may be sampled repeatedly by many different
        // or overlapping UV entries; the decoded-source budget alone cannot
        // bound crop work. Reserve before sampling and retain on later failure.
        self.crop_source_pixels = total;
        atlas::crop_map_icon(pixels, rectangle)
    }
}

fn read_resource(
    root: &Path,
    relative: &str,
    limit: usize,
    budget: &mut ReadBudget,
) -> Result<Option<Vec<u8>>, String> {
    budget.read(root, &format!("data/{relative}"), limit)
}

fn read_texture(
    root: &Path,
    relative: &str,
    budget: &mut ReadBudget,
) -> Result<Option<Vec<u8>>, String> {
    let Some(bytes) = read_resource(root, relative, MAX_TEXTURE_BYTES, budget)? else {
        return Ok(None);
    };
    let pixels = if relative == "images/minimap_paper.tex" {
        budget.decode(&bytes)?
    } else {
        budget.decode_noise(&bytes)?
    };
    encode_png(atlas::resize_image(pixels, 128)).map(Some)
}

fn read_terrain_base_color(root: &Path, budget: &mut ReadBudget) -> Result<[u8; 4], String> {
    let bytes = read_resource(root, "levels/tiles/map_edge.tex", MAX_ATLAS_BYTES, budget)?
        .ok_or_else(|| String::from("native map_edge texture is not installed"))?;
    let pixels = budget.decode(&bytes)?;
    majority_base_color(&pixels)
}

fn majority_base_color(pixels: &RgbaImage) -> Result<[u8; 4], String> {
    // Native minimap.ksh multiplies noise RGB by this base atlas. In the
    // inspected official atlas, element 01 is a uniformly opaque interior;
    // its color also covers 92% of all fully opaque atlas pixels. Derive the
    // strictly dominant opaque color instead of hardcoding a version's tint.
    // Boyer-Moore plus verification needs constant memory even for hostile art.
    let mut candidate = [0; 3];
    let mut votes = 0usize;
    let mut opaque_count = 0usize;
    for pixel in pixels.pixels().filter(|pixel| pixel[3] == 255) {
        let rgb = [pixel[0], pixel[1], pixel[2]];
        opaque_count += 1;
        if votes == 0 {
            candidate = rgb;
            votes = 1;
        } else if candidate == rgb {
            votes += 1;
        } else {
            votes -= 1;
        }
    }
    let matches = pixels
        .pixels()
        .filter(|pixel| pixel[3] == 255 && [pixel[0], pixel[1], pixel[2]] == candidate)
        .count();
    if opaque_count == 0 || matches <= opaque_count / 2 {
        return Err(String::from(
            "base texture has no uniform majority interior color",
        ));
    }
    Ok([candidate[0], candidate[1], candidate[2], 255])
}

fn read_icons(
    root: &Path,
    suffix: &str,
    requested: &BTreeSet<&str>,
    required: bool,
    budget: &mut ReadBudget,
) -> Result<AtlasIcons, String> {
    let mut output = AtlasIcons::default();
    let Some(xml) = read_resource(
        root,
        &format!("minimap/minimap_data{suffix}.xml"),
        MAX_XML_BYTES,
        budget,
    )?
    else {
        if required {
            return Err(String::from(
                "registered native minimap atlas is not installed",
            ));
        }
        return Ok(output);
    };
    let wanted = requested
        .iter()
        .map(|name| atlas::map_element_key(name))
        .collect();
    let (parsed, issues) = atlas::parse_selected_map_elements(&xml, &wanted)?;
    output.issues = issues;
    output.declared = requested
        .iter()
        .filter(|name| parsed.elements.contains_key(&atlas::map_element_key(name)))
        .map(|name| (*name).to_string())
        .collect();
    let texture_name = format!("minimap_atlas{suffix}.tex");
    if parsed.texture_filename != texture_name {
        output
            .issues
            .push(String::from("atlas references an unexpected map texture"));
        return Ok(output);
    }
    if !requested
        .iter()
        .any(|name| parsed.elements.contains_key(&atlas::map_element_key(name)))
    {
        return Ok(output);
    }
    let pixels = match read_resource(
        root,
        &format!("minimap/{texture_name}"),
        MAX_ATLAS_BYTES,
        budget,
    )
    .and_then(|bytes| bytes.ok_or_else(|| String::from("atlas texture is not installed")))
    .and_then(|bytes| budget.decode_with_alpha(&bytes, parsed.straight_alpha))
    {
        Ok(pixels) => pixels,
        Err(error) => {
            output.issues.push(error);
            return Ok(output);
        }
    };
    for name in requested {
        let Some(rectangle) = parsed.elements.get(&atlas::map_element_key(name)) else {
            continue;
        };
        let icon = budget.crop_icon(&pixels, *rectangle).and_then(encode_png);
        match icon {
            Ok(icon) => {
                output.icons.insert((*name).to_string(), icon);
            }
            Err(error) => output.issues.push(format!("atlas element {name}: {error}")),
        }
    }
    Ok(output)
}

fn encode_png(pixels: RgbaImage) -> Result<Vec<u8>, String> {
    encode_png_with_limit(pixels, MAX_PNG_BYTES)
}

fn encode_png_with_limit(pixels: RgbaImage, limit: usize) -> Result<Vec<u8>, String> {
    let mut output = LimitedPngWriter(Vec::new(), limit);
    image::codecs::png::PngEncoder::new(&mut output)
        .write_image(
            pixels.as_raw(),
            pixels.width(),
            pixels.height(),
            ExtendedColorType::Rgba8,
        )
        .map_err(|error| format!("failed to encode DST map PNG: {error}"))?;
    Ok(output.0)
}

fn add_output_bytes(total: &mut usize, bytes: usize) -> Result<(), String> {
    *total = total
        .checked_add(bytes)
        .filter(|value| *value <= MAX_OUTPUT_BYTES)
        .ok_or_else(|| String::from("DST map artwork exceeds its output byte limit"))?;
    Ok(())
}

struct LimitedPngWriter(Vec<u8>, usize);

impl Write for LimitedPngWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.1.saturating_sub(self.0.len()) {
            return Err(io::Error::other("DST map PNG exceeds its byte limit"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
#[path = "dst_map_assets_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "dst_map_terrain_base_tests.rs"]
mod terrain_base_tests;

#[cfg(test)]
#[path = "dst_map_icon_selection_tests.rs"]
mod icon_selection_tests;

#[cfg(test)]
#[path = "dst_map_selected_elements_tests.rs"]
mod selected_elements_tests;
