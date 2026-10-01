use super::*;

const XML: &[u8] = br#"<Atlas><Texture filename="customisation.tex"/><Elements>
    <Element name="rain.tex" u1="0.0625" u2="0.4375" v1="0.5625" v2="0.9375"/>
    </Elements></Atlas>"#;

#[test]
fn atlas_crops_half_pixel_insets_with_the_correct_vertical_origin() {
    let atlas = parse_atlas(XML).expect("synthetic atlas");
    assert_eq!(atlas.texture_filename, "customisation.tex");
    let mut pixels = RgbaImage::from_pixel(8, 8, image::Rgba([0, 0, 255, 255]));
    for y in 0..4 {
        for x in 0..4 {
            pixels.put_pixel(x, y, image::Rgba([255, 0, 0, 255]));
        }
    }
    let png = crop_icon(&pixels, atlas.elements["rain.tex"]).expect("crop synthetic icon");
    let icon = image::load_from_memory(&png)
        .expect("valid PNG")
        .into_rgba8();
    assert_eq!(icon.dimensions(), (4, 4));
    assert!(icon.pixels().all(|pixel| pixel.0 == [255, 0, 0, 255]));
}

#[test]
fn atlas_rejects_external_entities_bad_coordinates_and_duplicate_elements() {
    let xml = std::str::from_utf8(XML).unwrap();
    for invalid in [
        xml.replace("0.0625", "NaN"),
        xml.replace("0.0625", "-1"),
        xml.replace("0.0625", "1"),
        xml.replace("customisation.tex", "../private.tex"),
        xml.replace(
            "</Elements>",
            "<Element name=\"rain.tex\" u1=\"0\" u2=\"1\" v1=\"0\" v2=\"1\"/></Elements>",
        ),
        format!("<!DOCTYPE Atlas [<!ENTITY external SYSTEM 'file:///private'>]>{xml}"),
        xml.replace("<Texture ", "<Texture filename=\"duplicate.tex\" "),
        xml.replace("</Atlas>", ""),
    ] {
        assert!(
            parse_atlas(invalid.as_bytes()).is_err(),
            "accepted invalid atlas: {invalid}"
        );
    }
}

#[test]
fn atlas_bounds_xml_elements_and_encoded_output() {
    assert!(parse_atlas(&vec![b' '; MAX_ATLAS_XML_BYTES + 1]).is_err());
    let mut xml = String::from("<Atlas><Texture filename=\"icons.tex\"/><Elements>");
    for index in 0..=MAX_ELEMENTS {
        xml.push_str(&format!(
            "<Element name=\"icon{index}.tex\" u1=\"0\" u2=\"1\" v1=\"0\" v2=\"1\"/>"
        ));
    }
    xml.push_str("</Elements></Atlas>");
    assert!(
        parse_atlas(xml.as_bytes())
            .err()
            .unwrap()
            .contains("element count")
    );
    let mut writer = LimitedPngWriter(Vec::new());
    assert!(writer.write(&vec![0; MAX_ICON_PNG_BYTES + 1]).is_err());
    assert!(writer.0.is_empty());
}

#[test]
fn large_icons_are_scaled_to_the_desktop_pixel_budget() {
    let atlas = parse_atlas(br#"<Atlas><Texture filename="icons.tex"/><Elements><Element name="icon.tex" u1="0" u2="1" v1="0" v2="1"/></Elements></Atlas>"#).unwrap();
    let source = RgbaImage::from_pixel(128, 64, image::Rgba([20, 40, 60, 255]));
    let png = crop_icon(&source, atlas.elements["icon.tex"]).unwrap();
    let output = image::load_from_memory(&png).unwrap();
    assert_eq!((output.width(), output.height()), (72, 36));
}

#[test]
fn resizing_preserves_color_at_translucent_edges() {
    let mut source = RgbaImage::new(128, 128);
    for y in 0..128 {
        for x in 0..65 {
            source.put_pixel(x, y, image::Rgba([255; 4]));
        }
    }
    let output = resize_icon(source);
    assert_eq!(output.dimensions(), (72, 72));
    assert!(output.pixels().any(|pixel| pixel[3] > 0 && pixel[3] < 255));
    assert!(
        output
            .pixels()
            .filter(|pixel| pixel[3] > 0)
            .all(|pixel| pixel.0[..3] == [255; 3])
    );
}
