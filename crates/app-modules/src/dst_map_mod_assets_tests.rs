use super::super::{
    load_dst_map_assets,
    tests::{TestRoot, icon_atlas, rgba_texture},
};
use super::*;

fn source(root: &TestRoot, name: &str) -> DstMapModSource {
    DstMapModSource {
        name: name.into(),
        root: root.0.clone(),
    }
}

fn add_atlas(root: &TestRoot, stem: &str, name: &str, color: [u8; 4]) {
    root.write(
        &format!("images/{stem}.xml"),
        &icon_atlas(&format!("{stem}.tex"), name),
    );
    root.write(&format!("images/{stem}.tex"), &rgba_texture(2, 2, color));
}

#[test]
fn enabled_mod_texture_and_registered_icon_use_only_the_bound_source() {
    let native = TestRoot::new();
    let enabled = TestRoot::new();
    let other_instance = TestRoot::new();
    enabled.write(
        "levels/custom.tex",
        &rgba_texture(256, 256, [30, 60, 90, 255]),
    );
    other_instance.write("levels/custom.tex", &rgba_texture(2, 2, [255, 0, 0, 255]));
    enabled.write("images/maps.xml", b"<Atlas><Texture filename=\"maps.tex\"/><Elements><Element name=\"mod_house.png\" u1=\"0\" u2=\"0.5\" v1=\"0.5\" v2=\"1\"/></Elements></Atlas>");
    let mut texture = rgba_texture(2, 2, [255, 0, 0, 255]);
    texture[26..30].copy_from_slice(&[0, 255, 0, 255]);
    enabled.write("images/maps.tex", &texture);
    let path = "../mods/workshop-123/levels/custom.tex".to_string();
    let result = load_dst_map_assets(
        &native.0,
        &[source(&enabled, "workshop-123")],
        crate::DstMapIconSources {
            registered_atlases: &["../mods/workshop-123/images/maps.xml".into()],
            ..Default::default()
        },
        std::slice::from_ref(&path),
        &[],
        &["mod_house.png".into()],
        &[],
    )
    .unwrap();
    assert!(result.issues.is_empty(), "{:?}", result.issues);
    let terrain = image::load_from_memory(&result.textures[&path])
        .unwrap()
        .into_rgba8();
    assert_eq!(terrain.dimensions(), (128, 128));
    assert_eq!(terrain.get_pixel(1, 1).0, [30, 60, 90, 255]);
    let icon = image::load_from_memory(&result.icons["mod_house.png"])
        .unwrap()
        .into_rgba8();
    assert_eq!(icon.dimensions(), (1, 1));
    assert_eq!(icon.get_pixel(0, 0).0, [0, 255, 0, 255]);
}

#[test]
fn disabled_mod_names_and_other_namespaces_cannot_borrow_an_enabled_root() {
    let native = TestRoot::new();
    let enabled = TestRoot::new();
    add_atlas(&enabled, "maps", "mod_house.png", [255; 4]);
    enabled.write("levels/custom.tex", &rgba_texture(2, 2, [255; 4]));
    native.write(
        "data/levels/textures/mini_grass_noise.tex",
        &rgba_texture(2, 2, [10, 20, 30, 255]),
    );
    let result = load_dst_map_assets(
        &native.0,
        &[source(&enabled, "workshop-123")],
        crate::DstMapIconSources {
            registered_atlases: &["../mods/workshop-456/images/maps.xml".into()],
            ..Default::default()
        },
        &[
            "../mods/workshop-456/levels/custom.tex".into(),
            "levels/textures/mini_grass_noise.tex".into(),
        ],
        &[],
        &["mod_house.png".into()],
        &[],
    )
    .unwrap();
    assert_eq!(result.textures.len(), 1);
    assert!(result.icons.is_empty());
    assert!(
        result
            .issues
            .iter()
            .filter(|issue| issue.contains("not enabled for this shard"))
            .count()
            >= 2
    );
    let no_registered_atlas = load_dst_map_assets(
        &native.0,
        &[source(&enabled, "workshop-123")],
        crate::DstMapIconSources {
            registered_atlases: &[],
            ..Default::default()
        },
        &[],
        &[],
        &["mod_house.png".into()],
        &[],
    )
    .unwrap();
    assert!(no_registered_atlas.icons.contains_key("mod_house.png"));
    assert_eq!(no_registered_atlas.icon_fallbacks.len(), 1);
    assert_eq!(
        no_registered_atlas.icon_fallbacks[0].kind,
        super::super::DstMapIconFallbackKind::IndexedExact
    );
    assert!(no_registered_atlas.issues.is_empty());
}

#[test]
fn virtual_mod_paths_reject_traversal_streams_devices_and_absolute_paths() {
    let native = TestRoot::new();
    let enabled = TestRoot::new();
    let sources = [source(&enabled, "workshop-123")];
    let reader = ModAssets::new(&sources).unwrap();
    for path in [
        "../mods/workshop-123/../secret.tex",
        "../mods/workshop-123/images/../../secret.tex",
        "../mods/workshop-123/./secret.tex",
        "../mods/workshop-123//secret.tex",
        "../mods/workshop-123/C:/secret.tex",
        "../mods/workshop-123/images/map.tex:hidden.tex",
        "../mods/workshop-123/images\\map.tex",
        "../mods/workshop-123/images/NUL.tex",
        "../mods/workshop-123/images/COM1.tex",
        "../mods/workshop-123/images./map.tex",
        "../mods/workshop-123/images /map.tex",
        "../mods/workshop-123/images/map.xml",
        "../../mods/workshop-123/images/map.tex",
        "D:/other-instance/images/map.tex",
        "//server/share/map.tex",
    ] {
        assert!(
            reader.texture(path, &mut ReadBudget::default()).is_err(),
            "{path}"
        );
    }
    let result = load_dst_map_assets(
        &native.0,
        &sources,
        crate::DstMapIconSources {
            registered_atlases: &["../mods/workshop-123/../maps.xml".into()],
            ..Default::default()
        },
        &[],
        &[],
        &["mod_house.png".into()],
        &[],
    )
    .unwrap();
    assert!(result.issues.iter().any(|issue| issue.contains("Unsafe")));
}

#[test]
fn mod_atlas_texture_is_adjacent_and_cannot_select_external_files() {
    let native = TestRoot::new();
    let enabled = TestRoot::new();
    for texture in [
        "../private.tex",
        "C:/private.tex",
        "map.tex:private",
        "CON.tex",
    ] {
        enabled.write("images/maps.xml", &icon_atlas(texture, "mod_house.png"));
        let result = load_dst_map_assets(
            &native.0,
            &[source(&enabled, "workshop-123")],
            crate::DstMapIconSources {
                registered_atlases: &["../mods/workshop-123/images/maps.xml".into()],
                ..Default::default()
            },
            &[],
            &[],
            &["mod_house.png".into()],
            &[],
        )
        .unwrap();
        assert!(result.icons.is_empty(), "{texture}");
        assert!(
            result.issues.iter().any(|issue| issue.contains("filename")),
            "{:?}",
            result.issues
        );
    }
}

#[test]
fn native_atlas_element_names_allow_tex_without_treating_them_as_file_paths() {
    let native = TestRoot::new();
    let enabled = TestRoot::new();
    add_atlas(&enabled, "maps", "mod_character.tex", [20, 40, 60, 255]);
    let result = load_dst_map_assets(
        &native.0,
        &[source(&enabled, "workshop-123")],
        crate::DstMapIconSources {
            registered_atlases: &["../mods/workshop-123/images/maps.xml".into()],
            ..Default::default()
        },
        &[],
        &[],
        &["mod_character.tex".into(), "../private.tex".into()],
        &[],
    )
    .unwrap();
    assert!(result.icons.contains_key("mod_character.tex"));
    assert_eq!(result.icons.len(), 1);
    assert_eq!(result.issues.len(), 1);
    assert!(result.issues[0].contains("Unsupported"));
}

#[test]
fn mod_atlas_lookup_folds_ascii_case_but_preserves_every_requested_name() {
    let native = TestRoot::new();
    let enabled = TestRoot::new();
    enabled.write(
        "images/maps.xml",
        br#"<Atlas><Texture filename="MapTexture.tex"/><Elements>
        <Element name="bambooTree.tex" u1="0" u2="1" v1="0" v2="1"/>
        <Element name="jungleTree.tex" u1="0" u2="1" v1="0" v2="1"/>
        <Element name="limpetRock.tex" u1="0" u2="1" v1="0" v2="1"/>
        </Elements></Atlas>"#,
    );
    enabled.write(
        "images/MapTexture.tex",
        &rgba_texture(2, 2, [20, 40, 60, 255]),
    );
    let requested = [
        "bambootree.tex",
        "bambooTree.tex",
        "BAMBOOTREE.TEX",
        "jungletree.tex",
        "limpetrock.tex",
    ]
    .map(String::from);
    let result = load_dst_map_assets(
        &native.0,
        &[source(&enabled, "workshop-123")],
        crate::DstMapIconSources {
            registered_atlases: &["../mods/workshop-123/images/maps.xml".into()],
            ..Default::default()
        },
        &[],
        &[],
        &requested,
        &[],
    )
    .unwrap();
    assert!(result.issues.is_empty(), "{:?}", result.issues);
    assert_eq!(result.icons.len(), requested.len());
    for name in requested {
        let icon = image::load_from_memory(&result.icons[&name])
            .unwrap()
            .into_rgba8();
        assert_eq!(icon.get_pixel(0, 0).0, [20, 40, 60, 255]);
    }
}

#[test]
fn registered_mod_large_map_icon_is_downsampled_without_the_configuration_icon_limit() {
    let native = TestRoot::new();
    let enabled = TestRoot::new();
    enabled.write("images/maps.xml", &icon_atlas("maps.tex", "large.png"));
    enabled.write(
        "images/maps.tex",
        &rgba_texture(512, 512, [20, 40, 60, 255]),
    );
    let result = load_dst_map_assets(
        &native.0,
        &[source(&enabled, "workshop-123")],
        crate::DstMapIconSources {
            registered_atlases: &["../mods/workshop-123/images/maps.xml".into()],
            ..Default::default()
        },
        &[],
        &[],
        &["large.png".into()],
        &[],
    )
    .unwrap();
    assert!(result.issues.is_empty(), "{:?}", result.issues);
    let icon = image::load_from_memory(&result.icons["large.png"])
        .unwrap()
        .into_rgba8();
    assert_eq!(icon.dimensions(), (64, 64));
    assert!(icon.pixels().all(|pixel| pixel.0 == [20, 40, 60, 255]));
}

#[test]
fn first_registered_case_folded_names_and_unrelated_mod_icons_remain_usable() {
    let native = TestRoot::new();
    let enabled = TestRoot::new();
    add_atlas(&enabled, "before", "AMBIGUOUS.tex", [255, 0, 0, 255]);
    add_atlas(&enabled, "after", "aMbIgUoUs.tex", [0, 0, 255, 255]);
    enabled.write(
        "images/mixed.xml",
        br#"<Atlas><Texture filename="mixed.tex"/><Elements>
        <Element name="ambiguous.tex" u1="0" u2="0.5" v1="0" v2="1"/>
        <Element name="Ambiguous.tex" u1="0.5" u2="1" v1="0" v2="1"/>
        <Element name="space icon.tex" u1="0" u2="1" v1="0" v2="1"/>
        <Element name=".tex" u1="0" u2="1" v1="0" v2="1"/>
        </Elements><StraightAlpha>true</StraightAlpha></Atlas>"#,
    );
    enabled.write("images/mixed.tex", &rgba_texture(2, 2, [20, 40, 60, 128]));
    let paths =
        ["before", "mixed", "after"].map(|stem| format!("../mods/workshop-123/images/{stem}.xml"));
    for (ordered, selected_color) in [
        (paths.to_vec(), [255, 0, 0, 255]),
        (paths.iter().rev().cloned().collect(), [0, 0, 255, 255]),
        (
            vec![paths[1].clone(), paths[0].clone(), paths[2].clone()],
            [20, 40, 60, 128],
        ),
    ] {
        let result = load_dst_map_assets(
            &native.0,
            &[source(&enabled, "workshop-123")],
            crate::DstMapIconSources {
                registered_atlases: &ordered,
                ..Default::default()
            },
            &[],
            &[],
            &[
                "ambiguous.tex".into(),
                "Ambiguous.TEX".into(),
                "space icon.tex".into(),
                ".tex".into(),
            ],
            &[],
        )
        .unwrap();
        assert_eq!(result.icons.len(), 4);
        for name in ["ambiguous.tex", "Ambiguous.TEX"] {
            let icon = image::load_from_memory(&result.icons[name])
                .unwrap()
                .into_rgba8();
            assert_eq!(icon.get_pixel(0, 0).0, selected_color);
        }
        for name in ["space icon.tex", ".tex"] {
            let icon = image::load_from_memory(&result.icons[name])
                .unwrap()
                .into_rgba8();
            assert_eq!(icon.get_pixel(0, 0).0, [20, 40, 60, 128]);
        }
        assert!(result.issues.is_empty(), "{:?}", result.issues);
    }
}

#[test]
fn broken_first_mod_declarations_never_borrow_a_later_atlas() {
    for texture in [None, Some(b"invalid texture".as_slice())] {
        let native = TestRoot::new();
        let enabled = TestRoot::new();
        add_atlas(&enabled, "before", "AMBIGUOUS.tex", [255, 0, 0, 255]);
        add_atlas(&enabled, "after", "aMbIgUoUs.tex", [0, 0, 255, 255]);
        enabled.write(
            "images/mixed.xml",
            br#"<Atlas><Texture filename="mixed.tex"/><Elements>
            <Element name="ambiguous.tex" u1="0" u2="0.5" v1="0" v2="1"/>
            <Element name="Ambiguous.tex" u1="0.5" u2="1" v1="0" v2="1"/>
            <Element name="good.tex" u1="0" u2="1" v1="0" v2="1"/>
            </Elements></Atlas>"#,
        );
        if let Some(bytes) = texture {
            enabled.write("images/mixed.tex", bytes);
        }
        let paths = ["before", "mixed", "after"]
            .map(|stem| format!("../mods/workshop-123/images/{stem}.xml"));
        for ordered in [
            vec![paths[1].clone(), paths[0].clone(), paths[2].clone()],
            vec![paths[1].clone(), paths[2].clone(), paths[0].clone()],
        ] {
            let result = load_dst_map_assets(
                &native.0,
                &[source(&enabled, "workshop-123")],
                crate::DstMapIconSources {
                    registered_atlases: &ordered,
                    ..Default::default()
                },
                &[],
                &[],
                &[
                    "ambiguous.tex".into(),
                    "Ambiguous.TEX".into(),
                    "good.tex".into(),
                ],
                &[],
            )
            .unwrap();
            assert!(result.icons.is_empty());
            assert!(result.icon_fallbacks.is_empty());
            assert!(result.issues.iter().any(|issue| {
                issue.contains("images/mixed.xml")
                    && (issue.contains("not installed") || issue.contains("invalid or oversized"))
            }));
        }
    }
}

#[test]
fn distinct_registered_icons_follow_first_source_order_in_both_directions() {
    let native = TestRoot::new();
    let enabled = TestRoot::new();
    add_atlas(&enabled, "a", "SaMe.png", [255, 0, 0, 255]);
    add_atlas(&enabled, "b", "same.png", [0, 0, 255, 255]);
    add_atlas(&enabled, "c", "SAME.png", [0, 0, 255, 255]);
    add_atlas(&enabled, "unique", "unique.png", [0, 255, 0, 255]);
    let paths =
        ["a", "b", "c", "unique"].map(|stem| format!("../mods/workshop-123/images/{stem}.xml"));
    for (ordered, color) in [
        (paths.to_vec(), [255, 0, 0, 255]),
        (paths.iter().rev().cloned().collect(), [0, 0, 255, 255]),
    ] {
        let result = load_dst_map_assets(
            &native.0,
            &[source(&enabled, "workshop-123")],
            crate::DstMapIconSources {
                registered_atlases: &ordered,
                ..Default::default()
            },
            &[],
            &[],
            &["Same.png".into(), "unique.png".into()],
            &[],
        )
        .unwrap();
        let icon = image::load_from_memory(&result.icons["Same.png"])
            .unwrap()
            .into_rgba8();
        assert_eq!(icon.get_pixel(0, 0).0, color);
        assert!(result.icons.contains_key("unique.png"));
        assert!(result.issues.is_empty(), "{:?}", result.issues);
    }
}

#[test]
fn equivalent_icons_and_repeated_registration_are_accepted_without_conflict() {
    let native = TestRoot::new();
    let enabled = TestRoot::new();
    add_atlas(&enabled, "a", "Same.png", [20, 40, 60, 255]);
    add_atlas(&enabled, "b", "SAME.png", [20, 40, 60, 255]);
    let mut paths = vec!["../mods/workshop-123/images/a.xml".into(); 1000];
    paths.push("../mods/workshop-123/images/b.xml".into());
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
    assert_eq!(result.icons.len(), 1);
    assert!(result.issues.is_empty(), "{:?}", result.issues);
}

#[test]
fn a_later_mod_declaration_keeps_the_first_native_icon() {
    let native = TestRoot::new();
    let enabled = TestRoot::new();
    native.write(
        "data/minimap/minimap_data2.xml",
        &icon_atlas("minimap_atlas2.tex", "PigKing.PNG"),
    );
    native.write(
        "data/minimap/minimap_atlas2.tex",
        &rgba_texture(2, 2, [255, 0, 0, 255]),
    );
    add_atlas(&enabled, "maps", "pigking.png", [0, 0, 255, 255]);
    let result = load_dst_map_assets(
        &native.0,
        &[source(&enabled, "workshop-123")],
        crate::DstMapIconSources {
            registered_atlases: &["../mods/workshop-123/images/maps.xml".into()],
            ..Default::default()
        },
        &[],
        &[],
        &["Pigking.png".into()],
        &[],
    )
    .unwrap();
    let icon = image::load_from_memory(&result.icons["Pigking.png"])
        .unwrap()
        .into_rgba8();
    assert_eq!(icon.get_pixel(0, 0).0, [255, 0, 0, 255]);
    assert!(result.issues.is_empty(), "{:?}", result.issues);
}

#[test]
fn per_asset_and_cumulative_read_decode_budgets_are_enforced() {
    let enabled = TestRoot::new();
    enabled.write("levels/large.tex", &vec![0; MAX_MOD_TEXTURE_BYTES + 1]);
    let reader = ModAssets::new(&[source(&enabled, "workshop-123")]).unwrap();
    assert!(
        reader
            .texture(
                "../mods/workshop-123/levels/large.tex",
                &mut ReadBudget::default()
            )
            .unwrap_err()
            .contains("size limit")
    );
    let bytes = rgba_texture(2, 2, [255; 4]);
    enabled.write("levels/small.tex", &bytes);
    let mut budget = ReadBudget {
        source_bytes: ReadBudget::MAX_SOURCE_BYTES,
        ..ReadBudget::default()
    };
    assert!(
        reader
            .texture("../mods/workshop-123/levels/small.tex", &mut budget)
            .unwrap_err()
            .contains("cumulative source")
    );
    let mut budget = ReadBudget {
        decoded_pixels: ReadBudget::MAX_DECODED_PIXELS - 3,
        ..ReadBudget::default()
    };
    assert!(
        budget
            .decode(&bytes)
            .unwrap_err()
            .contains("cumulative decoded")
    );
    assert_eq!(budget.decoded_pixels, ReadBudget::MAX_DECODED_PIXELS - 3);
    let native = TestRoot::new();
    enabled.write("levels/base.tex", &rgba_texture(2, 2, [18, 36, 54, 255]));
    add_atlas(&enabled, "maps", "safe.png", [15, 45, 75, 255]);
    let texture_path = "../mods/workshop-123/levels/small.tex".to_string();
    let base_path = "../mods/workshop-123/levels/base.tex".to_string();
    assert_eq!(MAX_REGISTERED_ATLASES, 512);
    let mut atlases = (0..=MAX_REGISTERED_ATLASES)
        .map(|i| format!("../mods/workshop-123/{i}.xml"))
        .collect::<Vec<_>>();
    atlases[0] = "../mods/workshop-123/images/maps.xml".into();
    let at_limit = load_dst_map_assets(
        &native.0,
        &[source(&enabled, "workshop-123")],
        crate::DstMapIconSources {
            registered_atlases: &atlases[..MAX_REGISTERED_ATLASES],
            ..Default::default()
        },
        std::slice::from_ref(&texture_path),
        std::slice::from_ref(&base_path),
        &["safe.png".into()],
        &[],
    )
    .unwrap();
    assert!(at_limit.issues.is_empty(), "{:?}", at_limit.issues);
    assert_eq!(at_limit.icons.len(), 1);
    assert_eq!(
        image::load_from_memory(&at_limit.icons["safe.png"])
            .unwrap()
            .into_rgba8()
            .get_pixel(0, 0)
            .0,
        [15, 45, 75, 255]
    );
    assert!(at_limit.icon_fallbacks.is_empty());
    let over_limit = load_dst_map_assets(
        &native.0,
        &[source(&enabled, "workshop-123")],
        crate::DstMapIconSources {
            registered_atlases: &atlases,
            ..Default::default()
        },
        std::slice::from_ref(&texture_path),
        std::slice::from_ref(&base_path),
        &["safe.png".into()],
        &[],
    )
    .unwrap();
    assert!(over_limit.icons.is_empty());
    assert!(over_limit.icon_fallbacks.is_empty());
    assert_eq!(over_limit.issues.len(), 1, "{:?}", over_limit.issues);
    assert!(over_limit.issues[0].contains("atlas item limit (512)"));
    assert!(over_limit.issues[0].contains("icon artwork was omitted"));
    assert_eq!(over_limit.terrain_base_color, Some([100, 80, 60, 255]));
    assert_eq!(over_limit.terrain_base_colors.len(), 1);
    assert_eq!(
        over_limit.terrain_base_colors[&base_path],
        [18, 36, 54, 255]
    );
    assert_eq!(over_limit.textures.len(), 1);
    let terrain = image::load_from_memory(&over_limit.textures[&texture_path])
        .unwrap()
        .into_rgba8();
    assert_eq!(terrain.dimensions(), (2, 2));
    assert_eq!(terrain.get_pixel(1, 1).0, [255; 4]);
}

#[test]
fn ambiguous_mod_source_binding_is_rejected_instead_of_picking_an_instance_copy() {
    let first = TestRoot::new();
    let second = TestRoot::new();
    first.write("terrain.tex", &rgba_texture(2, 2, [255, 0, 0, 255]));
    second.write("terrain.tex", &rgba_texture(2, 2, [0, 0, 255, 255]));
    let reader = ModAssets::new(&[
        source(&first, "workshop-123"),
        source(&second, "workshop-123"),
    ])
    .unwrap();
    assert!(
        reader
            .texture(
                "../mods/workshop-123/terrain.tex",
                &mut ReadBudget::default()
            )
            .unwrap_err()
            .contains("conflicting authorized")
    );
}

#[cfg(any(unix, windows))]
#[test]
fn linked_mod_roots_and_nested_asset_directories_cannot_escape_authorized_roots() {
    let native = TestRoot::new();
    let enabled = TestRoot::new();
    let outside = TestRoot::new();
    outside.write("terrain.tex", &rgba_texture(2, 2, [255; 4]));
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
        assert!(
            status.status.success(),
            "{}",
            String::from_utf8_lossy(&status.stderr)
        );
    }
    let nested = load_dst_map_assets(
        &native.0,
        &[source(&enabled, "workshop-123")],
        crate::DstMapIconSources {
            registered_atlases: &[],
            ..Default::default()
        },
        &["../mods/workshop-123/linked/terrain.tex".into()],
        &[],
        &[],
        &[],
    )
    .unwrap();
    assert!(nested.textures.is_empty());
    assert!(nested.issues.iter().any(|issue| issue.contains("reparse")));
    let root_source = DstMapModSource {
        name: "workshop-123".into(),
        root: link.clone(),
    };
    let root = load_dst_map_assets(
        &native.0,
        &[root_source],
        crate::DstMapIconSources {
            registered_atlases: &[],
            ..Default::default()
        },
        &["../mods/workshop-123/terrain.tex".into()],
        &[],
        &[],
        &[],
    )
    .unwrap();
    assert!(root.textures.is_empty());
    assert!(root.issues.iter().any(|issue| issue.contains("reparse")));
    // Remove only the created link, never recursively traverse its target.
    #[cfg(windows)]
    std::fs::remove_dir(&link).unwrap();
    #[cfg(unix)]
    std::fs::remove_file(&link).unwrap();
    assert!(outside.0.join("terrain.tex").is_file());
}
