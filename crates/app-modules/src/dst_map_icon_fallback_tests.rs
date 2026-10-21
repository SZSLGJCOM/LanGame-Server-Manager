use super::super::{
    DstMapModSource, load_dst_map_assets,
    tests::{TestRoot, icon_atlas, rgba_texture},
};
use super::*;

fn source(root: &TestRoot, name: &str) -> DstMapModSource {
    DstMapModSource {
        name: name.into(),
        root: root.0.clone(),
    }
}

fn add_atlas(root: &TestRoot, stem: &str, element: &str, color: [u8; 4]) {
    root.write(
        &format!("images/{stem}.xml"),
        &icon_atlas(&format!("{stem}.tex"), element),
    );
    root.write(&format!("images/{stem}.tex"), &rgba_texture(2, 2, color));
}

#[test]
fn registered_unique_suffix_completion_preserves_actual_name_and_precedes_indexed_exact() {
    let native = TestRoot::new();
    let enabled = TestRoot::new();
    add_atlas(&enabled, "registered", "TreeImage.tex", [0, 255, 0, 255]);
    add_atlas(&enabled, "unregistered", "treeimage.png", [255, 0, 0, 255]);
    enabled.write("invalid.xml", b"<Atlas>broken");
    let result = load_dst_map_assets(
        &native.0,
        &[source(&enabled, "workshop-123")],
        crate::DstMapIconSources {
            registered_atlases: &["../mods/workshop-123/images/registered.xml".into()],
            ..Default::default()
        },
        &[],
        &[],
        &["treeimage.png".into()],
        &[],
    )
    .unwrap();
    assert!(result.issues.is_empty(), "{:?}", result.issues);
    let image = image::load_from_memory(&result.icons["treeimage.png"])
        .unwrap()
        .into_rgba8();
    assert_eq!(image.get_pixel(0, 0).0, [0, 255, 0, 255]);
    assert_eq!(
        result.icon_fallbacks,
        vec![DstMapIconFallback {
            icon: "treeimage.png".into(),
            element: "TreeImage.tex".into(),
            atlas: "../mods/workshop-123/images/registered.xml".into(),
            kind: DstMapIconFallbackKind::RegisteredAlias
        }]
    );
}

#[test]
fn indexed_exact_and_alias_share_a_single_atlas_decode_and_keep_successful_provenance() {
    let native = TestRoot::new();
    let enabled = TestRoot::new();
    enabled.write(
        "maps.xml",
        br#"<Atlas><Texture filename="maps.tex"/><Elements>
        <Element name="exact.png" u1="0" u2="1" v1="0" v2="1"/>
        <Element name="Alias.tex" u1="0" u2="1" v1="0" v2="1"/>
        <Element name="" u1="0" u2="1" v1="0" v2="1"/>
        </Elements></Atlas>"#,
    );
    enabled.write("maps.tex", &rgba_texture(2, 2, [20, 40, 60, 255]));
    let root = super::super::checked_root(&native.0).unwrap();
    let mods = ModAssets::new(&[source(&enabled, "workshop-123")]).unwrap();
    let mut budget = ReadBudget {
        decoded_pixels: ReadBudget::MAX_DECODED_PIXELS - 4,
        ..ReadBudget::default()
    };
    let result = load(
        &root,
        &mods,
        DstMapIconSources::default(),
        &BTreeSet::from(["exact.png", "alias.png"]),
        &mut budget,
    );
    assert!(result.issues.is_empty(), "{:?}", result.issues);
    assert_eq!(result.icons.len(), 2);
    assert_eq!(budget.decoded_pixels, ReadBudget::MAX_DECODED_PIXELS);
    assert_eq!(budget.crop_source_pixels, 8);
    let kinds: BTreeMap<_, _> = result
        .provenance
        .iter()
        .map(|item| (item.icon.as_str(), item.kind))
        .collect();
    assert_eq!(kinds["exact.png"], DstMapIconFallbackKind::IndexedExact);
    assert_eq!(kinds["alias.png"], DstMapIconFallbackKind::IndexedAlias);
    assert_eq!(
        result
            .provenance
            .iter()
            .find(|item| item.icon == "alias.png")
            .unwrap()
            .element,
        "Alias.tex"
    );
}

#[test]
fn declared_exact_broken_textures_never_borrow_suffix_or_unregistered_artwork() {
    for bytes in [None, Some(b"bad texture".as_slice())] {
        let native = TestRoot::new();
        let enabled = TestRoot::new();
        enabled.write("images/broken.xml", &icon_atlas("broken.tex", "same.png"));
        if let Some(bytes) = bytes {
            enabled.write("images/broken.tex", bytes);
        }
        add_atlas(&enabled, "alias", "same.tex", [255; 4]);
        add_atlas(&enabled, "indexed", "same.png", [255; 4]);
        let result = load_dst_map_assets(
            &native.0,
            &[source(&enabled, "workshop-123")],
            crate::DstMapIconSources {
                registered_atlases: &[
                    "../mods/workshop-123/images/broken.xml".into(),
                    "../mods/workshop-123/images/alias.xml".into(),
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
                .any(|issue| issue.contains("broken.xml"))
        );
    }
    let native = TestRoot::new();
    let enabled = TestRoot::new();
    native.write(
        "data/minimap/minimap_data2.xml",
        &icon_atlas("minimap_atlas2.tex", "same.png"),
    );
    add_atlas(&enabled, "alias", "same.tex", [255; 4]);
    let result = load_dst_map_assets(
        &native.0,
        &[source(&enabled, "workshop-123")],
        crate::DstMapIconSources {
            registered_atlases: &["../mods/workshop-123/images/alias.xml".into()],
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
}

#[test]
fn malformed_registered_or_native_atlases_make_exact_source_uncertain_and_disable_completion() {
    for native_failure in [true, false] {
        let native = TestRoot::new();
        let enabled = TestRoot::new();
        add_atlas(&enabled, "alias", "same.tex", [255; 4]);
        let mut registered = vec!["../mods/workshop-123/images/alias.xml".into()];
        if native_failure {
            native.write(
                "data/minimap/minimap_data2.xml",
                b"<Atlas><unknown/></Atlas>",
            );
        } else {
            enabled.write("broken.xml", b"<Atlas><unknown/></Atlas>");
            registered.push("../mods/workshop-123/broken.xml".into());
        }
        let result = load_dst_map_assets(
            &native.0,
            &[source(&enabled, "workshop-123")],
            crate::DstMapIconSources {
                registered_atlases: &registered,
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
        assert!(!result.issues.is_empty());
    }
}

#[test]
fn ambiguous_candidates_never_pick_directory_order_or_fall_through_to_a_lower_tier() {
    for registered in [true, false] {
        let native = TestRoot::new();
        let enabled = TestRoot::new();
        let element = if registered { "same.tex" } else { "same.png" };
        add_atlas(&enabled, "a", element, [255, 0, 0, 255]);
        add_atlas(&enabled, "b", element, [0, 255, 0, 255]);
        add_atlas(
            &enabled,
            "lower",
            if registered { "same.png" } else { "same.tex" },
            [0, 0, 255, 255],
        );
        let paths = if registered {
            vec![
                "../mods/workshop-123/images/a.xml".into(),
                "../mods/workshop-123/images/b.xml".into(),
            ]
        } else {
            vec![]
        };
        let result = load_dst_map_assets(
            &native.0,
            &[source(&enabled, "workshop-123")],
            crate::DstMapIconSources {
                registered_atlases: &paths,
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
                .any(|issue| issue.contains("completion is ambiguous"))
        );
    }
}

#[test]
fn incomplete_index_rejects_an_already_found_unique_candidate() {
    let native = TestRoot::new();
    let enabled = TestRoot::new();
    add_atlas(&enabled, "candidate", "same.png", [255; 4]);
    enabled.write("z_broken.xml", b"<Atlas><unknown/></Atlas>");
    let result = load_dst_map_assets(
        &native.0,
        &[source(&enabled, "workshop-123")],
        crate::DstMapIconSources {
            registered_atlases: &[],
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
            .any(|issue| issue.contains("index is incomplete"))
    );
}

#[test]
fn unusable_selected_completion_never_claims_success_or_substitutes_a_lower_tier() {
    let native = TestRoot::new();
    let enabled = TestRoot::new();
    enabled.write("images/exact.xml", &icon_atlas("exact.tex", "same.png"));
    add_atlas(&enabled, "alias", "same.tex", [255; 4]);
    let result = load_dst_map_assets(
        &native.0,
        &[source(&enabled, "workshop-123")],
        crate::DstMapIconSources {
            registered_atlases: &[],
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
            .any(|issue| issue.contains("completion atlas texture is not installed"))
    );
}

#[test]
fn atlas_index_preserves_its_directory_depth_limit() {
    // Metadata is now bounded by actual cumulative bytes rather than a file
    // count; the independent directory-depth protection remains unchanged.
    let deep = TestRoot::new();
    let path = std::iter::repeat_n("nested", MAX_INDEX_DEPTH + 1)
        .collect::<Vec<_>>()
        .join("/");
    deep.write(&format!("{path}/maps.xml"), b"<Atlas/>");
    let mods = ModAssets::new(&[source(&deep, "workshop-123")]).unwrap();
    assert!(
        indexed_catalog(&mods, &[], &BTreeSet::new(), &mut ReadBudget::default())
            .err()
            .unwrap()
            .contains("depth limit")
    );
}

#[cfg(any(unix, windows))]
#[test]
fn atlas_index_rejects_nested_links_without_reading_external_candidates() {
    let native = TestRoot::new();
    let enabled = TestRoot::new();
    let outside = TestRoot::new();
    add_atlas(&outside, "outside", "same.png", [255; 4]);
    let link = enabled.0.join("linked");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside.0, &link).unwrap();
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let status = std::process::Command::new("cmd.exe")
            .creation_flags(0x0800_0000)
            .args(["/D", "/C", "mklink", "/J"])
            .arg(&link)
            .arg(&outside.0)
            .output()
            .unwrap();
        assert!(status.status.success());
    }
    let result = load_dst_map_assets(
        &native.0,
        &[source(&enabled, "workshop-123")],
        crate::DstMapIconSources {
            registered_atlases: &[],
            ..Default::default()
        },
        &[],
        &[],
        &["same.png".into()],
        &[],
    )
    .unwrap();
    #[cfg(windows)]
    std::fs::remove_dir(&link).unwrap();
    #[cfg(unix)]
    std::fs::remove_file(&link).unwrap();
    assert!(result.icons.is_empty());
    assert!(result.icon_fallbacks.is_empty());
    assert!(
        result
            .issues
            .iter()
            .any(|issue| issue.contains("reparse point"))
    );
}

#[test]
fn missing_registered_sources_never_create_a_false_unique_indexed_candidate() {
    let native = TestRoot::new();
    let enabled = TestRoot::new();
    add_atlas(&enabled, "indexed", "same.png", [255; 4]);
    let root = super::super::checked_root(&native.0).unwrap();
    let mods = ModAssets::new(&[source(&enabled, "workshop-123")]).unwrap();
    for path in [
        "../mods/workshop-123/images/disappeared.xml",
        "minimap/minimap_data2.xml",
    ] {
        // Exercise the completion entry directly: the main pass may already
        // have read this resource before it disappears from the installation.
        let result = load(
            &root,
            &mods,
            DstMapIconSources {
                registered_atlases: &[path.into()],
                ..Default::default()
            },
            &BTreeSet::from(["same.png"]),
            &mut ReadBudget::default(),
        );
        assert!(result.icons.is_empty());
        assert!(result.provenance.is_empty());
        assert!(result.issues.iter().any(|issue| {
            issue.contains(path) && issue.contains("registered minimap atlas disappeared")
        }));
    }
}

#[test]
fn native_completion_keeps_the_native_atlas_file_size_limit() {
    let native = TestRoot::new();
    native.write(
        "data/minimap/minimap_data2.xml",
        &icon_atlas("minimap_atlas2.tex", "same.tex"),
    );
    let path = native.0.join("data/minimap/minimap_atlas2.tex");
    fs::File::create(path)
        .unwrap()
        .set_len(super::super::MAX_ATLAS_BYTES as u64 + 1)
        .unwrap();
    let result = load_dst_map_assets(
        &native.0,
        &[],
        crate::DstMapIconSources {
            registered_atlases: &[],
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
            .any(|issue| issue.contains("size limit"))
    );
}

#[test]
fn unrequested_terrain_uvs_and_engine_paths_do_not_enter_reads_or_crops() {
    for invalid_uv in [false, true] {
        let native = TestRoot::new();
        let enabled = TestRoot::new();
        add_atlas(&enabled, "candidate", "same.png", [255; 4]);
        let mut xml = icon_atlas("data/levels/tiles/not-installed.tex", "01");
        if invalid_uv {
            xml = String::from_utf8(xml)
                .unwrap()
                .replace("u1=\"0\"", "u1=\"NaN\"")
                .into_bytes();
        }
        enabled.write("levels/tiles/terrain.xml", &xml);
        let result = load_dst_map_assets(
            &native.0,
            &[source(&enabled, "workshop-123")],
            crate::DstMapIconSources {
                registered_atlases: &[],
                ..Default::default()
            },
            &[],
            &[],
            &["same.png".into()],
            &[],
        )
        .unwrap();
        assert_eq!(result.icons.len(), 1, "{:?}", result.issues);
        assert_eq!(result.icon_fallbacks.len(), 1);
        assert!(result.issues.is_empty(), "{:?}", result.issues);
    }
}

#[test]
fn completion_metadata_round_trips_with_the_public_wire_names() {
    let value = DstMapIconFallback {
        icon: "same.png".into(),
        element: "same.tex".into(),
        atlas: "../mods/workshop-123/maps.xml".into(),
        kind: DstMapIconFallbackKind::RegisteredAlias,
    };
    let json = serde_json::to_value(&value).unwrap();
    assert_eq!(json["kind"], "registered_alias");
    assert_eq!(
        serde_json::from_value::<DstMapIconFallback>(json).unwrap(),
        value
    );
    for kind in [
        DstMapIconFallbackKind::IndexedExact,
        DstMapIconFallbackKind::IndexedAlias,
    ] {
        let json = serde_json::to_value(kind).unwrap();
        assert!(matches!(
            json.as_str(),
            Some("indexed_exact" | "indexed_alias")
        ));
    }
    assert_eq!(
        serde_json::to_value(DstMapIconFallbackKind::EntityInventory).unwrap(),
        "entity_inventory"
    );
}
