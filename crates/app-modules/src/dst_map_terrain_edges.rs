use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use image::RgbaImage;

use super::{
    DstMapAssets, ModAssets, ReadBudget, add_output_bytes, atlas, encode_png_with_limit,
    read_resource,
};

const MAX_EDGE_PNG_BYTES: usize = 512 * 1024;

type TerrainEdgeRegions = ([f64; 4], BTreeMap<u8, [f64; 4]>);

/// Content-identified image-space regions. Numeric atlas names are not used as
/// an adjacency table, and the observed explicit texture is never substituted.
#[derive(Debug, Clone)]
pub struct DstMapTerrainEdge {
    pub atlas: String,
    pub texture: String,
    pub image: Vec<u8>,
    pub opaque: [f64; 4],
    pub samples: BTreeMap<u8, [f64; 4]>,
}

pub(super) fn load(
    root: &Path,
    mods: &ModAssets,
    requests: &[(String, String)],
    budget: &mut ReadBudget,
    output_bytes: &mut usize,
    output: &mut DstMapAssets,
) {
    let requested: BTreeSet<_> = requests.iter().collect();
    if requested.len() > super::MAX_TEXTURES {
        output.issues.push(format!(
            "DST map terrain edge item limit ({}) reached; additional edges were omitted.",
            super::MAX_TEXTURES
        ));
    }
    for (atlas, texture) in requested.into_iter().take(super::MAX_TEXTURES) {
        let result = material(root, mods, atlas, texture, budget).and_then(|edge| {
            add_output_bytes(output_bytes, edge.image.len())?;
            Ok(edge)
        });
        match result {
            Ok(edge) => output.terrain_edges.push(edge),
            Err(error) => output.issues.push(format!(
                "DST map terrain edges {atlas} + {texture}: {error}; native noise/base retained."
            )),
        }
    }
}

fn source(
    root: &Path,
    mods: &ModAssets,
    path: &str,
    extension: &str,
    cap: usize,
    budget: &mut ReadBudget,
) -> Result<Vec<u8>, String> {
    let bytes = if path.starts_with("../mods/") {
        let (mod_root, relative) = mods.resolve(path, extension)?;
        budget.read(mod_root, relative, cap)?
    } else if path
        .strip_prefix("levels/tiles/")
        .is_some_and(|name| atlas::valid_asset_name(name) && name.ends_with(extension))
    {
        read_resource(root, path, cap, budget)?
    } else {
        return Err("Unsupported terrain edge source".into());
    };
    bytes.ok_or_else(|| "Terrain edge source is not installed".into())
}

fn material(
    root: &Path,
    mods: &ModAssets,
    atlas_path: &str,
    texture_path: &str,
    budget: &mut ReadBudget,
) -> Result<DstMapTerrainEdge, String> {
    let xml = source(root, mods, atlas_path, ".xml", super::MAX_XML_BYTES, budget)?;
    let parsed = atlas::parse_terrain_atlas(&xml)?;
    if parsed.elements.len() != 48 {
        return Err(
            "Terrain edge atlas does not contain a complete 47-shape set and opaque region".into(),
        );
    }
    let bytes = source(root, mods, texture_path, ".tex", 16 * 1024 * 1024, budget)?;
    let pixels = budget.decode_with_alpha(&bytes, parsed.straight_alpha)?;
    let (opaque, samples) = classify(&pixels, &parsed, budget)?;
    let image = encode_png_with_limit(pixels, MAX_EDGE_PNG_BYTES)?;
    Ok(DstMapTerrainEdge {
        atlas: atlas_path.into(),
        texture: texture_path.into(),
        image,
        opaque,
        samples,
    })
}

fn absorbed_corners(mask: u8) -> u8 {
    let mut result = mask;
    for (sides, corner) in [(2 | 8, 1), (2 | 16, 4), (64 | 8, 32), (64 | 16, 128)] {
        if mask & sides != 0 {
            result |= corner;
        }
    }
    result
}

fn classify(
    pixels: &RgbaImage,
    parsed: &atlas::Atlas,
    budget: &mut ReadBudget,
) -> Result<TerrainEdgeRegions, String> {
    if parsed.elements.len() != 48 {
        return Err(
            "Terrain edge atlas does not contain a complete 47-shape set and opaque region".into(),
        );
    }
    let mut opaque = None;
    let mut empty = 0;
    let mut samples = BTreeMap::new();
    for rectangle in parsed.elements.values() {
        let (left, top, width, height) = atlas::map_icon_rectangle(pixels, *rectangle)?;
        if width < 32 || height < 32 {
            return Err("Terrain edge region is too small for unambiguous alpha probes".into());
        }
        let area = (width as usize)
            .checked_mul(height as usize)
            .ok_or("Terrain edge region overflows")?;
        budget.crop_source_pixels = budget
            .crop_source_pixels
            .checked_add(area)
            .filter(|total| *total <= ReadBudget::MAX_CROP_SOURCE_PIXELS)
            .ok_or("Terrain edge regions exceed their cumulative sampling pixel limit")?;
        let mut full = true;
        let mut transparent = true;
        for y in top..top + height {
            for x in left..left + width {
                let alpha = pixels.get_pixel(x, y)[3];
                full &= alpha == 255;
                transparent &= alpha == 0;
            }
        }
        let uv = atlas::terrain_uv(*rectangle);
        if full {
            if opaque.replace(uv).is_some() {
                return Err("Terrain edge atlas has ambiguous opaque regions".into());
            }
            continue;
        }
        empty += usize::from(transparent);
        let mut identified = None;
        // Multiple boundary windows must independently agree. Artistic detail
        // near a region's center must not become a false ninth topology bit.
        for inset in [1.0 / 64.0, 1.0 / 32.0, 3.0 / 64.0] {
            let mut mask = 0u8;
            for (bit, (gx, gy)) in [
                (0, 0),
                (1, 0),
                (2, 0),
                (0, 1),
                (2, 1),
                (0, 2),
                (1, 2),
                (2, 2),
            ]
            .into_iter()
            .enumerate()
            {
                let positions = [inset, 0.5, 1.0 - inset];
                let cx = (positions[gx] * f64::from(width - 1)).round() as u32;
                let cy = (positions[gy] * f64::from(height - 1)).round() as u32;
                let rx = (f64::from(width) / 64.0).round().max(1.0) as u32;
                let ry = (f64::from(height) / 64.0).round().max(1.0) as u32;
                let (x0, x1) = (cx.saturating_sub(rx), (cx + rx + 1).min(width));
                let (y0, y1) = (cy.saturating_sub(ry), (cy + ry + 1).min(height));
                let probe_pixels = ((x1 - x0) as usize) * ((y1 - y0) as usize);
                budget.crop_source_pixels = budget
                    .crop_source_pixels
                    .checked_add(probe_pixels)
                    .filter(|total| *total <= ReadBudget::MAX_CROP_SOURCE_PIXELS)
                    .ok_or("Terrain edge probes exceed their cumulative sampling pixel limit")?;
                let mut sum = 0u64;
                for y in y0..y1 {
                    for x in x0..x1 {
                        sum += u64::from(pixels.get_pixel(left + x, top + y)[3]);
                    }
                }
                let scale = u64::from(x1 - x0) * u64::from(y1 - y0) * 255;
                if sum * 8 >= scale * 7 {
                    mask |= 1 << bit;
                } else if sum * 8 > scale {
                    return Err("Terrain edge alpha probe is ambiguous".into());
                }
            }
            if absorbed_corners(mask) != mask || identified.is_some_and(|previous| previous != mask)
            {
                return Err(
                    "Terrain edge boundary probes do not form a stable nine-sample shape".into(),
                );
            }
            identified = Some(mask);
        }
        let mask = identified.ok_or("Terrain edge has no identified boundary shape")?;
        if samples.insert(mask, uv).is_some() {
            return Err("Terrain edge atlas has duplicate alpha shapes".into());
        }
    }
    let expected: BTreeSet<_> = (0..=u8::MAX).map(absorbed_corners).collect();
    if samples.keys().copied().collect::<BTreeSet<_>>() != expected || empty != 1 {
        return Err("Terrain edge atlas is missing an unambiguous complete 47-shape set".into());
    }
    Ok((
        opaque.ok_or("Terrain edge atlas has no fully opaque region")?,
        samples,
    ))
}

#[cfg(test)]
#[path = "dst_map_terrain_edges_tests.rs"]
mod tests;
