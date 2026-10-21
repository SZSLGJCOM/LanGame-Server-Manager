use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

pub(super) struct TestRoot(pub(super) PathBuf);

impl TestRoot {
    pub(super) fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "langame-map-art-{}-{unique}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let fixture = Self(root);
        fixture.write(
            "data/levels/tiles/map_edge.tex",
            &rgba_texture(2, 2, [100, 80, 60, 255]),
        );
        fixture
    }

    pub(super) fn write(&self, relative: &str, bytes: &[u8]) {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        assert!(self.0.starts_with(std::env::temp_dir()));
        fs::remove_dir_all(&self.0).expect("remove owned map artwork fixture");
    }
}

pub(super) fn rgba_texture(width: u16, height: u16, pixel: [u8; 4]) -> Vec<u8> {
    let mut bytes = b"KTEX".to_vec();
    let flags = 0xfffc_0000u32 | (1 << 13) | (1 << 9) | (4 << 4);
    bytes.extend_from_slice(&flags.to_le_bytes());
    bytes.extend_from_slice(&width.to_le_bytes());
    bytes.extend_from_slice(&height.to_le_bytes());
    bytes.extend_from_slice(&(width * 4).to_le_bytes());
    bytes.extend_from_slice(&(u32::from(width) * u32::from(height) * 4).to_le_bytes());
    for _ in 0..u32::from(width) * u32::from(height) {
        bytes.extend_from_slice(&pixel);
    }
    bytes
}

pub(super) fn icon_atlas(texture: &str, name: &str) -> Vec<u8> {
    format!("<Atlas><Texture filename=\"{texture}\"/><Elements><Element name=\"{name}\" u1=\"0\" u2=\"1\" v1=\"0\" v2=\"1\"/></Elements></Atlas>").into_bytes()
}

#[test]
fn installed_artwork_decodes_and_limits_texture_and_icon_dimensions() {
    let root = TestRoot::new();
    root.write(
        "data/levels/textures/mini_grass_noise.tex",
        &rgba_texture(256, 128, [64, 32, 0, 128]),
    );
    root.write(
        "data/minimap/minimap_data2.xml",
        &icon_atlas("minimap_atlas2.tex", "pigking.png"),
    );
    root.write(
        "data/minimap/minimap_atlas2.tex",
        &rgba_texture(128, 128, [0, 100, 0, 255]),
    );
    let result = load_dst_map_assets(
        &root.0,
        &[],
        crate::DstMapIconSources {
            registered_atlases: &[],
            ..Default::default()
        },
        &["levels/textures/mini_grass_noise.tex".into()],
        &[],
        &["pigking.png".into()],
        &[],
    )
    .unwrap();
    assert!(result.issues.is_empty());
    assert_eq!(result.terrain_base_color, Some([100, 80, 60, 255]));
    let terrain = image::load_from_memory(&result.textures["levels/textures/mini_grass_noise.tex"])
        .unwrap()
        .into_rgba8();
    assert_eq!(terrain.dimensions(), (128, 64));
    assert_eq!(terrain.get_pixel(30, 30).0, [64, 32, 0, 255]);
    let icon = image::load_from_memory(&result.icons["pigking.png"]).unwrap();
    assert_eq!((icon.width(), icon.height()), (64, 64));
}

#[test]
fn native_and_mod_noise_keep_stored_rgb_even_at_zero_alpha_before_resize() {
    let root = TestRoot::new();
    let enabled = TestRoot::new();
    let mut noise = rgba_texture(256, 128, [40, 80, 120, 128]);
    for y in 0..128 {
        for x in 128..256 {
            let offset = 18 + (y * 256 + x) * 4;
            noise[offset..offset + 4].copy_from_slice(&[90, 30, 60, 0]);
        }
    }
    root.write("data/levels/textures/mini_shader_noise.tex", &noise);
    enabled.write("levels/noise.tex", &noise);
    let mut alpha_material = rgba_texture(2, 1, [40, 80, 120, 128]);
    alpha_material[22..26].copy_from_slice(&[90, 30, 60, 0]);
    root.write("data/images/minimap_paper.tex", &alpha_material);
    root.write(
        "data/minimap/minimap_data2.xml",
        &icon_atlas("minimap_atlas2.tex", "alpha_control.png"),
    );
    root.write("data/minimap/minimap_atlas2.tex", &alpha_material);
    let paths = [
        "levels/textures/mini_shader_noise.tex",
        "../mods/shader_pack/levels/noise.tex",
        "images/minimap_paper.tex",
    ]
    .map(String::from);
    let result = load_dst_map_assets(
        &root.0,
        &[DstMapModSource {
            name: "shader_pack".into(),
            root: enabled.0.clone(),
        }],
        crate::DstMapIconSources {
            registered_atlases: &[],
            ..Default::default()
        },
        &paths,
        &[],
        &["alpha_control.png".into()],
        &[],
    )
    .unwrap();
    assert!(result.issues.is_empty(), "{:?}", result.issues);
    for path in &paths[..2] {
        let pixels = image::load_from_memory(&result.textures[path])
            .unwrap()
            .into_rgba8();
        assert_eq!(pixels.dimensions(), (128, 64));
        assert_eq!(pixels.get_pixel(16, 16).0, [40, 80, 120, 255]);
        assert_eq!(pixels.get_pixel(110, 16).0, [90, 30, 60, 255]);
    }
    // Paper and icons retain their established premultiplied-alpha decoding.
    for bytes in [
        &result.textures[&paths[2]],
        &result.icons["alpha_control.png"],
    ] {
        let pixels = image::load_from_memory(bytes).unwrap().into_rgba8();
        assert_eq!(pixels.dimensions(), (2, 1));
        assert_eq!(pixels.get_pixel(0, 0).0, [80, 159, 239, 128]);
        assert_eq!(pixels.get_pixel(1, 0).0, [0; 4]);
    }
}

#[test]
fn atlas_uvs_select_the_first_registered_native_region() {
    let root = TestRoot::new();
    root.write(
        "data/minimap/minimap_data1.xml",
        b"<Atlas><Texture filename=\"minimap_atlas1.tex\"/><Elements><Element name=\"pigking.png\" u1=\"0\" u2=\"0.5\" v1=\"0.5\" v2=\"1\"/></Elements></Atlas>",
    );
    root.write(
        "data/minimap/minimap_atlas1.tex",
        &rgba_texture(2, 2, [0, 0, 255, 255]),
    );
    root.write(
        "data/minimap/minimap_data2.xml",
        &icon_atlas("minimap_atlas2.tex", "pigking.png"),
    );
    let mut pixels = rgba_texture(2, 2, [0, 0, 255, 255]);
    pixels[26..30].copy_from_slice(&[255, 0, 0, 255]);
    root.write("data/minimap/minimap_atlas1.tex", &pixels);
    root.write(
        "data/minimap/minimap_atlas2.tex",
        &rgba_texture(2, 2, [0, 0, 255, 255]),
    );
    let result = load_dst_map_assets(
        &root.0,
        &[],
        crate::DstMapIconSources {
            registered_atlases: &[],
            ..Default::default()
        },
        &[],
        &[],
        &["pigking.png".into()],
        &[],
    )
    .unwrap();
    let icon = image::load_from_memory(&result.icons["pigking.png"])
        .unwrap()
        .into_rgba8();
    assert_eq!(icon.dimensions(), (1, 1));
    assert_eq!(icon.get_pixel(0, 0).0, [255, 0, 0, 255]);
}

#[test]
fn native_atlas_case_insensitive_lookup_preserves_suffix_precedence_and_request_spelling() {
    let root = TestRoot::new();
    for (suffix, name, color) in [
        ("2", "PigKing.png", [255, 0, 0, 255]),
        ("1", "pigKing.PNG", [0, 0, 255, 255]),
    ] {
        root.write(
            &format!("data/minimap/minimap_data{suffix}.xml"),
            &icon_atlas(&format!("minimap_atlas{suffix}.tex"), name),
        );
        root.write(
            &format!("data/minimap/minimap_atlas{suffix}.tex"),
            &rgba_texture(2, 2, color),
        );
    }
    let result = load_dst_map_assets(
        &root.0,
        &[],
        crate::DstMapIconSources {
            registered_atlases: &[],
            ..Default::default()
        },
        &[],
        &[],
        &["pigking.png".into(), "PIGKING.PNG".into()],
        &[],
    )
    .unwrap();
    assert!(result.issues.is_empty(), "{:?}", result.issues);
    assert_eq!(result.icons.len(), 2);
    for name in ["pigking.png", "PIGKING.PNG"] {
        let icon = image::load_from_memory(&result.icons[name])
            .unwrap()
            .into_rgba8();
        assert_eq!(icon.get_pixel(0, 0).0, [0, 0, 255, 255]);
    }
}

#[test]
fn native_map_atlas_large_icon_is_downsampled_to_the_map_pixel_budget() {
    let root = TestRoot::new();
    root.write(
        "data/minimap/minimap_data2.xml",
        &icon_atlas("minimap_atlas2.tex", "large.png"),
    );
    root.write(
        "data/minimap/minimap_atlas2.tex",
        &rgba_texture(512, 512, [20, 40, 60, 255]),
    );
    let result = load_dst_map_assets(
        &root.0,
        &[],
        crate::DstMapIconSources {
            registered_atlases: &[],
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
fn crop_source_pixel_budget_has_an_exact_cumulative_limit_and_never_resets_on_rejection() {
    let rectangle = atlas::parse_atlas(&icon_atlas("maps.tex", "icon.png"))
        .unwrap()
        .elements["icon.png"];
    // A 128x64 input costs all 8192 source pixels, not its 64x32 output.
    let source = RgbaImage::from_pixel(128, 64, image::Rgba([255; 4]));
    let source_pixels = 128 * 64;
    let mut exact = ReadBudget {
        crop_source_pixels: ReadBudget::MAX_CROP_SOURCE_PIXELS - source_pixels,
        ..ReadBudget::default()
    };
    assert_eq!(
        exact.crop_icon(&source, rectangle).unwrap().dimensions(),
        (64, 32)
    );
    assert_eq!(exact.crop_source_pixels, ReadBudget::MAX_CROP_SOURCE_PIXELS);
    assert_eq!(
        exact.decoded_pixels, 0,
        "crop and decode budgets are independent"
    );
    let one_pixel = RgbaImage::from_pixel(1, 1, image::Rgba([255; 4]));
    assert!(
        exact
            .crop_icon(&one_pixel, rectangle)
            .unwrap_err()
            .contains("cumulative crop source")
    );
    assert_eq!(exact.crop_source_pixels, ReadBudget::MAX_CROP_SOURCE_PIXELS);

    let previous = ReadBudget::MAX_CROP_SOURCE_PIXELS - source_pixels + 1;
    let mut over_by_one = ReadBudget {
        crop_source_pixels: previous,
        ..ReadBudget::default()
    };
    assert!(
        over_by_one
            .crop_icon(&source, rectangle)
            .unwrap_err()
            .contains("cumulative crop source")
    );
    assert_eq!(over_by_one.crop_source_pixels, previous);
    over_by_one.crop_icon(&one_pixel, rectangle).unwrap();
    assert_eq!(over_by_one.crop_source_pixels, previous + 1);

    let mut overflowing = ReadBudget {
        crop_source_pixels: usize::MAX,
        ..ReadBudget::default()
    };
    assert!(overflowing.crop_icon(&one_pixel, rectangle).is_err());
    assert_eq!(overflowing.crop_source_pixels, usize::MAX);
    let mut invalid_source = ReadBudget::default();
    assert!(
        invalid_source
            .crop_icon(&RgbaImage::new(4097, 1), rectangle)
            .is_err()
    );
    assert_eq!(invalid_source.crop_source_pixels, 0);
}

#[test]
fn native_and_mod_atlas_crops_share_one_budget_and_cannot_recover_it_after_rejection() {
    let root = TestRoot::new();
    // read_icons is internal and expects the canonical root established by
    // load_dst_map_assets before the checked file reader compares final paths.
    let install_root = checked_root(&root.0).unwrap();
    root.write(
        "data/minimap/minimap_data2.xml",
        &icon_atlas("minimap_atlas2.tex", "native.png"),
    );
    root.write(
        "data/minimap/minimap_atlas2.tex",
        &rgba_texture(2, 2, [255; 4]),
    );
    let enabled = TestRoot::new();
    enabled.write(
        "maps.xml",
        br#"<Atlas><Texture filename="maps.tex"/><Elements>
        <Element name="a.png" u1="0" u2="1" v1="0" v2="1"/>
        <Element name="b.png" u1="0" u2="1" v1="0" v2="1"/>
        </Elements></Atlas>"#,
    );
    enabled.write("maps.tex", &rgba_texture(2, 2, [255; 4]));
    let mods = ModAssets::new(&[DstMapModSource {
        name: "workshop-123".into(),
        root: enabled.0.clone(),
    }])
    .unwrap();
    let requested = BTreeSet::from(["native.png"]);
    let mut budget = ReadBudget {
        crop_source_pixels: ReadBudget::MAX_CROP_SOURCE_PIXELS - 8,
        ..ReadBudget::default()
    };
    let native = read_icons(&install_root, "2", &requested, false, &mut budget).unwrap();
    assert!(native.icons.contains_key("native.png"));
    assert_eq!(
        budget.crop_source_pixels,
        ReadBudget::MAX_CROP_SOURCE_PIXELS - 4
    );
    let result = mods
        .icons(
            "../mods/workshop-123/maps.xml",
            &BTreeSet::from(["a.png", "b.png"]),
            &mut budget,
        )
        .unwrap();
    assert!(result.icons.contains_key("a.png"));
    assert!(!result.icons.contains_key("b.png"));
    assert!(
        result
            .issues
            .iter()
            .any(|issue| issue.contains("b.png") && issue.contains("cumulative crop source"))
    );
    assert_eq!(
        budget.crop_source_pixels,
        ReadBudget::MAX_CROP_SOURCE_PIXELS
    );
    let native_again = read_icons(&install_root, "2", &requested, false, &mut budget).unwrap();
    assert!(native_again.icons.is_empty());
    assert!(
        native_again
            .issues
            .iter()
            .any(|issue| issue.contains("native.png") && issue.contains("cumulative crop source"))
    );
    assert_eq!(
        budget.crop_source_pixels,
        ReadBudget::MAX_CROP_SOURCE_PIXELS
    );
}

#[test]
fn broken_first_native_declarations_cannot_borrow_later_native_or_mod_artwork() {
    for conflicting_suffix in ["2", "1"] {
        for texture in [None, Some(b"invalid texture".as_slice())] {
            let root = TestRoot::new();
            let enabled = TestRoot::new();
            for suffix in ["2", "1", ""] {
                let texture_name = format!("minimap_atlas{suffix}.tex");
                if suffix == conflicting_suffix {
                    root.write(
                        &format!("data/minimap/minimap_data{suffix}.xml"),
                        format!(
                            "<Atlas><Texture filename=\"{texture_name}\"/><Elements>
                            <Element name=\"bambooTree.tex\" u1=\"0\" u2=\"0.5\" v1=\"0\" v2=\"1\"/>
                            <Element name=\"BAMBOOTREE.TEX\" u1=\"0.5\" u2=\"1\" v1=\"0\" v2=\"1\"/>
                            <Element name=\"good.tex\" u1=\"0\" u2=\"1\" v1=\"0\" v2=\"1\"/>
                            </Elements></Atlas>"
                        )
                        .as_bytes(),
                    );
                    if let Some(bytes) = texture {
                        root.write(&format!("data/minimap/{texture_name}"), bytes);
                    }
                } else {
                    root.write(
                        &format!("data/minimap/minimap_data{suffix}.xml"),
                        &icon_atlas(
                            &texture_name,
                            if suffix == "1" && conflicting_suffix == "2" {
                                "unrelated.png"
                            } else {
                                "BambooTree.tex"
                            },
                        ),
                    );
                    root.write(
                        &format!("data/minimap/{texture_name}"),
                        &rgba_texture(2, 2, [20, 40, 60, 255]),
                    );
                }
            }
            enabled.write("maps.xml", &icon_atlas("maps.tex", "bambootree.tex"));
            enabled.write("maps.tex", &rgba_texture(2, 2, [20, 40, 60, 255]));
            let result = load_dst_map_assets(
                &root.0,
                &[DstMapModSource {
                    name: "workshop-123".into(),
                    root: enabled.0.clone(),
                }],
                crate::DstMapIconSources {
                    registered_atlases: &["../mods/workshop-123/maps.xml".into()],
                    ..Default::default()
                },
                &[],
                &[],
                &[
                    "bambootree.tex".into(),
                    "BambooTree.TEX".into(),
                    "good.tex".into(),
                ],
                &[],
            )
            .unwrap();
            assert!(result.icons.is_empty());
            assert!(result.issues.iter().any(|issue| {
                issue.contains("not installed") || issue.contains("invalid or oversized")
            }));
        }
    }
}

#[test]
fn missing_or_corrupt_artwork_reports_issues_without_hiding_usable_textures() {
    let root = TestRoot::new();
    root.write(
        "data/levels/textures/mini_grass_noise.tex",
        &rgba_texture(2, 2, [255; 4]),
    );
    root.write(
        "data/levels/textures/mini_forest_noise.tex",
        b"not a texture",
    );
    let result = load_dst_map_assets(
        &root.0,
        &[],
        crate::DstMapIconSources {
            registered_atlases: &[],
            ..Default::default()
        },
        &[
            "levels/textures/mini_grass_noise.tex".into(),
            "levels/textures/mini_forest_noise.tex".into(),
            "levels/textures/Ground_noise_moon_fungus_mini.tex".into(),
            "../mods/workshop-1/levels/textures/mini_islands.tex".into(),
        ],
        &[],
        &["unknown_mod_icon.png".into()],
        &[],
    )
    .unwrap();
    assert_eq!(result.textures.len(), 1);
    assert!(result.icons.is_empty());
    assert_eq!(result.issues.len(), 4);
    assert!(
        result
            .issues
            .iter()
            .any(|issue| issue.contains("invalid or oversized"))
    );
    assert!(
        result
            .issues
            .iter()
            .any(|issue| issue.contains("unknown_mod_icon.png"))
    );
}

#[test]
fn requests_reject_each_unsafe_source_without_reading_it() {
    let root = TestRoot::new();
    for path in [
        "../secret.tex",
        "levels/textures/../secret.tex",
        "levels/textures/mini_x/secret.tex",
        "images/secret.tex",
        "C:/secret.tex",
        "levels\\textures\\mini_grass.tex",
        "levels/textures/mini_x.tex:secret",
    ] {
        let result = load_dst_map_assets(
            &root.0,
            &[],
            crate::DstMapIconSources {
                registered_atlases: &[],
                ..Default::default()
            },
            &[path.into()],
            &[],
            &[],
            &[],
        )
        .unwrap();
        assert!(result.textures.is_empty(), "{path}");
        assert!(result.issues[0].contains("Unsupported"), "{path}");
    }
    // Atlas keys may start with a dot; they never become filesystem paths.
    for icon in [
        "../secret.png",
        "sub/icon.png",
        "secret.xml",
        "C:\\secret.png",
        "secret\0.png",
    ] {
        let result = load_dst_map_assets(
            &root.0,
            &[],
            crate::DstMapIconSources {
                registered_atlases: &[],
                ..Default::default()
            },
            &[],
            &[],
            &[icon.into()],
            &[],
        )
        .unwrap();
        assert!(result.icons.is_empty(), "{icon}");
        assert!(result.issues[0].contains("Unsupported"), "{icon}");
    }
}

#[test]
fn request_limits_count_distinct_assets_and_return_each_asset_once() {
    let root = TestRoot::new();
    root.write(
        "data/images/minimap_paper.tex",
        &rgba_texture(2, 2, [255; 4]),
    );
    let result = load_dst_map_assets(
        &root.0,
        &[],
        crate::DstMapIconSources {
            registered_atlases: &[],
            ..Default::default()
        },
        &vec!["images/minimap_paper.tex".into(); 1000],
        &[],
        &[],
        &[],
    )
    .unwrap();
    assert_eq!(result.textures.len(), 1);
    assert!(result.issues.is_empty());
    let textures = load_dst_map_assets(
        &root.0,
        &[],
        crate::DstMapIconSources {
            registered_atlases: &[],
            ..Default::default()
        },
        &(0..=MAX_TEXTURES)
            .map(|i| format!("levels/textures/mini_{i}.tex"))
            .collect::<Vec<_>>(),
        &[],
        &[],
        &[],
    )
    .unwrap();
    assert!(
        textures
            .issues
            .iter()
            .any(|issue| issue.contains("texture item limit"))
    );
    let icons = load_dst_map_assets(
        &root.0,
        &[],
        crate::DstMapIconSources {
            registered_atlases: &[],
            ..Default::default()
        },
        &[],
        &[],
        &(0..=MAX_ICONS)
            .map(|i| format!("icon_{i}.png"))
            .collect::<Vec<_>>(),
        &[],
    )
    .unwrap();
    assert!(
        icons
            .issues
            .iter()
            .any(|issue| issue.contains("icon item limit"))
    );
}

#[test]
fn artwork_isolation_keeps_other_categories_when_one_request_limit_is_exceeded() {
    let root = TestRoot::new();
    let textures: Vec<_> = (0..=MAX_TEXTURES)
        .map(|i| format!("levels/textures/mini_{i:03}.tex"))
        .collect();
    for path in &textures {
        root.write(&format!("data/{path}"), &rgba_texture(2, 2, [255; 4]));
    }
    root.write(
        "data/minimap/minimap_data2.xml",
        &icon_atlas("minimap_atlas2.tex", "safe.png"),
    );
    root.write(
        "data/minimap/minimap_atlas2.tex",
        &rgba_texture(2, 2, [255; 4]),
    );
    let result = load_dst_map_assets(
        &root.0,
        &[],
        crate::DstMapIconSources {
            registered_atlases: &[],
            ..Default::default()
        },
        &textures,
        &[],
        &["safe.png".into()],
        &[],
    )
    .unwrap();
    assert_eq!(result.textures.len(), MAX_TEXTURES);
    assert!(result.icons.contains_key("safe.png"));
    assert!(
        result
            .issues
            .iter()
            .any(|issue| issue.contains("texture item limit"))
    );

    let icons: Vec<_> = (0..=MAX_ICONS)
        .map(|i| format!("icon_{i:03}.png"))
        .collect();
    let elements = icons
        .iter()
        .map(|name| format!("<Element name=\"{name}\" u1=\"0\" u2=\"1\" v1=\"0\" v2=\"1\"/>"))
        .collect::<String>();
    root.write("data/minimap/minimap_data2.xml", format!("<Atlas><Texture filename=\"minimap_atlas2.tex\"/><Elements>{elements}</Elements></Atlas>").as_bytes());
    let result = load_dst_map_assets(
        &root.0,
        &[],
        crate::DstMapIconSources {
            registered_atlases: &[],
            ..Default::default()
        },
        &textures[..1],
        &[],
        &icons,
        &[],
    )
    .unwrap();
    assert_eq!(result.textures.len(), 1);
    assert_eq!(result.icons.len(), MAX_ICONS);
    assert!(
        result
            .issues
            .iter()
            .any(|issue| issue.contains("icon item limit"))
    );

    let atlases = (0..=mod_assets::MAX_REGISTERED_ATLASES)
        .map(|i| format!("../mods/example/{i}.xml"))
        .collect::<Vec<_>>();
    let result = load_dst_map_assets(
        &root.0,
        &[],
        crate::DstMapIconSources {
            registered_atlases: &atlases,
            ..Default::default()
        },
        &textures[..1],
        &[],
        &["safe.png".into()],
        &[],
    )
    .unwrap();
    assert_eq!(result.textures.len(), 1);
    assert!(result.icons.is_empty());
    assert!(
        result
            .issues
            .iter()
            .any(|issue| issue.contains("atlas item limit"))
    );
}

#[test]
fn artwork_isolation_retains_completed_pngs_when_output_budget_rejects_later_artwork() {
    let root = TestRoot::new();
    let enabled = TestRoot::new();
    let mut texture = rgba_texture(128, 128, [0, 0, 0, 255]);
    let mut state = 0x1234_5678u32;
    for pixel in texture[18..].chunks_exact_mut(4) {
        for channel in pixel.iter_mut().take(3) {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            *channel = state as u8;
        }
    }
    let textures: Vec<_> = (0..MAX_TEXTURES)
        .map(|i| format!("levels/textures/mini_{i:03}.tex"))
        .collect();
    for path in &textures {
        root.write(&format!("data/{path}"), &texture);
    }
    let mut names: Vec<_> = (0..MAX_ICONS - 1)
        .map(|i| format!("a_large_{i:03}.png"))
        .collect();
    let mut elements = names
        .iter()
        .map(|name| format!("<Element name=\"{name}\" u1=\"0\" u2=\"1\" v1=\"0\" v2=\"1\"/>"))
        .collect::<String>();
    names.push("z_small.png".into());
    elements.push_str(
        "<Element name=\"z_small.png\" u1=\"0\" u2=\"0.0078125\" v1=\"0\" v2=\"0.0078125\"/>",
    );
    enabled.write(
        "images/large.xml",
        format!("<Atlas><Texture filename=\"large.tex\"/><Elements>{elements}</Elements></Atlas>")
            .as_bytes(),
    );
    enabled.write("images/large.tex", &texture);
    let result = load_dst_map_assets(
        &root.0,
        &[DstMapModSource {
            name: "example".into(),
            root: enabled.0.clone(),
        }],
        crate::DstMapIconSources {
            registered_atlases: &[],
            ..Default::default()
        },
        &textures,
        &[],
        &names,
        &[],
    )
    .unwrap();
    assert!(!result.textures.is_empty());
    assert!(result.icons.contains_key("z_small.png"));
    assert!(result.icons.len() < names.len());
    assert_eq!(result.icon_fallbacks.len(), result.icons.len());
    assert!(
        result
            .icon_fallbacks
            .iter()
            .all(|source| result.icons.contains_key(&source.icon))
    );
    assert!(
        result
            .issues
            .iter()
            .any(|issue| issue.contains("output byte limit"))
    );
    assert!(
        result
            .textures
            .values()
            .chain(result.icons.values())
            .map(Vec::len)
            .sum::<usize>()
            <= MAX_OUTPUT_BYTES
    );
}

#[test]
fn hostile_atlas_cannot_choose_a_different_texture_or_allocate_oversized_assets() {
    let root = TestRoot::new();
    root.write(
        "data/minimap/minimap_data2.xml",
        &icon_atlas("another.tex", "pigking.png"),
    );
    root.write(
        "data/levels/textures/mini_large.tex",
        &vec![0; MAX_TEXTURE_BYTES + 1],
    );
    let result = load_dst_map_assets(
        &root.0,
        &[],
        crate::DstMapIconSources {
            registered_atlases: &[],
            ..Default::default()
        },
        &["levels/textures/mini_large.tex".into()],
        &[],
        &["pigking.png".into()],
        &[],
    )
    .unwrap();
    assert!(result.textures.is_empty());
    assert!(result.icons.is_empty());
    assert!(
        result
            .issues
            .iter()
            .any(|issue| issue.contains("unexpected map texture"))
    );
    assert!(
        result
            .issues
            .iter()
            .any(|issue| issue.contains("size limit"))
    );
}

#[test]
fn base_material_uses_the_installed_majority_and_ignores_transparent_edge_colors() {
    let root = TestRoot::new();
    let mut texture = rgba_texture(4, 2, [20, 40, 60, 255]);
    for offset in [18, 22, 26] {
        texture[offset..offset + 4].copy_from_slice(&[250, 0, 0, 0]);
    }
    texture[30..34].copy_from_slice(&[0, 200, 0, 255]);
    root.write("data/levels/tiles/map_edge.tex", &texture);
    let result = load_dst_map_assets(
        &root.0,
        &[],
        crate::DstMapIconSources {
            registered_atlases: &[],
            ..Default::default()
        },
        &[],
        &[],
        &[],
        &[],
    )
    .unwrap();
    assert_eq!(result.terrain_base_color, Some([20, 40, 60, 255]));
    assert!(result.issues.is_empty());

    root.write(
        "data/levels/tiles/map_edge.tex",
        &rgba_texture(2, 2, [11, 22, 33, 255]),
    );
    let changed = load_dst_map_assets(
        &root.0,
        &[],
        crate::DstMapIconSources {
            registered_atlases: &[],
            ..Default::default()
        },
        &[],
        &[],
        &[],
        &[],
    )
    .unwrap();
    assert_eq!(changed.terrain_base_color, Some([11, 22, 33, 255]));
}

#[test]
fn ambiguous_or_missing_base_material_is_reported_without_inventing_a_tint() {
    let root = TestRoot::new();
    let mut texture = rgba_texture(2, 2, [20, 40, 60, 255]);
    texture[18..26].copy_from_slice(&[90, 80, 70, 255, 90, 80, 70, 255]);
    root.write("data/levels/tiles/map_edge.tex", &texture);
    let result = load_dst_map_assets(
        &root.0,
        &[],
        crate::DstMapIconSources {
            registered_atlases: &[],
            ..Default::default()
        },
        &[],
        &[],
        &[],
        &[],
    )
    .unwrap();
    assert!(result.terrain_base_color.is_none());
    assert!(result.issues[0].contains("no uniform majority"));
    fs::remove_file(root.0.join("data/levels/tiles/map_edge.tex")).unwrap();
    let result = load_dst_map_assets(
        &root.0,
        &[],
        crate::DstMapIconSources {
            registered_atlases: &[],
            ..Default::default()
        },
        &[],
        &[],
        &[],
        &[],
    )
    .unwrap();
    assert!(result.terrain_base_color.is_none());
    assert!(result.issues[0].contains("not installed"));
}

#[cfg(unix)]
#[test]
fn linked_installation_ancestors_and_asset_directories_are_rejected() {
    let root = TestRoot::new();
    let outside = TestRoot::new();
    outside.write("mini_grass_noise.tex", &rgba_texture(2, 2, [255; 4]));
    std::os::unix::fs::symlink(&outside.0, root.0.join("data/levels/textures")).unwrap();
    let result = load_dst_map_assets(
        &root.0,
        &[],
        crate::DstMapIconSources {
            registered_atlases: &[],
            ..Default::default()
        },
        &["levels/textures/mini_grass_noise.tex".into()],
        &[],
        &[],
        &[],
    )
    .unwrap();
    assert!(result.textures.is_empty());
    assert!(result.issues[0].contains("reparse"));
    std::os::unix::fs::symlink(&outside.0, root.0.join("linked")).unwrap();
    assert!(
        load_dst_map_assets(
            &root.0.join("linked"),
            &[],
            crate::DstMapIconSources {
                registered_atlases: &[],
                ..Default::default()
            },
            &[],
            &[],
            &[],
            &[]
        )
        .unwrap_err()
        .contains("reparse")
    );
}

#[test]
#[ignore = "reads only an explicitly supplied owned official DST installation"]
fn installed_official_map_artwork_decodes_without_game_tools() {
    let root = std::env::var_os("LANGAME_DST_SMOKE_PACKAGE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let games = std::env::var_os("LANGAME_DST_SMOKE_GAMES_ROOT").expect("owned games root");
            PathBuf::from(games).join("dontstarve")
        });
    let mut textures = vec![String::from("images/minimap_paper.tex")];
    for entry in fs::read_dir(root.join("data/levels/textures")).unwrap() {
        let entry = entry.unwrap();
        let path = format!("levels/textures/{}", entry.file_name().to_string_lossy());
        if valid_texture_path(&path) && entry.file_type().unwrap().is_file() {
            textures.push(path);
        }
    }
    assert!(
        textures.len() > 30,
        "expected the installed native terrain texture set"
    );
    let icons = [
        "pigking.png",
        "wormhole.png",
        "portal_dst.png",
        "resurrection_stone.png",
    ]
    .map(String::from);
    let result = load_dst_map_assets(
        &root,
        &[],
        crate::DstMapIconSources {
            registered_atlases: &[],
            ..Default::default()
        },
        &textures,
        &[],
        &icons,
        &[],
    )
    .unwrap();
    assert!(result.issues.is_empty(), "{:?}", result.issues);
    assert_eq!(result.textures.len(), textures.len());
    assert_eq!(result.icons.len(), icons.len());
    let base = result
        .terrain_base_color
        .expect("installed native base material");
    assert_eq!(base[3], 255);
    println!("NATIVE_MAP_BASE_COLOR {base:?}");
    for (name, png) in &result.textures {
        let image = image::load_from_memory(png).unwrap();
        assert!(image.width() <= 128 && image.height() <= 128);
        println!(
            "NATIVE_MAP_TEXTURE {name} {}x{} png_bytes={}",
            image.width(),
            image.height(),
            png.len()
        );
    }
    for (name, png) in &result.icons {
        let image = image::load_from_memory(png).unwrap();
        assert!(image.width() <= 64 && image.height() <= 64);
        println!(
            "NATIVE_MAP_ICON {name} {}x{} png_bytes={}",
            image.width(),
            image.height(),
            png.len()
        );
    }
}
