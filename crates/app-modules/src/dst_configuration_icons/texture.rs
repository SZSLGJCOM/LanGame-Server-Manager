use std::io::{Cursor, Read};

use image::{DynamicImage, RgbaImage};

pub(super) const MAX_TEXTURE_BYTES: usize = 64 * 1024 * 1024 + 4096;
const MAX_DIMENSION: u32 = 4096;
const MAX_PIXELS: u32 = 4096 * 4096;
const MAX_MIPMAPS: usize = 13;

#[derive(Clone, Copy)]
struct Mipmap {
    width: u32,
    height: u32,
    size: usize,
}

/// Read the current PC KTEX container, without executing a game tool or Lua.
/// Container layout: https://github.com/oblivioncth/Stexatlaser#klei-tex-format
/// Format identifiers: https://github.com/nsimplex/ktools/blob/master/src/common/ktex/specs.cpp
pub(super) fn decode_texture(bytes: &[u8]) -> Result<RgbaImage, String> {
    if bytes.len() > MAX_TEXTURE_BYTES || bytes.get(..4) != Some(b"KTEX") {
        return Err(String::from("invalid or oversized DST KTEX texture"));
    }
    let mut input = Cursor::new(&bytes[4..]);
    let flags = read_u32(&mut input)?;
    let platform = flags & 0xf;
    let format = (flags >> 4) & 0x1f;
    let texture_type = (flags >> 9) & 0xf;
    let mipmap_count = ((flags >> 13) & 0x1f) as usize;
    if !matches!(platform, 0 | 12) || texture_type != 1 {
        return Err(String::from(
            "DST icons require a PC two-dimensional KTEX texture",
        ));
    }
    if !(1..=MAX_MIPMAPS).contains(&mipmap_count) || !matches!(format, 0..=2 | 4 | 5) {
        return Err(String::from("unsupported DST KTEX format or mipmap count"));
    }

    let mut mipmaps = Vec::with_capacity(mipmap_count);
    let mut total_data = 0usize;
    for _ in 0..mipmap_count {
        let width = u32::from(read_u16(&mut input)?);
        let height = u32::from(read_u16(&mut input)?);
        let pitch = u32::from(read_u16(&mut input)?);
        let size = read_u32(&mut input)? as usize;
        validate_dimensions(width, height)?;
        if let Some(previous) = mipmaps.last().copied() {
            let previous: Mipmap = previous;
            if width != (previous.width / 2).max(1)
                || height != (previous.height / 2).max(1)
                || (previous.width == 1 && previous.height == 1)
            {
                return Err(String::from("invalid DST KTEX mipmap dimensions"));
            }
        }
        let (expected_pitch, expected_size) = encoded_layout(format, width, height);
        if pitch != expected_pitch || size != expected_size {
            return Err(String::from(
                "DST KTEX mipmap length or pitch does not match its dimensions",
            ));
        }
        total_data = total_data
            .checked_add(size)
            .filter(|size| *size <= MAX_TEXTURE_BYTES)
            .ok_or_else(|| String::from("DST KTEX mipmap data exceeds its byte limit"))?;
        mipmaps.push(Mipmap {
            width,
            height,
            size,
        });
    }
    let data_offset = 8 + mipmap_count * 10;
    if bytes.len().checked_sub(data_offset) != Some(total_data) {
        return Err(String::from("truncated or trailing DST KTEX mipmap data"));
    }
    let first = mipmaps[0];
    let data = &bytes[data_offset..data_offset + first.size];
    let mut decoded = if format <= 2 {
        decode_blocks(data, first, format)?
    } else if format == 4 {
        RgbaImage::from_raw(first.width, first.height, data.to_vec())
            .ok_or_else(|| String::from("invalid DST RGBA texture length"))?
    } else {
        let rgb = image::RgbImage::from_raw(first.width, first.height, data.to_vec())
            .ok_or_else(|| String::from("invalid DST RGB texture length"))?;
        DynamicImage::ImageRgb8(rgb).into_rgba8()
    };

    // Klei PC atlases use bottom-up rows and premultiplied alpha. PNG uses
    // top-down rows and straight alpha; preserve translucent icon edges.
    image::imageops::flip_vertical_in_place(&mut decoded);
    for pixel in decoded.pixels_mut() {
        let alpha = u32::from(pixel[3]);
        if alpha == 0 {
            pixel.0 = [0; 4];
        } else if alpha < 255 {
            for channel in &mut pixel.0[..3] {
                *channel = ((u32::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
            }
        }
    }
    Ok(decoded)
}

fn validate_dimensions(width: u32, height: u32) -> Result<(), String> {
    if width == 0
        || height == 0
        || width > MAX_DIMENSION
        || height > MAX_DIMENSION
        || width
            .checked_mul(height)
            .is_none_or(|pixels| pixels > MAX_PIXELS)
    {
        return Err(String::from("DST KTEX dimensions exceed the pixel limit"));
    }
    Ok(())
}

fn encoded_layout(format: u32, width: u32, height: u32) -> (u32, usize) {
    let pitch = match format {
        0 => width.div_ceil(4) * 8,
        1 | 2 => width.div_ceil(4) * 16,
        4 => width * 4,
        _ => width * 3,
    };
    let rows = if format <= 2 {
        height.div_ceil(4)
    } else {
        height
    };
    (pitch, (pitch * rows) as usize)
}

fn decode_blocks(data: &[u8], mipmap: Mipmap, format: u32) -> Result<RgbaImage, String> {
    if !mipmap.width.is_multiple_of(4) || !mipmap.height.is_multiple_of(4) {
        return Err(String::from(
            "DST block-compressed atlas dimensions must be multiples of four",
        ));
    }
    // DDS wraps the same BC1/BC2/BC3 blocks. Use image's public, maintained
    // decoder rather than duplicating block decompression.
    // https://learn.microsoft.com/windows/win32/direct3ddds/dds-header
    let mut header = [0u8; 128];
    header[..4].copy_from_slice(b"DDS ");
    for (offset, value) in [
        (4, 124),
        (8, 0x0008_1007),
        (12, mipmap.height),
        (16, mipmap.width),
        (20, mipmap.size as u32),
        (76, 32),
        (80, 4),
        (108, 0x1000),
    ] {
        header[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    header[84..88].copy_from_slice(match format {
        0 => b"DXT1",
        1 => b"DXT3",
        _ => b"DXT5",
    });
    let decoder = image::codecs::dds::DdsDecoder::new(Cursor::new(header).chain(data))
        .map_err(|error| format!("failed to decode DST texture blocks: {error}"))?;
    DynamicImage::from_decoder(decoder)
        .map(DynamicImage::into_rgba8)
        .map_err(|error| format!("failed to read DST texture pixels: {error}"))
}

fn read_u16(input: &mut impl Read) -> Result<u16, String> {
    let mut bytes = [0; 2];
    input
        .read_exact(&mut bytes)
        .map_err(|_| String::from("truncated DST KTEX header"))?;
    Ok(u16::from_le_bytes(bytes))
}

fn read_u32(input: &mut impl Read) -> Result<u32, String> {
    let mut bytes = [0; 4];
    input
        .read_exact(&mut bytes)
        .map_err(|_| String::from("truncated DST KTEX header"))?;
    Ok(u32::from_le_bytes(bytes))
}

#[cfg(test)]
#[path = "texture_tests.rs"]
pub(super) mod tests;
