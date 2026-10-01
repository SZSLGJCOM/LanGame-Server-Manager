use std::collections::BTreeMap;
use std::path::Path;

mod atlas;
mod cache;
mod mapping;
mod resources;
mod texture;

const MAX_SCHEMA_BYTES: usize = 1024 * 1024;
const MAX_FIELDS: usize = 280;
const MAX_OUTPUT_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Default)]
pub struct DstConfigurationIcons {
    pub icons: BTreeMap<String, Vec<u8>>,
    pub warnings: Vec<String>,
}

struct AtlasFiles {
    name: &'static str,
    xml: Vec<u8>,
    texture: Vec<u8>,
}

struct DecodedAtlas {
    files: AtlasFiles,
    atlas: atlas::Atlas,
    pixels: image::RgbaImage,
}

/// Prefer installed artwork, then retain validated native atlas pairs in the
/// app-data cache. No game artwork is embedded in the application. Cache failures
/// are warnings when usable icons can still be returned.
pub fn load_dst_configuration_icons(
    install_root: &Path,
    app_data_root: &Path,
    schema_json: &str,
) -> Result<DstConfigurationIcons, String> {
    let requested = requested_icons(schema_json)?;
    if requested.is_empty() {
        return Ok(DstConfigurationIcons::default());
    }
    let mut output = DstConfigurationIcons::default();
    let installed = resources::Resources::open(install_root).unwrap_or_else(|error| {
        output
            .warnings
            .push(format!("installed DST artwork: {error}"));
        None
    });
    let cached = resources::Resources::open_cache(app_data_root).unwrap_or_else(|error| {
        output.warnings.push(format!("cached DST artwork: {error}"));
        None
    });
    let mut output_bytes = 0usize;
    let mut native_files = Vec::new();
    let mut used_installation = false;
    let mut used_cache = false;
    for atlas_name in ["worldgen_customization", "worldsettings_customization"] {
        let fields: Vec<_> = requested
            .iter()
            .filter(|(_, icon)| icon.atlas == atlas_name)
            .collect();
        let from_install = read_atlas_source(
            installed.as_ref(),
            atlas_name,
            "installed",
            &mut output.warnings,
        );
        used_installation |= from_install.is_some();
        let Some(decoded) = from_install.or_else(|| {
            let decoded =
                read_atlas_source(cached.as_ref(), atlas_name, "cached", &mut output.warnings);
            used_cache |= decoded.is_some();
            decoded
        }) else {
            continue;
        };
        let mut encoded: BTreeMap<&str, Vec<u8>> = BTreeMap::new();
        for (field, icon) in fields {
            let Some(rectangle) = decoded.atlas.elements.get(icon.element) else {
                continue;
            };
            if !encoded.contains_key(icon.element) {
                let png = atlas::crop_icon(&decoded.pixels, *rectangle)
                    .map_err(|error| format!("DST icon `{}`: {error}", icon.element))?;
                encoded.insert(icon.element, png);
            }
            let png = &encoded[icon.element];
            output_bytes = output_bytes
                .checked_add(png.len())
                .filter(|size| *size <= MAX_OUTPUT_BYTES)
                .ok_or_else(|| {
                    String::from("DST configuration icons exceed the total PNG byte limit")
                })?;
            output.icons.insert(field.to_string(), png.clone());
        }
        native_files.push(decoded.files);
    }
    if used_cache
        && cached
            .as_ref()
            .is_some_and(|resources| resources.recovered_snapshot)
    {
        output.warnings.push(String::from(
            "using the previous complete DST icon cache after an interrupted update",
        ));
    }
    if output.icons.is_empty() && !output.warnings.is_empty() {
        return Err(output.warnings.join("; "));
    }
    if used_installation
        && !output.icons.is_empty()
        && let Err(error) = cache::write_snapshot(app_data_root, &native_files)
    {
        output
            .warnings
            .push(format!("DST artwork cache was not updated: {error}"));
    }
    Ok(output)
}

fn read_atlas_source(
    resources: Option<&resources::Resources>,
    name: &'static str,
    source: &str,
    warnings: &mut Vec<String>,
) -> Option<DecodedAtlas> {
    let resources = resources?;
    match decode_atlas(resources, name) {
        Ok(decoded) => decoded,
        Err(error) => {
            warnings.push(format!("{source} DST atlas `{name}`: {error}"));
            None
        }
    }
}

fn decode_atlas(
    resources: &resources::Resources,
    name: &'static str,
) -> Result<Option<DecodedAtlas>, String> {
    let xml = resources.read(name, "xml", atlas::MAX_ATLAS_XML_BYTES)?;
    let texture = resources.read(name, "tex", texture::MAX_TEXTURE_BYTES)?;
    let (xml, texture) = match (xml, texture) {
        (None, None) => return Ok(None),
        (Some(xml), Some(texture)) => (xml, texture),
        _ => return Err(String::from("XML and TEX must both be present")),
    };
    let atlas = atlas::parse_atlas(&xml)?;
    if atlas.texture_filename != format!("{name}.tex") {
        return Err(String::from("atlas references an unexpected texture"));
    }
    let pixels = texture::decode_texture(&texture)?;
    for rectangle in atlas.elements.values() {
        atlas::icon_rectangle(&pixels, *rectangle)?;
    }
    Ok(Some(DecodedAtlas {
        files: AtlasFiles { name, xml, texture },
        atlas,
        pixels,
    }))
}

fn requested_icons(schema_json: &str) -> Result<BTreeMap<String, mapping::Icon>, String> {
    if schema_json.len() > MAX_SCHEMA_BYTES {
        return Err(String::from("DST icon schema exceeds its byte limit"));
    }
    let schema: serde_json::Value = serde_json::from_str(schema_json)
        .map_err(|error| format!("invalid DST icon schema: {error}"))?;
    let properties = schema
        .get("properties")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| String::from("DST icon schema has no properties"))?;
    let mut requested = BTreeMap::new();
    let mut world_fields = 0;
    for (field, property) in properties {
        let section = property
            .get("x-lsgm-section")
            .and_then(serde_json::Value::as_str);
        if !matches!(
            section,
            Some("mastersettings" | "mastergen" | "cavessettings" | "cavesgen")
        ) {
            continue;
        }
        world_fields += 1;
        if world_fields > MAX_FIELDS || field.len() > 128 {
            return Err(String::from(
                "DST world icon fields exceed their count or name limit",
            ));
        }
        let Some(key) = property
            .get("x-lsgm-source-key")
            .and_then(serde_json::Value::as_str)
            .and_then(|key| key.strip_prefix("overrides."))
        else {
            continue;
        };
        if let Some(icon) = mapping::icon(key) {
            requested.insert(field.clone(), icon);
        }
    }
    Ok(requested)
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

#[cfg(test)]
mod cache_tests;
