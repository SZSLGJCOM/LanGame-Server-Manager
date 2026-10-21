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
fn atlas_rejects_external_entities_bad_coordinates_and_conflicting_elements() {
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
fn map_atlas_keeps_the_first_declaration_even_when_later_rectangles_differ() {
    let xml = std::str::from_utf8(XML).unwrap();
    let repeated = xml.replace(
        "</Elements>",
        "<Element name=\"rain.tex\" u1=\"0.0625\" u2=\"0.4375\" v1=\"0.5625\" v2=\"0.9375\"/></Elements>",
    );
    assert!(parse_atlas(repeated.as_bytes()).is_err());
    let (atlas, issues) = parse_atlas_with_issues(repeated.as_bytes()).unwrap();
    assert_eq!(atlas.elements.len(), 1);
    assert!(issues.is_empty());

    // Native XML lookup keeps the first declaration, not the last rectangle.
    let conflicting = repeated.replace(
        "</Elements>",
        "<Element name=\"rain.tex\" u1=\"0\" u2=\"1\" v1=\"0\" v2=\"1\"/>
         <Element name=\"rain.tex\" u1=\"0.0625\" u2=\"0.4375\" v1=\"0.5625\" v2=\"0.9375\"/>
         <Element name=\"clear.tex\" u1=\"0\" u2=\"0.5\" v1=\"0\" v2=\"0.5\"/></Elements>",
    );
    assert!(parse_atlas(conflicting.as_bytes()).is_err());
    let (atlas, issues) = parse_atlas_with_issues(conflicting.as_bytes()).unwrap();
    assert_eq!(
        atlas.elements["rain.tex"],
        parse_atlas(XML).unwrap().elements["rain.tex"]
    );
    assert!(atlas.elements.contains_key("clear.tex"));
    assert!(issues.is_empty());
}

#[test]
fn only_map_atlas_keys_follow_the_engines_ascii_case_insensitive_names() {
    let xml = std::str::from_utf8(XML)
        .unwrap()
        .replace("rain.tex", "bambooTree.tex")
        .replace("customisation.tex", "MapTexture.tex");
    let strict = parse_atlas(xml.as_bytes()).unwrap();
    assert!(strict.elements.contains_key("bambooTree.tex"));
    assert!(!strict.elements.contains_key("bambootree.tex"));
    let repeated = xml.replace(
        "</Elements>",
        "<Element name=\"BAMBOOTREE.TEX\" u1=\"0.0625\" u2=\"0.4375\" v1=\"0.5625\" v2=\"0.9375\"/></Elements>",
    );
    assert_eq!(parse_atlas(repeated.as_bytes()).unwrap().elements.len(), 2);
    let (atlas, issues) = parse_atlas_with_issues(repeated.as_bytes()).unwrap();
    assert_eq!(atlas.texture_filename, "MapTexture.tex");
    assert_eq!(atlas.elements.len(), 1);
    assert!(atlas.elements.contains_key("bambootree.tex"));
    assert!(issues.is_empty());

    let conflicting = repeated.replace(
        "</Elements>",
        "<Element name=\"BambooTree.tex\" u1=\"0\" u2=\"1\" v1=\"0\" v2=\"1\"/>
         <Element name=\"bambootree.tex\" u1=\"0.0625\" u2=\"0.4375\" v1=\"0.5625\" v2=\"0.9375\"/></Elements>",
    );
    let (atlas, issues) = parse_atlas_with_issues(conflicting.as_bytes()).unwrap();
    assert_eq!(atlas.elements.len(), 1);
    assert_eq!(
        atlas.elements["bambootree.tex"],
        parse_atlas(XML).unwrap().elements["rain.tex"]
    );
    assert_eq!(atlas.element_names["bambootree.tex"], "bambooTree.tex");
    assert!(issues.is_empty());
}

#[test]
fn atlas_element_lookup_keys_are_distinct_from_texture_filenames() {
    let xml = std::str::from_utf8(XML).unwrap();
    for name in [".tex", "oversized _waxed.tex", "oversized waxed.tex"] {
        let keyed = xml.replace("rain.tex", name);
        let (atlas, issues) = parse_atlas_with_issues(keyed.as_bytes()).unwrap();
        assert!(atlas.elements.contains_key(name));
        assert!(issues.is_empty());
        assert!(parse_atlas(keyed.as_bytes()).is_err());
        assert!(parse_atlas(xml.replace("customisation.tex", name).as_bytes()).is_err());
    }
    for name in [
        ".",
        "..",
        "../private.tex",
        "dir/icon.tex",
        "dir\\icon.tex",
        "https:icon.tex",
    ] {
        let keyed = xml.replace("rain.tex", name);
        assert!(parse_atlas_with_issues(keyed.as_bytes()).is_err());
    }
}

#[test]
fn only_map_atlases_skip_fully_validated_unnamed_entries() {
    let xml = std::str::from_utf8(XML).unwrap();
    let unnamed = xml.replace(
        "</Elements>",
        "<Element name=\"\" u1=\"0\" u2=\"1\" v1=\"0\" v2=\"1\"/></Elements>",
    );
    assert!(!valid_element_name(""));
    assert!(parse_atlas(unnamed.as_bytes()).is_err());
    let (atlas, issues) = parse_atlas_with_issues(unnamed.as_bytes()).unwrap();
    assert_eq!(atlas.elements.len(), 1);
    assert!(atlas.elements.contains_key("rain.tex"));
    assert!(!atlas.element_names.contains_key(""));
    assert_eq!(issues, vec!["DST atlas contains an unnamed element"]);
    for invalid in [
        unnamed.replace("u1=\"0\"", "u1=\"NaN\""),
        unnamed.replace("u1=\"0\"", "u1=\"-1\""),
        unnamed.replace("u1=\"0\"", "u1=\"1\""),
        unnamed.replace("name=\"\"", "name=\"\" name=\"\""),
        unnamed.replace("v2=\"1\"/>", "v2=\"1\"><Nested/></Element>"),
    ] {
        assert!(parse_atlas_with_issues(invalid.as_bytes()).is_err());
    }
}

#[test]
fn map_index_enumerates_all_keys_and_validates_uvs_and_texture_paths_only_for_candidates() {
    let xml = std::str::from_utf8(XML)
        .unwrap()
        .replace("customisation.tex", "data/levels/tiles/terrain.tex")
        .replace("rain.tex", "01");
    let wanted = BTreeSet::from(["rain.tex".to_string()]);
    assert!(parse_atlas(xml.as_bytes()).is_err());
    assert!(parse_atlas_with_issues(xml.as_bytes()).is_err());
    assert!(
        parse_atlas_for_map_index(xml.as_bytes(), &wanted)
            .unwrap()
            .is_none()
    );
    for unrequested_uv in [xml.replace("0.0625", "NaN"), xml.replace("0.0625", "1")] {
        assert!(
            parse_atlas_for_map_index(unrequested_uv.as_bytes(), &wanted)
                .unwrap()
                .is_none()
        );
        let candidate = unrequested_uv
            .replace("data/levels/tiles/terrain.tex", "customisation.tex")
            .replace("name=\"01\"", "name=\"rain.tex\"");
        assert!(parse_atlas_for_map_index(candidate.as_bytes(), &wanted).is_err());
    }
    for invalid in [
        xml.replace("</Atlas>", "<Unknown/></Atlas>"),
        xml.replace("name=\"01\"", "name=\"01\" name=\"02\""),
        xml.replace("u1=\"0.0625\"", ""),
        xml.replace("name=\"01\"", "name=\"../01\""),
        xml.replace("</Atlas>", ""),
        xml.replace("name=\"01\"", "name=\"rain.tex\""),
    ] {
        assert!(parse_atlas_for_map_index(invalid.as_bytes(), &wanted).is_err());
    }
}

#[test]
fn partial_atlas_loading_still_rejects_invalid_uvs() {
    let xml = std::str::from_utf8(XML).unwrap();
    for invalid in ["NaN", "-1", "1", "inf"] {
        let invalid = xml.replace("0.0625", invalid);
        assert!(parse_atlas_with_issues(invalid.as_bytes()).is_err());
    }
}

#[test]
fn map_atlases_accept_bounded_optional_straight_alpha_metadata() {
    let xml = std::str::from_utf8(XML).unwrap();
    let (atlas, _) = parse_atlas_with_issues(XML).unwrap();
    assert!(!atlas.straight_alpha);
    for (value, expected) in [("true", true), ("false", false), (" \n false\t", false)] {
        let declared = xml.replace(
            "</Atlas>",
            &format!("<StraightAlpha>{value}</StraightAlpha></Atlas>"),
        );
        let (atlas, issues) = parse_atlas_with_issues(declared.as_bytes()).unwrap();
        assert_eq!(atlas.straight_alpha, expected);
        assert!(atlas.elements.contains_key("rain.tex"));
        assert!(issues.is_empty());
        assert!(parse_atlas(declared.as_bytes()).is_err());
    }
}

#[test]
fn map_atlases_reject_invalid_or_ambiguous_straight_alpha_metadata() {
    let xml = std::str::from_utf8(XML).unwrap();
    for declaration in [
        "<StraightAlpha/>".to_string(),
        "<StraightAlpha></StraightAlpha>".to_string(),
        "<StraightAlpha> </StraightAlpha>".to_string(),
        "<StraightAlpha>1</StraightAlpha>".to_string(),
        "<StraightAlpha>True</StraightAlpha>".to_string(),
        "<StraightAlpha><Value>false</Value></StraightAlpha>".to_string(),
        "<StraightAlpha value=\"false\">false</StraightAlpha>".to_string(),
        "<StraightAlpha>false</StraightAlpha><StraightAlpha>false</StraightAlpha>".to_string(),
        "<StraightAlpha><![CDATA[false]]></StraightAlpha>".to_string(),
        format!(
            "<StraightAlpha>{}false</StraightAlpha>",
            " ".repeat(MAX_ALPHA_TEXT_BYTES)
        ),
    ] {
        let invalid = xml.replace("</Atlas>", &format!("{declaration}</Atlas>"));
        assert!(
            parse_atlas_with_issues(invalid.as_bytes()).is_err(),
            "accepted invalid alpha metadata: {declaration}"
        );
    }
}

#[test]
#[ignore = "requires the actual downloaded Mod root in LANGAME_DST_ATLAS_SMOKE_ROOT"]
fn installed_native_minimap_atlases_parse_complete_documents() {
    let root = std::path::PathBuf::from(
        std::env::var_os("LANGAME_DST_ATLAS_SMOKE_ROOT")
            .expect("LANGAME_DST_ATLAS_SMOKE_ROOT must identify native workshop content/322330"),
    );
    // This is an acceptance probe of the complete registered documents, not a
    // production Mod allowlist. Paths and expected source facts are deliberately
    // kept here so an unknown trailing XML node cannot hide behind a snippet.
    let cases: [(&str, &str, usize, &[&str], &str); 4] = [
        (
            "2986194136/images/inventoryimages/inventory_shipwrecked.xml",
            "inventory_shipwrecked.tex",
            404,
            &[
                "armorseashell.tex",
                "messagebottle_sw.tex",
                "tropicalfan.tex",
            ],
            "slot_machine.tex",
        ),
        (
            "2986194136/images/inventoryimages/inventory_hamlet.xml",
            "inventory_hamlet.tex",
            586,
            &[],
            ".tex",
        ),
        (
            "2986194136/images/inventoryimages/inventory_extension.xml",
            "inventory_extension.tex",
            129,
            &["bell.tex"],
            "turnip_oversized _waxed.tex",
        ),
        (
            "1289779251/images/cherryminimap.xml",
            "cherryminimap.tex",
            61,
            &[],
            "cherry_stonetree.png",
        ),
    ];
    for (relative, texture, element_count, shadowed, required) in cases {
        let path = root.join(relative);
        let metadata = std::fs::metadata(&path).expect("native atlas metadata");
        assert!(metadata.is_file() && metadata.len() <= MAX_ATLAS_XML_BYTES as u64);
        let bytes = std::fs::read(&path).expect("read complete native atlas");
        let (atlas, issues) = parse_atlas_with_issues(&bytes)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        assert_eq!(atlas.texture_filename, texture, "{relative}");
        assert_eq!(atlas.elements.len(), element_count, "{relative}");
        assert!(
            atlas.elements.contains_key(required),
            "{relative}: {required}"
        );
        if texture == "inventory_shipwrecked.tex" {
            for name in ["bambooTree.tex", "jungleTree.tex", "limpetRock.tex"] {
                assert!(
                    atlas.elements.contains_key(&map_element_key(name)),
                    "{relative}: native ASCII-fold key {name}"
                );
            }
        }
        for name in shadowed {
            assert!(atlas.elements.contains_key(*name), "{relative}: {name}");
        }
        assert!(issues.is_empty(), "{relative}: {issues:?}");
        assert!(!atlas.straight_alpha, "{relative}");
        println!(
            "native atlas {relative}: read {} bytes; {} unambiguous elements; {} issues; straight_alpha={}",
            bytes.len(),
            atlas.elements.len(),
            issues.len(),
            atlas.straight_alpha
        );
    }
}

#[test]
fn atlas_bounds_xml_elements_and_encoded_output() {
    assert!(parse_atlas(&vec![b' '; MAX_ATLAS_XML_BYTES + 1]).is_err());
    let mut xml = String::from("<Atlas><Texture filename=\"icons.tex\"/><Elements>");
    let mut repeated = xml.clone();
    for index in 0..=MAX_ELEMENTS {
        xml.push_str(&format!(
            "<Element name=\"icon{index}.tex\" u1=\"0\" u2=\"1\" v1=\"0\" v2=\"1\"/>"
        ));
        repeated.push_str("<Element name=\"icon.tex\" u1=\"0\" u2=\"1\" v1=\"0\" v2=\"1\"/>");
    }
    xml.push_str("</Elements></Atlas>");
    repeated.push_str("</Elements></Atlas>");
    assert!(
        parse_atlas(xml.as_bytes())
            .err()
            .unwrap()
            .contains("element count")
    );
    assert!(
        parse_atlas_with_issues(repeated.as_bytes())
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
fn map_icons_allow_native_large_rectangles_without_changing_the_configuration_limit() {
    let rectangle = Rectangle {
        u1: 0.0,
        u2: 1.0,
        v1: 0.0,
        v2: 1.0,
    };
    let source = RgbaImage::from_pixel(512, 512, image::Rgba([20, 40, 60, 255]));
    assert!(icon_rectangle(&source, rectangle).is_err());
    assert!(crop_icon(&source, rectangle).is_err());
    assert_eq!(
        map_icon_rectangle(&source, rectangle).unwrap(),
        (0, 0, 512, 512)
    );
    let output = crop_map_icon(&source, rectangle).unwrap();
    assert_eq!(output.dimensions(), (64, 64));
    assert!(output.pixels().all(|pixel| pixel.0 == [20, 40, 60, 255]));
}

#[test]
fn borrowed_map_crops_match_existing_resize_pixels_and_aspect_rounding() {
    let rectangle = Rectangle {
        u1: 0.0,
        u2: 1.0,
        v1: 0.0,
        v2: 1.0,
    };
    for (width, height) in [(512, 512), (512, 128), (256, 173), (127, 91), (63, 31)] {
        let source = RgbaImage::from_fn(width, height, |x, y| {
            image::Rgba([
                (x % 256) as u8,
                (y % 256) as u8,
                ((x + y) % 256) as u8,
                if x < width / 2 { 255 } else { (y % 256) as u8 },
            ])
        });
        assert_eq!(
            crop_map_icon(&source, rectangle).unwrap(),
            resize_image(source, 64),
            "{width}x{height} alpha filtering must remain identical"
        );
    }
}

#[test]
fn map_crops_still_reject_oversized_sources_and_invalid_uvs() {
    let rectangle = Rectangle {
        u1: 0.0,
        u2: 1.0,
        v1: 0.0,
        v2: 1.0,
    };
    for (width, height) in [(4097, 1), (1, 4097)] {
        let source = RgbaImage::new(width, height);
        assert!(crop_map_icon(&source, rectangle).is_err());
    }
    let boundary = RgbaImage::from_pixel(4096, 1, image::Rgba([255; 4]));
    assert_eq!(
        crop_map_icon(&boundary, rectangle).unwrap().dimensions(),
        (64, 1)
    );
    let source = RgbaImage::new(2, 2);
    for invalid in [
        Rectangle {
            u1: f64::NAN,
            ..rectangle
        },
        Rectangle {
            u1: -0.1,
            ..rectangle
        },
        Rectangle {
            u2: 1.1,
            ..rectangle
        },
        Rectangle {
            v1: 1.0,
            ..rectangle
        },
        Rectangle {
            v2: f64::INFINITY,
            ..rectangle
        },
    ] {
        assert!(crop_map_icon(&source, invalid).is_err());
    }
}

#[test]
fn resizing_preserves_color_at_translucent_edges() {
    let mut source = RgbaImage::new(128, 128);
    for y in 0..128 {
        for x in 0..65 {
            source.put_pixel(x, y, image::Rgba([255; 4]));
        }
    }
    let output = resize_image(source, 72);
    assert_eq!(output.dimensions(), (72, 72));
    assert!(output.pixels().any(|pixel| pixel[3] > 0 && pixel[3] < 255));
    assert!(
        output
            .pixels()
            .filter(|pixel| pixel[3] > 0)
            .all(|pixel| pixel.0[..3] == [255; 3])
    );
}
