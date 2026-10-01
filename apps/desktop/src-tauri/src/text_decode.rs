use encoding_rs::GB18030;

pub(crate) fn decode_utf8_or_gb18030_text(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);

    if let Ok(source) = std::str::from_utf8(bytes) {
        return source.to_owned();
    }

    let (source, had_errors) = GB18030.decode_without_bom_handling(bytes);
    if had_errors {
        return String::from_utf8_lossy(bytes).into_owned();
    }

    source.into_owned()
}

#[cfg(test)]
mod tests {
    use super::decode_utf8_or_gb18030_text;

    #[test]
    fn preserves_utf8_mod_text_and_removes_its_optional_bom() {
        let source = "name = \"中文模组\"\n";
        assert_eq!(decode_utf8_or_gb18030_text(source.as_bytes()), source);

        let mut with_bom = vec![0xEF, 0xBB, 0xBF];
        with_bom.extend_from_slice(source.as_bytes());
        assert_eq!(decode_utf8_or_gb18030_text(&with_bom), source);
    }

    #[test]
    fn decodes_gb18030_two_and_four_byte_characters() {
        let bytes = [0xD6, 0xD0, 0xCE, 0xC4, 0x95, 0x34, 0xB2, 0x35];
        assert_eq!(decode_utf8_or_gb18030_text(&bytes), "中文𠮷");
    }

    #[test]
    fn replaces_invalid_and_truncated_input_without_losing_ascii() {
        for bytes in [&b"name = \xFF"[..], &b"name = \x81"[..]] {
            assert_eq!(decode_utf8_or_gb18030_text(bytes), "name = \u{FFFD}");
        }
    }

    #[test]
    fn accepts_empty_input_and_a_bom_without_content() {
        assert_eq!(decode_utf8_or_gb18030_text(&[]), "");
        assert_eq!(decode_utf8_or_gb18030_text(&[0xEF, 0xBB, 0xBF]), "");
    }
}
