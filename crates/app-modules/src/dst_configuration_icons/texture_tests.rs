use super::*;

pub(in crate::dst_configuration_icons) fn texture_fixture(
    format: u32,
    width: u16,
    height: u16,
    data: &[u8],
) -> Vec<u8> {
    let flags = 0xfffc_0000 | (1 << 13) | (1 << 9) | (format << 4) | 12;
    let mut output = b"KTEX".to_vec();
    output.extend_from_slice(&flags.to_le_bytes());
    output.extend_from_slice(&width.to_le_bytes());
    output.extend_from_slice(&height.to_le_bytes());
    let (pitch, _) = encoded_layout(format, u32::from(width), u32::from(height));
    output.extend_from_slice(&(pitch as u16).to_le_bytes());
    output.extend_from_slice(&(data.len() as u32).to_le_bytes());
    output.extend_from_slice(data);
    output
}

#[test]
fn texture_decodes_bc3_blocks_with_bottom_up_rows() {
    let mut blocks = Vec::new();
    for color in [0x001fu16, 0xf800u16] {
        blocks.extend_from_slice(&[255, 255, 0, 0, 0, 0, 0, 0]);
        blocks.extend_from_slice(&color.to_le_bytes());
        blocks.extend_from_slice(&0u16.to_le_bytes());
        blocks.extend_from_slice(&[0; 4]);
    }
    let decoded = decode_texture(&texture_fixture(2, 4, 8, &blocks)).expect("decode synthetic BC3");
    assert_eq!(decoded.dimensions(), (4, 8));
    assert_eq!(decoded.get_pixel(0, 0).0, [255, 0, 0, 255]);
    assert_eq!(decoded.get_pixel(3, 7).0, [0, 0, 255, 255]);
}

#[test]
fn texture_preserves_straight_alpha_and_clears_invisible_colors() {
    let bytes = texture_fixture(4, 2, 1, &[64, 32, 0, 128, 99, 80, 70, 0]);
    let decoded = decode_texture(&bytes).expect("decode synthetic RGBA");
    assert_eq!(decoded.get_pixel(0, 0).0, [128, 64, 0, 128]);
    assert_eq!(decoded.get_pixel(1, 0).0, [0, 0, 0, 0]);
}

#[test]
fn texture_honors_explicit_straight_alpha_without_changing_default_decoding() {
    let bytes = texture_fixture(4, 3, 1, &[64, 32, 8, 128, 99, 80, 70, 0, 7, 19, 31, 255]);
    let straight = decode_texture_with_alpha(&bytes, true).expect("straight-alpha RGBA");
    assert_eq!(straight.get_pixel(0, 0).0, [64, 32, 8, 128]);
    assert_eq!(straight.get_pixel(1, 0).0, [0, 0, 0, 0]);
    assert_eq!(straight.get_pixel(2, 0).0, [7, 19, 31, 255]);

    let premultiplied = decode_texture_with_alpha(&bytes, false).expect("premultiplied RGBA");
    assert_eq!(premultiplied.get_pixel(0, 0).0, [128, 64, 16, 128]);
    assert_eq!(premultiplied.get_pixel(1, 0).0, [0, 0, 0, 0]);
    assert_eq!(premultiplied.get_pixel(2, 0).0, [7, 19, 31, 255]);
    assert_eq!(decode_texture(&bytes).unwrap(), premultiplied);
}

#[test]
fn texture_rejects_truncation_trailing_data_and_dimension_amplification() {
    let valid = texture_fixture(4, 2, 1, &[0; 8]);
    for length in 0..valid.len() {
        assert!(
            decode_texture(&valid[..length]).is_err(),
            "accepted {length} bytes"
        );
    }
    let mut trailing = valid.clone();
    trailing.push(0);
    assert!(decode_texture(&trailing).is_err());
    let mut oversized = valid.clone();
    oversized[8..10].copy_from_slice(&u16::MAX.to_le_bytes());
    assert!(
        decode_texture(&oversized)
            .expect_err("oversized header")
            .contains("pixel limit")
    );
    let mut unsupported = valid;
    unsupported[4] = 0xfc;
    assert!(decode_texture(&unsupported).is_err());
}

#[test]
fn texture_checks_the_entire_mipmap_chain_before_decoding() {
    let first = texture_fixture(4, 2, 2, &[0; 16]);
    let second = texture_fixture(4, 1, 1, &[0; 4]);
    let mut chain = first[..18].to_vec();
    let flags = u32::from_le_bytes(chain[4..8].try_into().unwrap()) + (1 << 13);
    chain[4..8].copy_from_slice(&flags.to_le_bytes());
    chain.extend_from_slice(&second[8..18]);
    chain.extend_from_slice(&first[18..]);
    chain.extend_from_slice(&second[18..]);
    assert!(decode_texture(&chain).is_ok());
    chain[18..20].copy_from_slice(&2u16.to_le_bytes());
    assert!(
        decode_texture(&chain)
            .expect_err("non-halving mip")
            .contains("mipmap dimensions")
    );
}

#[test]
fn texture_decodes_exporter_omitted_pitch_in_a_bc3_mipmap_chain() {
    // Independent literal header: PC-default DXT5, two mipmaps. Both 4x4 and
    // 2x2 occupy one BC3 block; exporters may leave both pitch fields at zero.
    let mut bytes = vec![
        b'K', b'T', b'E', b'X', 0x20, 0x42, 0xf0, 0xff, 4, 0, 4, 0, 0, 0, 16, 0, 0, 0, 2, 0, 2, 0,
        0, 0, 16, 0, 0, 0,
    ];
    let red = [255, 255, 0, 0, 0, 0, 0, 0, 0, 248, 0, 0, 0, 0, 0, 0];
    bytes.extend_from_slice(&red);
    bytes.extend_from_slice(&red);
    let decoded = decode_texture(&bytes).expect("decode omitted-pitch BC3 chain");
    assert_eq!(decoded.dimensions(), (4, 4));
    assert!(decoded.pixels().all(|pixel| pixel.0 == [255, 0, 0, 255]));

    let mut wrong_pitch = bytes.clone();
    wrong_pitch[22..24].copy_from_slice(&8u16.to_le_bytes());
    assert!(decode_texture(&wrong_pitch).is_err());
    let mut wrong_size = bytes.clone();
    wrong_size[24..28].copy_from_slice(&15u32.to_le_bytes());
    assert!(decode_texture(&wrong_size).is_err());
    assert!(decode_texture(&bytes[..bytes.len() - 1]).is_err());
    bytes.push(0);
    assert!(decode_texture(&bytes).is_err());
}

#[test]
fn texture_decodes_omitted_pitch_for_tightly_packed_rgb_and_rgba() {
    for (format, data) in [(4, vec![64, 32, 0, 128]), (5, vec![64, 32, 0])] {
        let mut bytes = texture_fixture(format, 1, 1, &data);
        bytes[12..14].copy_from_slice(&0u16.to_le_bytes());
        let decoded = decode_texture(&bytes).expect("decode omitted-pitch raw pixels");
        assert_eq!(
            decoded.get_pixel(0, 0).0,
            if format == 4 {
                [128, 64, 0, 128]
            } else {
                [64, 32, 0, 255]
            }
        );
        bytes[12..14].copy_from_slice(&1u16.to_le_bytes());
        assert!(decode_texture(&bytes).is_err());
    }
}

#[test]
#[ignore = "reads only an explicitly supplied owned workshop resource directory"]
fn texture_decodes_owned_workshop_resources() {
    let root = std::env::var_os("LANGAME_DST_TEXTURE_SMOKE_ROOT")
        .map(std::path::PathBuf::from)
        .expect("owned workshop resource directory");
    let mut pending = vec![root.clone()];
    let mut decoded_count = 0usize;
    let mut omitted_pitch_count = 0usize;
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory).expect("read workshop directory") {
            let entry = entry.expect("read workshop resource entry");
            let file_type = entry.file_type().expect("workshop resource file type");
            if file_type.is_dir() {
                pending.push(entry.path());
            } else if file_type.is_file()
                && entry.path().extension().is_some_and(|value| value == "tex")
            {
                let bytes = std::fs::read(entry.path()).expect("read workshop TEX");
                let decoded = decode_texture(&bytes).unwrap_or_else(|error| {
                    panic!("{}: {error}", entry.path().display());
                });
                assert!(decoded.width() > 0 && decoded.height() > 0);
                decoded_count += 1;
                omitted_pitch_count += usize::from(bytes[12..14] == [0, 0]);
            }
        }
    }
    assert!(decoded_count > 0, "no KTEX resources in {}", root.display());
    println!(
        "NATIVE_WORKSHOP_TEXTURES root={} decoded={decoded_count} omitted_pitch={omitted_pitch_count}",
        root.display()
    );
}
