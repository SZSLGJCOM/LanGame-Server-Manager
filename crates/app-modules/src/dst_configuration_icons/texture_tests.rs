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
