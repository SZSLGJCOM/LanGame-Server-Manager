use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};

use image::{ExtendedColorType, GenericImageView, ImageEncoder, RgbaImage};
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};

pub(super) const MAX_ATLAS_XML_BYTES: usize = 1024 * 1024;
const MAX_ELEMENTS: usize = 2048;
const MAX_ALPHA_TEXT_BYTES: usize = 32;
const MAX_ICON_DIMENSION: u32 = 256;
const MAX_MAP_SOURCE_DIMENSION: u32 = 4096;
const MAX_MAP_ICON_DIMENSION: u32 = 64;
const MAX_ICON_PNG_BYTES: usize = 512 * 1024;

pub(crate) struct Atlas {
    pub texture_filename: String,
    pub elements: BTreeMap<String, Rectangle>,
    pub element_names: BTreeMap<String, String>,
    pub straight_alpha: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Rectangle {
    u1: f64,
    u2: f64,
    v1: f64,
    v2: f64,
}

pub(crate) fn parse_atlas(bytes: &[u8]) -> Result<Atlas, String> {
    parse_atlas_elements(bytes, AtlasUse::Configuration).map(|(atlas, _)| atlas)
}

pub(crate) fn parse_atlas_with_issues(bytes: &[u8]) -> Result<(Atlas, Vec<String>), String> {
    parse_atlas_elements(bytes, AtlasUse::Map)
}

/// Parse the complete registered or explicitly declared map atlas, but only
/// interpret UVs for the requested normalized keys. Unselected rectangles are
/// never exposed as valid artwork; their structure, names and attributes still
/// participate in full-document validation.
pub(crate) fn parse_selected_map_elements(
    bytes: &[u8],
    wanted: &BTreeSet<String>,
) -> Result<(Atlas, Vec<String>), String> {
    parse_atlas_elements(bytes, AtlasUse::SelectedMap(wanted))
}

/// A terrain render layer supplies its actual texture separately. The XML's
/// filename is bounded metadata, never authority for an additional file read.
pub(crate) fn parse_terrain_atlas(bytes: &[u8]) -> Result<Atlas, String> {
    let (atlas, issues) = parse_atlas_elements(bytes, AtlasUse::Terrain)?;
    if !issues.is_empty() {
        return Err(String::from(
            "Terrain atlas has ambiguous or unnamed elements",
        ));
    }
    Ok(atlas)
}

pub(crate) fn parse_atlas_for_map_index(
    bytes: &[u8],
    wanted: &BTreeSet<String>,
) -> Result<Option<Atlas>, String> {
    // Index completeness concerns all logical keys, including duplicates.
    // Unrequested UI/terrain entries can have unusable UVs or engine texture
    // paths; none enter file reads or crops. Validate their complete structure,
    // attribute bounds and keys, then validate UVs/paths only for candidates.
    let (atlas, _) = parse_atlas_elements(bytes, AtlasUse::Index(wanted))?;
    if !wanted.iter().any(|key| atlas.elements.contains_key(key)) {
        return Ok(None);
    }
    validate_texture_filename(&atlas.texture_filename)?;
    Ok(Some(atlas))
}

#[derive(Clone, Copy)]
enum AtlasUse<'a> {
    Configuration,
    Map,
    SelectedMap(&'a BTreeSet<String>),
    Terrain,
    Index(&'a BTreeSet<String>),
}

fn validate_texture_filename(filename: &str) -> Result<(), String> {
    if !valid_asset_name(filename) || !filename.ends_with(".tex") {
        return Err(String::from("DST atlas has an invalid texture filename"));
    }
    Ok(())
}

fn parse_atlas_elements(
    bytes: &[u8],
    purpose: AtlasUse<'_>,
) -> Result<(Atlas, Vec<String>), String> {
    let map_compatibility = !matches!(purpose, AtlasUse::Configuration);
    let selected_keys = match purpose {
        AtlasUse::SelectedMap(keys) | AtlasUse::Index(keys) => Some(keys),
        _ => None,
    };
    if bytes.len() > MAX_ATLAS_XML_BYTES {
        return Err(String::from("DST atlas XML exceeds its byte limit"));
    }
    let xml = std::str::from_utf8(bytes).map_err(|_| String::from("DST atlas XML is not UTF-8"))?;
    let mut reader = Reader::from_str(xml);
    let mut stack: Vec<String> = Vec::new();
    let mut texture_filename = None;
    let mut elements = BTreeMap::new();
    let mut element_names = BTreeMap::new();
    let mut issues = Vec::new();
    let mut element_count = 0;
    let mut straight_alpha = false;
    let mut alpha_seen = false;
    let mut alpha_text = String::new();
    let mut root_seen = false;
    loop {
        let event = reader
            .read_event()
            .map_err(|error| format!("invalid DST atlas XML: {error}"))?;
        let is_empty = matches!(event, Event::Empty(_));
        match event {
            Event::Start(element) | Event::Empty(element) => {
                let name = element.name();
                let name = name.as_ref();
                match (stack.last().map(String::as_str), name) {
                    (None, "Atlas") if !root_seen => {
                        root_seen = true;
                    }
                    (Some("Atlas"), "Texture") if texture_filename.is_none() => {
                        let attributes = attributes(&element)?;
                        let filename = required(&attributes, "filename")?;
                        if !matches!(purpose, AtlasUse::Index(_) | AtlasUse::Terrain) {
                            validate_texture_filename(filename)?;
                        }
                        texture_filename = Some(filename.to_string());
                    }
                    (Some("Atlas"), "Elements") => {}
                    (Some("Atlas"), "StraightAlpha")
                        if map_compatibility && !alpha_seen && !is_empty =>
                    {
                        if !attributes(&element)?.is_empty() {
                            return Err(String::from(
                                "DST atlas StraightAlpha must not have attributes",
                            ));
                        }
                        alpha_seen = true;
                    }
                    (Some("Elements"), "Element") => {
                        if element_count >= MAX_ELEMENTS {
                            return Err(String::from("DST atlas exceeds its element count limit"));
                        }
                        element_count += 1;
                        let attributes = attributes(&element)?;
                        let element_name = required(&attributes, "name")?;
                        if !(if map_compatibility {
                            element_name.is_empty() || valid_element_name(element_name)
                        } else {
                            valid_asset_name(element_name)
                        }) {
                            return Err(String::from("DST atlas has an invalid element name"));
                        }
                        let element_key = if map_compatibility {
                            map_element_key(element_name)
                        } else {
                            element_name.to_string()
                        };
                        for name in ["u1", "u2", "v1", "v2"] {
                            required(&attributes, name)?;
                        }
                        let rectangle = if selected_keys.is_some_and(|keys| {
                            !keys.contains(&element_key) || elements.contains_key(&element_key)
                        }) {
                            // Selected map modes retain the first native
                            // declaration. A shadowed entry contributes no
                            // rectangle, but all attributes and structure
                            // above still require full validation.
                            None
                        } else {
                            let rectangle = Rectangle {
                                u1: coordinate(&attributes, "u1")?,
                                u2: coordinate(&attributes, "u2")?,
                                v1: coordinate(&attributes, "v1")?,
                                v2: coordinate(&attributes, "v2")?,
                            };
                            if rectangle.u1 >= rectangle.u2 || rectangle.v1 >= rectangle.v2 {
                                return Err(String::from(
                                    "DST atlas has an empty or reversed element rectangle",
                                ));
                            }
                            Some(rectangle)
                        };
                        if let Some(rectangle) = rectangle {
                            if map_compatibility && element_name.is_empty() {
                                // An empty engine key cannot match a valid map request.
                                // Validate its rectangle and full XML structure anyway;
                                // unrelated named entries remain usable and indexable.
                                issues.push(String::from("DST atlas contains an unnamed element"));
                            } else {
                                // Native MiniMap/ImageWidget keep the first XML declaration,
                                // including ASCII-case variants. Do not let a later region
                                // silently change a logical icon's selected artwork.
                                // Terrain classification instead needs an unambiguous
                                // complete shape set, so duplicate keys reject that material.
                                match elements.entry(element_key) {
                                    Entry::Occupied(_)
                                        if !map_compatibility
                                            || matches!(purpose, AtlasUse::Terrain) =>
                                    {
                                        return Err(String::from(
                                            "DST atlas contains duplicate element names",
                                        ));
                                    }
                                    Entry::Occupied(_) => {}
                                    Entry::Vacant(entry) => {
                                        element_names
                                            .insert(entry.key().clone(), element_name.to_string());
                                        entry.insert(rectangle);
                                    }
                                }
                            }
                        }
                    }
                    _ => return Err(String::from("unexpected DST atlas XML structure")),
                }
                if !is_empty {
                    stack.push(name.to_string());
                }
            }
            Event::End(element) => {
                if stack.pop().as_deref() != Some(element.name().as_ref()) {
                    return Err(String::from("mismatched DST atlas XML element"));
                }
                if element.name().as_ref() == "StraightAlpha" {
                    straight_alpha = match alpha_text.trim() {
                        "true" => true,
                        "false" => false,
                        _ => return Err(String::from("invalid DST atlas StraightAlpha value")),
                    };
                }
            }
            Event::Text(text) if stack.last().is_some_and(|name| name == "StraightAlpha") => {
                let text = text.as_ref();
                if text.len() > MAX_ALPHA_TEXT_BYTES.saturating_sub(alpha_text.len()) {
                    return Err(String::from(
                        "DST atlas StraightAlpha text exceeds its limit",
                    ));
                }
                alpha_text.push_str(text);
            }
            Event::Text(text) if text.as_ref().bytes().all(|byte| byte.is_ascii_whitespace()) => {}
            Event::Decl(_) | Event::Comment(_) => {}
            Event::Eof => break,
            _ => return Err(String::from("unsupported content in DST atlas XML")),
        }
    }
    if !root_seen || !stack.is_empty() {
        return Err(String::from("incomplete DST atlas XML"));
    }
    Ok((
        Atlas {
            texture_filename: texture_filename
                .ok_or_else(|| String::from("DST atlas has no texture"))?,
            elements,
            element_names,
            straight_alpha,
        },
        issues,
    ))
}

pub(crate) fn map_element_key(value: &str) -> String {
    // The engine hashes logical atlas names without ASCII case sensitivity.
    // This must never be applied to the adjacent physical texture filename.
    value.to_ascii_lowercase()
}

pub(crate) fn valid_element_name(value: &str) -> bool {
    // Unlike Texture.filename, an Element.name never participates in a file
    // read. Keep the bounded, path-free key while accepting native names such
    // as `.tex` and names containing spaces.
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b' '))
        && !matches!(value, "." | "..")
}

pub(crate) fn valid_asset_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !value.starts_with('.')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

fn attributes(element: &BytesStart<'_>) -> Result<BTreeMap<String, String>, String> {
    let mut values = BTreeMap::new();
    for attribute in element.attributes() {
        let attribute =
            attribute.map_err(|error| format!("invalid DST atlas attribute: {error}"))?;
        if values.len() >= 8 || attribute.value.len() > 256 || attribute.key.as_ref().len() > 32 {
            return Err(String::from("DST atlas attribute exceeds its limit"));
        }
        let key = attribute.key.as_ref();
        let value = attribute
            .normalized_value(XmlVersion::Implicit1_0)
            .map_err(|error| format!("invalid DST atlas attribute value: {error}"))?;
        values.insert(key.to_string(), value.into_owned());
    }
    Ok(values)
}

fn required<'a>(values: &'a BTreeMap<String, String>, name: &str) -> Result<&'a str, String> {
    values
        .get(name)
        .map(String::as_str)
        .ok_or_else(|| format!("DST atlas attribute `{name}` is missing"))
}

fn coordinate(values: &BTreeMap<String, String>, name: &str) -> Result<f64, String> {
    let value = required(values, name)?
        .parse::<f64>()
        .map_err(|_| format!("invalid DST atlas `{name}` coordinate"))?;
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err(format!(
            "DST atlas `{name}` coordinate is outside the texture"
        ));
    }
    Ok(value)
}

pub(super) fn crop_icon(texture: &RgbaImage, rectangle: Rectangle) -> Result<Vec<u8>, String> {
    let (left, top, width, height) = icon_rectangle(texture, rectangle)?;
    let icon = image::imageops::crop_imm(texture, left, top, width, height).to_image();
    let icon = resize_image(icon, 72);
    let mut output = LimitedPngWriter(Vec::new());
    image::codecs::png::PngEncoder::new(&mut output)
        .write_image(
            icon.as_raw(),
            icon.width(),
            icon.height(),
            ExtendedColorType::Rgba8,
        )
        .map_err(|error| format!("failed to encode DST icon PNG: {error}"))?;
    Ok(output.0)
}

pub(crate) fn icon_rectangle(
    texture: &RgbaImage,
    rectangle: Rectangle,
) -> Result<(u32, u32, u32, u32), String> {
    icon_rectangle_with_limit(texture, rectangle, MAX_ICON_DIMENSION)
}

pub(crate) fn map_icon_rectangle(
    texture: &RgbaImage,
    rectangle: Rectangle,
) -> Result<(u32, u32, u32, u32), String> {
    if texture.width() > MAX_MAP_SOURCE_DIMENSION || texture.height() > MAX_MAP_SOURCE_DIMENSION {
        return Err(String::from(
            "DST map icon source exceeds its dimension limit",
        ));
    }
    icon_rectangle_with_limit(texture, rectangle, MAX_MAP_SOURCE_DIMENSION)
}

pub(crate) fn terrain_uv(rectangle: Rectangle) -> [f64; 4] {
    [
        rectangle.u1,
        1.0 - rectangle.v2,
        rectangle.u2 - rectangle.u1,
        rectangle.v2 - rectangle.v1,
    ]
}

fn icon_rectangle_with_limit(
    texture: &RgbaImage,
    rectangle: Rectangle,
    max_dimension: u32,
) -> Result<(u32, u32, u32, u32), String> {
    if [rectangle.u1, rectangle.u2, rectangle.v1, rectangle.v2]
        .iter()
        .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
        || rectangle.u1 >= rectangle.u2
        || rectangle.v1 >= rectangle.v2
    {
        return Err(String::from(
            "DST icon rectangle has invalid UV coordinates",
        ));
    }
    // Atlas UVs use a bottom-left origin and a half-pixel inset. Recover the
    // complete pixel rectangle after the KTEX decoder has flipped the image.
    // https://github.com/dstmodders/klei-tools/blob/main/pkg/win32/Python27/Lib/site-packages/klei/atlas.py
    let width = f64::from(texture.width());
    let height = f64::from(texture.height());
    let left = (rectangle.u1 * width + 0.00001).floor() as u32;
    let right = (rectangle.u2 * width - 0.00001).ceil() as u32;
    let top = ((1.0 - rectangle.v2) * height + 0.00001).floor() as u32;
    let bottom = ((1.0 - rectangle.v1) * height - 0.00001).ceil() as u32;
    if right > texture.width()
        || bottom > texture.height()
        || right <= left
        || bottom <= top
        || right - left > max_dimension
        || bottom - top > max_dimension
    {
        return Err(String::from("DST icon rectangle exceeds its pixel limit"));
    }
    Ok((left, top, right - left, bottom - top))
}

pub(crate) fn crop_map_icon(
    texture: &RgbaImage,
    rectangle: Rectangle,
) -> Result<RgbaImage, String> {
    let (left, top, width, height) = map_icon_rectangle(texture, rectangle)?;
    let source = image::imageops::crop_imm(texture, left, top, width, height);
    if width <= MAX_MAP_ICON_DIMENSION && height <= MAX_MAP_ICON_DIMENSION {
        return Ok(source.to_image());
    }
    // Use the same aspect rounding and alpha filtering as resize_image, but
    // sample a borrowed crop directly so a 4096² icon never needs a second
    // source-sized RGBA allocation.
    let scale = f64::from(MAX_MAP_ICON_DIMENSION) / f64::from(width.max(height));
    let output_width = (f64::from(width) * scale).round().max(1.0) as u32;
    let output_height = (f64::from(height) * scale).round().max(1.0) as u32;
    let mut icon =
        image::imageops::thumbnail(&PremultipliedIconView(source), output_width, output_height);
    unpremultiply_pixels(&mut icon);
    Ok(icon)
}

struct PremultipliedIconView<'a>(image::SubImage<&'a RgbaImage>);

impl GenericImageView for PremultipliedIconView<'_> {
    type Pixel = image::Rgba<u8>;

    fn dimensions(&self) -> (u32, u32) {
        self.0.dimensions()
    }

    fn get_pixel(&self, x: u32, y: u32) -> Self::Pixel {
        let mut pixel = self.0.get_pixel(x, y);
        premultiply_pixel(&mut pixel);
        pixel
    }
}

pub(crate) fn resize_image(mut icon: RgbaImage, max_dimension: u32) -> RgbaImage {
    if icon.width() <= max_dimension && icon.height() <= max_dimension {
        return icon;
    }
    // Filter premultiplied colors so invisible pixels cannot darken the edges.
    for pixel in icon.pixels_mut() {
        premultiply_pixel(pixel);
    }
    let mut icon = image::DynamicImage::ImageRgba8(icon)
        .thumbnail(max_dimension, max_dimension)
        .into_rgba8();
    unpremultiply_pixels(&mut icon);
    icon
}

fn premultiply_pixel(pixel: &mut image::Rgba<u8>) {
    let alpha = u32::from(pixel[3]);
    for channel in &mut pixel.0[..3] {
        *channel = ((u32::from(*channel) * alpha + 127) / 255) as u8;
    }
}

fn unpremultiply_pixels(icon: &mut RgbaImage) {
    for pixel in icon.pixels_mut() {
        let alpha = u32::from(pixel[3]);
        if alpha == 0 {
            pixel.0 = [0; 4];
        } else if alpha < 255 {
            for channel in &mut pixel.0[..3] {
                *channel = ((u32::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
            }
        }
    }
}

struct LimitedPngWriter(Vec<u8>);

impl Write for LimitedPngWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > MAX_ICON_PNG_BYTES.saturating_sub(self.0.len()) {
            return Err(io::Error::other("DST icon PNG exceeds its byte limit"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
#[path = "atlas_tests.rs"]
mod tests;
