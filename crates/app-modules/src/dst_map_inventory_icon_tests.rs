use super::super::{
    DstMapEntityInventoryIcon, DstMapModSource, load_dst_map_assets,
    tests::{TestRoot, icon_atlas, rgba_texture},
};
use super::*;

fn declaration(atlas: &str, element: &str) -> DstMapEntityInventoryIcon {
    DstMapEntityInventoryIcon {
        icon: "orphan.png".into(),
        atlas: atlas.into(),
        element: element.into(),
    }
}
fn fixture() -> (
    TestRoot,
    TestRoot,
    Vec<DstMapModSource>,
    DstMapEntityInventoryIcon,
) {
    let native = TestRoot::new();
    let enabled = TestRoot::new();
    enabled.write("images/item.xml", &icon_atlas("item.tex", "ActualItem.TeX"));
    enabled.write("images/item.tex", &rgba_texture(2, 2, [10, 20, 30, 255]));
    let sources = vec![DstMapModSource {
        name: "enabled".into(),
        root: enabled.0.clone(),
    }];
    (
        native,
        enabled,
        sources,
        declaration("../mods/enabled/images/item.xml", "actualitem.tex"),
    )
}

#[test]
fn explicit_entity_inventory_can_complete_a_different_icon_name_with_exact_provenance() {
    let (native, _enabled, mods, declared) = fixture();
    let output = load_dst_map_assets(
        &native.0,
        &mods,
        DstMapIconSources {
            entity_inventory: &[declared],
            ..Default::default()
        },
        &[],
        &[],
        &["orphan.png".into()],
        &[],
    )
    .unwrap();
    assert!(output.issues.is_empty(), "{:?}", output.issues);
    assert_eq!(
        image::load_from_memory(&output.icons["orphan.png"])
            .unwrap()
            .into_rgba8()
            .get_pixel(0, 0)
            .0,
        [10, 20, 30, 255]
    );
    assert_eq!(
        output.icon_fallbacks,
        vec![DstMapIconFallback {
            icon: "orphan.png".into(),
            element: "ActualItem.TeX".into(),
            atlas: "../mods/enabled/images/item.xml".into(),
            kind: DstMapIconFallbackKind::EntityInventory,
        }]
    );
}

#[test]
fn entity_inventory_never_bypasses_exact_alias_or_broken_selected_sources() {
    for registered in [false, true] {
        for alias in [false, true] {
            for broken in [false, true] {
                let (native, enabled, mods, declared) = fixture();
                enabled.write(
                    "images/prior.xml",
                    &icon_atlas("prior.tex", if alias { "orphan.tex" } else { "orphan.png" }),
                );
                if !broken {
                    enabled.write("images/prior.tex", &rgba_texture(2, 2, [40, 50, 60, 255]));
                }
                let paths = if registered {
                    vec!["../mods/enabled/images/prior.xml".into()]
                } else {
                    vec![]
                };
                let output = load_dst_map_assets(
                    &native.0,
                    &mods,
                    DstMapIconSources {
                        registered_atlases: &paths,
                        entity_inventory: &[declared],
                    },
                    &[],
                    &[],
                    &["orphan.png".into()],
                    &[],
                )
                .unwrap();
                assert!(
                    output
                        .icon_fallbacks
                        .iter()
                        .all(|item| item.kind != DstMapIconFallbackKind::EntityInventory)
                );
                if broken {
                    assert!(output.icons.is_empty());
                    assert!(!output.issues.is_empty());
                } else {
                    assert_eq!(
                        image::load_from_memory(&output.icons["orphan.png"])
                            .unwrap()
                            .into_rgba8()
                            .get_pixel(0, 0)
                            .0,
                        [40, 50, 60, 255]
                    );
                }
            }
        }
    }
}

#[test]
fn ambiguous_index_or_conflicting_declarations_cannot_borrow_entity_inventory() {
    let (native, enabled, mods, declared) = fixture();
    let conflict = declaration("../mods/enabled/images/other.xml", "actualitem.tex");
    let output = load_dst_map_assets(
        &native.0,
        &mods,
        DstMapIconSources {
            entity_inventory: &[declared.clone(), conflict],
            ..Default::default()
        },
        &[],
        &[],
        &["orphan.png".into()],
        &[],
    )
    .unwrap();
    assert!(output.icons.is_empty());
    assert!(
        output
            .issues
            .iter()
            .any(|issue| issue.contains("conflicting entity"))
    );
    for stem in ["a", "b"] {
        enabled.write(
            &format!("images/{stem}.xml"),
            &icon_atlas(&format!("{stem}.tex"), "orphan.png"),
        );
        enabled.write(&format!("images/{stem}.tex"), &rgba_texture(2, 2, [255; 4]));
    }
    let output = load_dst_map_assets(
        &native.0,
        &mods,
        DstMapIconSources {
            entity_inventory: &[declared],
            ..Default::default()
        },
        &[],
        &[],
        &["orphan.png".into()],
        &[],
    )
    .unwrap();
    assert!(output.icons.is_empty());
    assert!(
        output
            .issues
            .iter()
            .any(|issue| issue.contains("ambiguous"))
    );
}

#[test]
fn unauthorized_missing_and_unsafe_inventory_sources_degrade_only_their_icon() {
    for (path, element) in [
        ("../mods/disabled/images/item.xml", "actualitem.tex"),
        ("../mods/enabled/../item.xml", "actualitem.tex"),
        ("C:/private/item.xml", "actualitem.tex"),
        ("../mods/enabled/images/missing.xml", "actualitem.tex"),
        ("../mods/enabled/images/item.xml", "missing.tex"),
        ("images/inventoryimages1.xml", "actualitem.tex"),
    ] {
        let (native, enabled, mods, _) = fixture();
        enabled.write("images/good.xml", &icon_atlas("good.tex", "good.png"));
        enabled.write("images/good.tex", &rgba_texture(2, 2, [255; 4]));
        let output = load_dst_map_assets(
            &native.0,
            &mods,
            DstMapIconSources {
                entity_inventory: &[declaration(path, element)],
                ..Default::default()
            },
            &[],
            &[],
            &["orphan.png".into(), "good.png".into()],
            &[],
        )
        .unwrap();
        assert!(!output.icons.contains_key("orphan.png"), "{path}");
        assert!(output.icons.contains_key("good.png"));
        assert!(
            output
                .issues
                .iter()
                .any(|issue| issue.contains("DST entity inventory icon orphan.png"))
        );
        assert!(output.terrain_base_color.is_some());
    }
}

#[test]
fn entity_inventory_shares_decode_budget_and_loose_native_path_rules() {
    let (native, _enabled, mods, declared) = fixture();
    let root = super::super::checked_root(&native.0).unwrap();
    let mods = ModAssets::new(&mods).unwrap();
    let mut budget = ReadBudget {
        decoded_pixels: ReadBudget::MAX_DECODED_PIXELS - 3,
        ..Default::default()
    };
    let output = load(
        &root,
        &mods,
        DstMapIconSources {
            entity_inventory: &[declared],
            ..Default::default()
        },
        &BTreeSet::from(["orphan.png"]),
        &mut budget,
    );
    assert!(output.icons.is_empty());
    assert!(
        output
            .issues
            .iter()
            .any(|issue| issue.contains("decoded pixel limit")),
        "{:?}",
        output.issues
    );
    assert_eq!(budget.decoded_pixels, ReadBudget::MAX_DECODED_PIXELS - 3);
    native.write(
        "data/images/declared.xml",
        &icon_atlas("declared.tex", "literal.tex"),
    );
    native.write("data/images/declared.tex", &rgba_texture(2, 2, [255; 4]));
    let output = load_dst_map_assets(
        &native.0,
        &[],
        DstMapIconSources {
            entity_inventory: &[declaration("images/declared.xml", "literal.tex")],
            ..Default::default()
        },
        &[],
        &[],
        &["orphan.png".into()],
        &[],
    )
    .unwrap();
    assert!(output.issues.is_empty(), "{:?}", output.issues);
    assert_eq!(
        output.icon_fallbacks[0].kind,
        DstMapIconFallbackKind::EntityInventory
    );
}

#[test]
#[ignore = "requires an explicitly authorized original inventory XML/TEX pair"]
fn installed_entity_inventory_material_completes_only_the_explicit_declaration() {
    let xml = std::path::PathBuf::from(
        std::env::var_os("LANGAME_DST_INVENTORY_SMOKE_ATLAS").expect("original atlas path"),
    );
    let element =
        std::env::var("LANGAME_DST_INVENTORY_SMOKE_ELEMENT").expect("original declared element");
    let native = TestRoot::new();
    let path = format!(
        "../mods/original_material/{}",
        xml.file_name().unwrap().to_str().unwrap()
    );
    let output = load_dst_map_assets(
        &native.0,
        &[DstMapModSource {
            name: "original_material".into(),
            root: xml.parent().unwrap().into(),
        }],
        DstMapIconSources {
            entity_inventory: &[declaration(&path, &element)],
            ..Default::default()
        },
        &[],
        &[],
        &["orphan.png".into()],
        &[],
    )
    .unwrap();
    assert!(output.issues.is_empty(), "{:?}", output.issues);
    assert_eq!(
        output.icon_fallbacks[0].kind,
        DstMapIconFallbackKind::EntityInventory
    );
    assert_eq!(output.icon_fallbacks[0].atlas, path);
    assert!(
        output.icon_fallbacks[0]
            .element
            .eq_ignore_ascii_case(&element)
    );
    let pixels = image::load_from_memory(&output.icons["orphan.png"])
        .unwrap()
        .into_rgba8();
    assert!(pixels.pixels().any(|pixel| pixel.0[3] > 0));
    println!(
        "actual declared inventory material: xml_bytes={} png_bytes={} dimensions={}x{}",
        std::fs::metadata(&xml).unwrap().len(),
        output.icons["orphan.png"].len(),
        pixels.width(),
        pixels.height()
    );
}
