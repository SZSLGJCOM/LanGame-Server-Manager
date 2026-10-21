use super::tests::{TestRoot, icon_atlas, rgba_texture};
use super::*;

fn source(root: &TestRoot) -> DstMapModSource {
    DstMapModSource {
        name: "workshop-123".into(),
        root: root.0.clone(),
    }
}

fn add_atlas(root: &TestRoot, stem: &str, element: &str, color: [u8; 4]) -> String {
    root.write(
        &format!("images/{stem}.xml"),
        &icon_atlas(&format!("{stem}.tex"), element),
    );
    root.write(&format!("images/{stem}.tex"), &rgba_texture(2, 2, color));
    format!("../mods/workshop-123/images/{stem}.xml")
}

#[test]
fn exact_suffixes_are_independent_and_shadowed_textures_are_never_decoded() {
    let native = TestRoot::new();
    let enabled = TestRoot::new();
    let png = add_atlas(&enabled, "png", "Same.PNG", [255, 0, 0, 255]);
    let shadowed = add_atlas(&enabled, "shadowed", "SAME.png", [255; 4]);
    fs::remove_file(enabled.0.join("images/shadowed.tex")).unwrap();
    let tex = add_atlas(&enabled, "tex", "same.TEX", [0, 0, 255, 255]);
    let unique = add_atlas(&enabled, "unique", "unique.png", [0, 255, 0, 255]);
    let result = load_dst_map_assets(
        &native.0,
        &[source(&enabled)],
        crate::DstMapIconSources {
            registered_atlases: &[png.clone(), shadowed, tex, unique, png],
            ..Default::default()
        },
        &[],
        &[],
        &["same.png".into(), "Same.tex".into(), "unique.png".into()],
        &[],
    )
    .unwrap();
    assert!(result.issues.is_empty(), "{:?}", result.issues);
    assert!(result.icon_fallbacks.is_empty());
    for (name, color) in [
        ("same.png", [255, 0, 0, 255]),
        ("Same.tex", [0, 0, 255, 255]),
        ("unique.png", [0, 255, 0, 255]),
    ] {
        let icon = image::load_from_memory(&result.icons[name])
            .unwrap()
            .into_rgba8();
        assert_eq!(icon.get_pixel(0, 0).0, color);
    }
}

#[test]
fn unknown_earlier_mod_xml_blocks_later_exact_sources_but_keeps_already_selected_icons() {
    for bytes in [None, Some(b"<Atlas><unclosed>".as_slice())] {
        let native = TestRoot::new();
        let enabled = TestRoot::new();
        let safe = add_atlas(&enabled, "safe", "safe.png", [255; 4]);
        if let Some(bytes) = bytes {
            enabled.write("images/unknown.xml", bytes);
        }
        let later = add_atlas(&enabled, "later", "same.png", [0, 255, 0, 255]);
        add_atlas(&enabled, "indexed", "same.tex", [0, 0, 255, 255]);
        let result = load_dst_map_assets(
            &native.0,
            &[source(&enabled)],
            crate::DstMapIconSources {
                registered_atlases: &[
                    safe,
                    "../mods/workshop-123/images/unknown.xml".into(),
                    later,
                ],
                ..Default::default()
            },
            &[],
            &[],
            &["safe.png".into(), "same.png".into()],
            &[],
        )
        .unwrap();
        assert_eq!(result.icons.len(), 1);
        assert!(result.icons.contains_key("safe.png"));
        assert!(result.icon_fallbacks.is_empty());
        assert!(
            result
                .issues
                .iter()
                .any(|issue| issue.contains("unknown.xml"))
        );
    }
}

#[test]
fn unknown_or_explicitly_missing_native_xml_cannot_borrow_a_later_native_or_mod_atlas() {
    for malformed in [false, true] {
        let native = TestRoot::new();
        let enabled = TestRoot::new();
        if malformed {
            native.write(
                "data/minimap/minimap_data1.xml",
                b"<Atlas><Unknown/></Atlas>",
            );
        }
        native.write(
            "data/minimap/minimap_data2.xml",
            &icon_atlas("minimap_atlas2.tex", "same.png"),
        );
        native.write(
            "data/minimap/minimap_atlas2.tex",
            &rgba_texture(2, 2, [255; 4]),
        );
        let later = add_atlas(&enabled, "later", "same.png", [0, 255, 0, 255]);
        let result = load_dst_map_assets(
            &native.0,
            &[source(&enabled)],
            crate::DstMapIconSources {
                registered_atlases: &[
                    "minimap/minimap_data1.xml".into(),
                    "minimap/minimap_data2.xml".into(),
                    later,
                ],
                ..Default::default()
            },
            &[],
            &[],
            &["same.png".into()],
            &[],
        )
        .unwrap();
        assert!(result.icons.is_empty());
        assert!(result.icon_fallbacks.is_empty());
        assert!(
            result
                .issues
                .iter()
                .any(|issue| issue.contains("minimap_data1.xml"))
        );
    }
}

#[test]
fn observed_registration_order_also_bounds_the_native_completion_sources() {
    let native = TestRoot::new();
    let enabled = TestRoot::new();
    let with_alias = |texture: &str| {
        String::from_utf8(icon_atlas(texture, "same.png"))
            .unwrap()
            .replace(
                "</Elements>",
                "<Element name=\"alias.tex\" u1=\"0\" u2=\"1\" v1=\"0\" v2=\"1\"/></Elements>",
            )
            .into_bytes()
    };
    native.write(
        "data/minimap/minimap_data1.xml",
        &with_alias("minimap_atlas1.tex"),
    );
    native.write(
        "data/minimap/minimap_atlas1.tex",
        &rgba_texture(2, 2, [255, 0, 0, 255]),
    );
    native.write(
        "data/minimap/minimap_data2.xml",
        &icon_atlas("minimap_atlas2.tex", "same.png"),
    );
    native.write(
        "data/minimap/minimap_atlas2.tex",
        &rgba_texture(2, 2, [0, 255, 0, 255]),
    );
    let first = add_atlas(&enabled, "first", "same.png", [0, 0, 255, 255]);
    enabled.write("images/first.xml", &with_alias("first.tex"));
    let result = load_dst_map_assets(
        &native.0,
        &[source(&enabled)],
        crate::DstMapIconSources {
            registered_atlases: &[first.clone(), "minimap/minimap_data2.xml".into()],
            ..Default::default()
        },
        &[],
        &[],
        &["same.png".into(), "alias.png".into()],
        &[],
    )
    .unwrap();
    assert!(result.issues.is_empty(), "{:?}", result.issues);
    for name in ["same.png", "alias.png"] {
        let icon = image::load_from_memory(&result.icons[name])
            .unwrap()
            .into_rgba8();
        assert_eq!(icon.get_pixel(0, 0).0, [0, 0, 255, 255]);
    }
    assert_eq!(result.icon_fallbacks.len(), 1);
    assert_eq!(result.icon_fallbacks[0].atlas, first);
}
