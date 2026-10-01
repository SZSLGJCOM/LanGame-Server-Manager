use std::collections::BTreeMap;
use std::io::{self, Write};

use image::{ExtendedColorType, ImageEncoder, RgbaImage};
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};

pub(super) const MAX_ATLAS_XML_BYTES: usize = 1024 * 1024;
const MAX_ELEMENTS: usize = 2048;
const MAX_ICON_DIMENSION: u32 = 256;
const MAX_ICON_PNG_BYTES: usize = 512 * 1024;

pub(super) struct Atlas {
    pub texture_filename: String,
    pub elements: BTreeMap<String, Rectangle>,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Rectangle {
    u1: f64,
    u2: f64,
    v1: f64,
    v2: f64,
}

pub(super) fn parse_atlas(bytes: &[u8]) -> Result<Atlas, String> {
    if bytes.len() > MAX_ATLAS_XML_BYTES {
        return Err(String::from("DST atlas XML exceeds its byte limit"));
    }
    let xml = std::str::from_utf8(bytes).map_err(|_| String::from("DST atlas XML is not UTF-8"))?;
    let mut reader = Reader::from_str(xml);
    let mut stack: Vec<String> = Vec::new();
    let mut texture_filename = None;
    let mut elements = BTreeMap::new();
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
                        if !valid_asset_name(filename) || !filename.ends_with(".tex") {
                            return Err(String::from("DST atlas has an invalid texture filename"));
                        }
                        texture_filename = Some(filename.to_string());
                    }
                    (Some("Atlas"), "Elements") => {}
                    (Some("Elements"), "Element") => {
                        if elements.len() >= MAX_ELEMENTS {
                            return Err(String::from("DST atlas exceeds its element count limit"));
                        }
                        let attributes = attributes(&element)?;
                        let element_name = required(&attributes, "name")?;
                        if !valid_asset_name(element_name) {
                            return Err(String::from("DST atlas has an invalid element name"));
                        }
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
                        if elements
                            .insert(element_name.to_string(), rectangle)
                            .is_some()
                        {
                            return Err(String::from("DST atlas contains duplicate element names"));
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
    Ok(Atlas {
        texture_filename: texture_filename
            .ok_or_else(|| String::from("DST atlas has no texture"))?,
        elements,
    })
}

pub(super) fn valid_asset_name(value: &str) -> bool {
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
    let icon = resize_icon(icon);
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

pub(super) fn icon_rectangle(
    texture: &RgbaImage,
    rectangle: Rectangle,
) -> Result<(u32, u32, u32, u32), String> {
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
        || right - left > MAX_ICON_DIMENSION
        || bottom - top > MAX_ICON_DIMENSION
    {
        return Err(String::from("DST icon rectangle exceeds its pixel limit"));
    }
    Ok((left, top, right - left, bottom - top))
}

fn resize_icon(mut icon: RgbaImage) -> RgbaImage {
    if icon.width() <= 72 && icon.height() <= 72 {
        return icon;
    }
    // Filter premultiplied colors so invisible pixels cannot darken the edges.
    for pixel in icon.pixels_mut() {
        let alpha = u32::from(pixel[3]);
        for channel in &mut pixel.0[..3] {
            *channel = ((u32::from(*channel) * alpha + 127) / 255) as u8;
        }
    }
    let mut icon = image::DynamicImage::ImageRgba8(icon)
        .thumbnail(72, 72)
        .into_rgba8();
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
    icon
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
