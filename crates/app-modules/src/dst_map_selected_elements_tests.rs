use super::tests::{TestRoot, icon_atlas, rgba_texture};
use super::*;

fn source(root: &TestRoot) -> DstMapModSource {
    DstMapModSource {
        name: "enabled".into(),
        root: root.0.clone(),
    }
}

fn with_invalid_uv(texture: &str, valid: &str, invalid: &str) -> Vec<u8> {
    String::from_utf8(icon_atlas(texture, valid))
        .unwrap()
        .replace(
            "</Elements>",
            &format!(
                "<Element name=\"{invalid}\" u1=\"0\" u2=\"95\" v1=\"0\" v2=\"1\"/></Elements>"
            ),
        )
        .into_bytes()
}

fn mod_atlas(root: &TestRoot, stem: &str, xml: &[u8], color: [u8; 4]) -> String {
    root.write(&format!("images/{stem}.xml"), xml);
    root.write(&format!("images/{stem}.tex"), &rgba_texture(2, 2, color));
    format!("../mods/enabled/images/{stem}.xml")
}

fn load(
    native: &TestRoot,
    enabled: &TestRoot,
    registered: &[String],
    icons: &[&str],
    inventory: &[DstMapEntityInventoryIcon],
) -> DstMapAssets {
    load_dst_map_assets(
        &native.0,
        &[source(enabled)],
        DstMapIconSources {
            registered_atlases: registered,
            entity_inventory: inventory,
        },
        &[],
        &[],
        &icons.iter().map(|name| (*name).into()).collect::<Vec<_>>(),
        &[],
    )
    .unwrap()
}

fn pixel(output: &DstMapAssets, icon: &str) -> [u8; 4] {
    image::load_from_memory(&output.icons[icon])
        .unwrap()
        .into_rgba8()
        .get_pixel(0, 0)
        .0
}

#[test]
fn native_and_mod_direct_icons_do_not_validate_unrequested_uv_rectangles() {
    for native_atlas in [false, true] {
        let native = TestRoot::new();
        let enabled = TestRoot::new();
        let path = if native_atlas {
            native.write(
                "data/minimap/minimap_data2.xml",
                &with_invalid_uv("minimap_atlas2.tex", "Target.PNG", "unrelated.png"),
            );
            native.write(
                "data/minimap/minimap_atlas2.tex",
                &rgba_texture(2, 2, [20, 40, 60, 255]),
            );
            "minimap/minimap_data2.xml".into()
        } else {
            mod_atlas(
                &enabled,
                "direct",
                &with_invalid_uv("direct.tex", "Target.PNG", "unrelated.png"),
                [20, 40, 60, 255],
            )
        };
        let output = load(&native, &enabled, &[path], &["target.png"], &[]);
        assert!(output.issues.is_empty(), "{:?}", output.issues);
        assert_eq!(pixel(&output, "target.png"), [20, 40, 60, 255]);
        assert!(output.icon_fallbacks.is_empty());
    }
}

#[test]
fn registered_alias_uses_its_actual_element_despite_unrequested_invalid_uvs() {
    let native = TestRoot::new();
    let enabled = TestRoot::new();
    let path = mod_atlas(
        &enabled,
        "alias",
        &with_invalid_uv("alias.tex", "Target.TeX", "unrelated.png"),
        [255, 0, 0, 255],
    );
    mod_atlas(
        &enabled,
        "indexed",
        &icon_atlas("indexed.tex", "target.png"),
        [0, 255, 0, 255],
    );
    let output = load(&native, &enabled, &[path.clone()], &["target.png"], &[]);
    assert!(output.issues.is_empty(), "{:?}", output.issues);
    assert_eq!(pixel(&output, "target.png"), [255, 0, 0, 255]);
    assert_eq!(
        output.icon_fallbacks,
        vec![DstMapIconFallback {
            icon: "target.png".into(),
            element: "Target.TeX".into(),
            atlas: path,
            kind: DstMapIconFallbackKind::RegisteredAlias,
        }]
    );
}

#[test]
fn explicit_inventory_selection_ignores_only_other_keys_and_rejects_its_bad_uv() {
    for native_atlas in [false, true] {
        for selected_bad in [false, true] {
            let native = TestRoot::new();
            let enabled = TestRoot::new();
            let xml = if selected_bad {
                with_invalid_uv("item.tex", "unrelated.png", "actual.tex")
            } else {
                with_invalid_uv("item.tex", "actual.tex", "unrelated.png")
            };
            let path = if native_atlas {
                native.write("data/images/item.xml", &xml);
                native.write(
                    "data/images/item.tex",
                    &rgba_texture(2, 2, [10, 20, 30, 255]),
                );
                "images/item.xml".into()
            } else {
                mod_atlas(&enabled, "item", &xml, [10, 20, 30, 255])
            };
            let output = load(
                &native,
                &enabled,
                &[],
                &["target.png"],
                &[DstMapEntityInventoryIcon {
                    icon: "target.png".into(),
                    atlas: path.clone(),
                    element: "actual.tex".into(),
                }],
            );
            if selected_bad {
                assert!(output.icons.is_empty());
                assert!(output.icon_fallbacks.is_empty());
                assert!(
                    output
                        .issues
                        .iter()
                        .any(|issue| issue.contains("outside the texture")),
                    "{:?}",
                    output.issues
                );
            } else {
                assert!(output.issues.is_empty(), "{:?}", output.issues);
                assert_eq!(pixel(&output, "target.png"), [10, 20, 30, 255]);
                assert_eq!(
                    output.icon_fallbacks[0].kind,
                    DstMapIconFallbackKind::EntityInventory
                );
                assert_eq!(output.icon_fallbacks[0].atlas, path);
                assert_eq!(output.icon_fallbacks[0].element, "actual.tex");
            }
        }
    }
}

#[test]
fn later_shadowed_bad_key_does_not_replace_the_first_source_or_hide_other_icons() {
    let native = TestRoot::new();
    let enabled = TestRoot::new();
    let first = mod_atlas(
        &enabled,
        "first",
        &icon_atlas("first.tex", "target.png"),
        [255, 0, 0, 255],
    );
    let later = mod_atlas(
        &enabled,
        "later",
        &with_invalid_uv("later.tex", "other.png", "TARGET.PNG"),
        [0, 0, 255, 255],
    );
    let output = load(
        &native,
        &enabled,
        &[first, later],
        &["target.png", "other.png"],
        &[],
    );
    assert!(output.issues.is_empty(), "{:?}", output.issues);
    assert_eq!(pixel(&output, "target.png"), [255, 0, 0, 255]);
    assert_eq!(pixel(&output, "other.png"), [0, 0, 255, 255]);
    assert!(output.icon_fallbacks.is_empty());
}

#[test]
fn invalid_requested_direct_or_alias_sources_cannot_borrow_lower_sources() {
    for native_atlas in [false, true] {
        for alias in [false, true] {
            let native = TestRoot::new();
            let enabled = TestRoot::new();
            let safe = mod_atlas(
                &enabled,
                "safe",
                &icon_atlas("safe.tex", "safe.png"),
                [255; 4],
            );
            let key = if alias { "target.tex" } else { "target.png" };
            let path = if native_atlas {
                native.write(
                    "data/minimap/minimap_data2.xml",
                    &with_invalid_uv("minimap_atlas2.tex", "unused.png", key),
                );
                native.write(
                    "data/minimap/minimap_atlas2.tex",
                    &rgba_texture(2, 2, [255; 4]),
                );
                "minimap/minimap_data2.xml".into()
            } else {
                mod_atlas(
                    &enabled,
                    "bad",
                    &with_invalid_uv("bad.tex", "unused.png", key),
                    [255; 4],
                )
            };
            let lower = mod_atlas(
                &enabled,
                "lower",
                &icon_atlas("lower.tex", "target.png"),
                [0, 255, 0, 255],
            );
            let item = mod_atlas(
                &enabled,
                "item",
                &icon_atlas("item.tex", "item.tex"),
                [0, 0, 255, 255],
            );
            let mut registered = vec![safe, path];
            if !alias {
                registered.push(lower);
            }
            let output = load(
                &native,
                &enabled,
                &registered,
                &["target.png", "safe.png"],
                &[DstMapEntityInventoryIcon {
                    icon: "target.png".into(),
                    atlas: item,
                    element: "item.tex".into(),
                }],
            );
            assert!(output.icons.contains_key("safe.png"));
            assert!(!output.icons.contains_key("target.png"));
            assert!(output.icon_fallbacks.is_empty());
            assert_eq!(output.terrain_base_color, Some([100, 80, 60, 255]));
            assert!(
                output
                    .issues
                    .iter()
                    .any(|issue| issue.contains("outside the texture")),
                "{:?}",
                output.issues
            );
        }
    }
}

#[test]
fn selected_reads_validate_trailing_structure_and_unrequested_keys_and_attributes() {
    let valid =
        String::from_utf8(with_invalid_uv("direct.tex", "target.png", "unrelated.png")).unwrap();
    for xml in [
        format!("{valid}<Unexpected/>"),
        valid.replace("name=\"unrelated.png\"", "name=\"../unsafe.png\""),
        valid.replace("u2=\"95\"", ""),
        valid.replace("u2=\"95\"", "u2=\"95\" u2=\"1\""),
    ] {
        let native = TestRoot::new();
        let enabled = TestRoot::new();
        let first = mod_atlas(&enabled, "direct", xml.as_bytes(), [255; 4]);
        let later = mod_atlas(
            &enabled,
            "later",
            &icon_atlas("later.tex", "target.png"),
            [255; 4],
        );
        let output = load(&native, &enabled, &[first, later], &["target.png"], &[]);
        assert!(output.icons.is_empty(), "{xml}");
        assert!(output.icon_fallbacks.is_empty());
        assert!(!output.issues.is_empty());
    }
}

#[test]
fn strict_full_atlas_configuration_and_terrain_parsers_are_unchanged() {
    let xml = with_invalid_uv("map.tex", "target.png", "unrelated.png");
    assert!(atlas::parse_atlas(&xml).is_err());
    assert!(atlas::parse_atlas_with_issues(&xml).is_err());
    assert!(atlas::parse_terrain_atlas(&xml).is_err());
    let (selected, issues) =
        atlas::parse_selected_map_elements(&xml, &BTreeSet::from(["target.png".into()])).unwrap();
    assert!(issues.is_empty());
    assert_eq!(selected.elements.len(), 1);
    assert!(!selected.elements.contains_key("unrelated.png"));
}

#[test]
fn selected_element_duplicates_keep_existing_first_declaration_and_error_semantics() {
    let native = TestRoot::new();
    let enabled = TestRoot::new();
    let xml = br#"<Atlas><Texture filename="same.tex"/><Elements>
        <Element name="Target.PNG" u1="0" u2="0.5" v1="0" v2="1"/>
        <Element name="TARGET.png" u1="0.5" u2="1" v1="0" v2="1"/>
        <Element name="unused.png" u1="0" u2="95" v1="0" v2="1"/>
        </Elements></Atlas>"#;
    let path = mod_atlas(&enabled, "same", xml, [255; 4]);
    let mut pixels = rgba_texture(2, 1, [255, 0, 0, 255]);
    pixels[22..26].copy_from_slice(&[0, 0, 255, 255]);
    enabled.write("images/same.tex", &pixels);
    let output = load(&native, &enabled, &[path.clone()], &["target.png"], &[]);
    assert!(output.issues.is_empty(), "{:?}", output.issues);
    assert_eq!(pixel(&output, "target.png"), [255, 0, 0, 255]);
    let shadowed_bad = String::from_utf8(xml.to_vec())
        .unwrap()
        .replace("u1=\"0.5\" u2=\"1\"", "u1=\"0.5\" u2=\"95\"");
    enabled.write("images/same.xml", shadowed_bad.as_bytes());
    let output = load(&native, &enabled, &[path.clone()], &["target.png"], &[]);
    assert!(output.issues.is_empty(), "{:?}", output.issues);
    assert_eq!(pixel(&output, "target.png"), [255, 0, 0, 255]);
    let indexed = load(&native, &enabled, &[], &["target.png"], &[]);
    assert!(indexed.issues.is_empty(), "{:?}", indexed.issues);
    assert_eq!(pixel(&indexed, "target.png"), [255, 0, 0, 255]);
    assert_eq!(
        indexed.icon_fallbacks[0].kind,
        DstMapIconFallbackKind::IndexedExact
    );
    assert_eq!(indexed.icon_fallbacks[0].element, "Target.PNG");
    let first_bad = String::from_utf8(xml.to_vec())
        .unwrap()
        .replace("u2=\"0.5\"", "u2=\"95\"");
    enabled.write("images/same.xml", first_bad.as_bytes());
    let output = load(&native, &enabled, &[path], &["target.png"], &[]);
    assert!(output.icons.is_empty());
    assert!(
        output
            .issues
            .iter()
            .any(|issue| issue.contains("outside the texture"))
    );
    let indexed = load(&native, &enabled, &[], &["target.png"], &[]);
    assert!(indexed.icons.is_empty());
    assert!(
        indexed
            .issues
            .iter()
            .any(|issue| issue.contains("outside the texture"))
    );
}

#[test]
#[ignore = "reads explicitly supplied complete original Creeps XML/TEX resources"]
fn installed_selected_map_elements_decode_original_icons_without_repairing_unrequested_uvs() {
    let original = PathBuf::from(
        std::env::var_os("LANGAME_DST_SELECTED_SMOKE_MOD_ROOT").expect("original Mod root"),
    );
    let xml = fs::read(original.join("map_icons/creepindedeepicon.xml")).unwrap();
    assert_eq!(xml.len(), 7894);
    assert!(
        atlas::parse_atlas_with_issues(&xml)
            .err()
            .unwrap()
            .contains("outside the texture")
    );
    assert_eq!(
        fs::metadata(original.join("map_icons/creepindedeepicon.tex"))
            .unwrap()
            .len(),
        1638418
    );
    let native = TestRoot::new();
    let sources = [DstMapModSource {
        name: "original_selected".into(),
        root: original,
    }];
    // Test-only expectations from the original author's complete registration
    // order and actual failed native snapshot; no production Mod/icon table.
    let registered = [
        "images/minimap/iron_boulder.xml",
        "images/minimap/seagrass.xml",
        "images/minimap/kelp.xml",
        "images/minimap/vent.xml",
        "images/minimap/entrance_open.xml",
        "images/minimap/entrance_closed.xml",
        "images/minimap/orange_coral.xml",
        "images/minimap/clam.xml",
        "images/minimap/sponge.xml",
        "images/minimap/wormplant.xml",
        "map_icons/creepindedeepicon.xml",
        "map_icons/hamleticon.xml",
    ]
    .map(|path| format!("../mods/original_selected/{path}"));
    let icons = [
        "coral_brain_rock.png",
        "crate.png",
        "limpetrock.png",
        "minimap_coralreef.png",
        "minimap_woodlegs_cage.png",
        "pig_ruins_pot.png",
        "quacken.png",
        "rockmagma.png",
        "vamp_bat_cave.png",
        "wreck.png",
    ]
    .map(String::from);
    let output = load_dst_map_assets(
        &native.0,
        &sources,
        DstMapIconSources {
            registered_atlases: &registered,
            ..Default::default()
        },
        &[],
        &[],
        &icons,
        &[],
    )
    .unwrap();
    assert!(output.issues.is_empty(), "{:?}", output.issues);
    assert_eq!(output.icons.len(), 10);
    assert!(output.icon_fallbacks.is_empty());
    for png in output.icons.values() {
        let pixels = image::load_from_memory(png).unwrap().into_rgba8();
        assert!(pixels.width() > 0 && pixels.width() <= 64);
        assert!(pixels.height() > 0 && pixels.height() <= 64);
        assert!(pixels.pixels().any(|pixel| pixel[3] > 0));
    }
    let mods = ModAssets::new(&sources).unwrap();
    let first_icons = mods
        .icons(
            &registered[10],
            &BTreeSet::from([
                "coral_brain_rock.png",
                "minimap_coralreef.png",
                "minimap_woodlegs_cage.png",
                "quacken.png",
                "wreck.png",
            ]),
            &mut ReadBudget::default(),
        )
        .unwrap();
    assert!(first_icons.issues.is_empty());
    for (name, png) in first_icons.icons {
        assert_eq!(output.icons[&name], png, "{name}: registered first source");
    }
    let bad = load_dst_map_assets(
        &native.0,
        &sources,
        DstMapIconSources {
            registered_atlases: &registered,
            ..Default::default()
        },
        &[],
        &[],
        &[".png".into()],
        &[],
    )
    .unwrap();
    assert!(bad.icons.is_empty());
    assert!(
        bad.issues
            .iter()
            .any(|issue| issue.contains("outside the texture"))
    );
    println!(
        "actual original selected-map artwork: complete_xml_bytes=7894 original_tex_bytes=1638418 icons=10 issues=0 first_registered_source_preserved=true invalid_requested_key_rejected=true"
    );
}
