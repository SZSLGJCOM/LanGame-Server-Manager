use super::super::tests::{TestRoot, rgba_texture};
use super::*;

fn masks() -> Vec<u8> {
    // Independent set predicate: every present cardinal requires its two
    // corner coverages. No native element number or production helper is used.
    (0..=u8::MAX)
        .filter(|m| {
            (m & 2 == 0 || m & 5 == 5)
                && (m & 16 == 0 || m & 132 == 132)
                && (m & 64 == 0 || m & 160 == 160)
                && (m & 8 == 0 || m & 33 == 33)
        })
        .collect()
}

fn fixture(reverse: bool) -> (RgbaImage, Vec<u8>) {
    let mut regions: Vec<_> = std::iter::once(None)
        .chain(masks().into_iter().map(Some))
        .collect();
    assert_eq!(regions.len(), 48);
    if reverse {
        regions.reverse();
    }
    let mut pixels = RgbaImage::new(512, 384);
    let mut xml =
        String::from("<Atlas><Texture filename=\"data/levels/tiles/stale.tex\"/><Elements>");
    for (i, mask) in regions.iter().enumerate() {
        let (left, top) = ((i as u32 % 8) * 64, (i as u32 / 8) * 64);
        for y in 0..64 {
            for x in 0..64 {
                let (gx, gy) = (x * 3 / 64, y * 3 / 64);
                let bit = match (gx, gy) {
                    (0, 0) => Some(0),
                    (1, 0) => Some(1),
                    (2, 0) => Some(2),
                    (0, 1) => Some(3),
                    (2, 1) => Some(4),
                    (0, 2) => Some(5),
                    (1, 2) => Some(6),
                    (2, 2) => Some(7),
                    _ => None,
                };
                let on = mask.is_none()
                    || bit.is_some_and(|bit| mask.is_some_and(|mask| mask & (1 << bit) != 0));
                pixels.put_pixel(
                    left + x,
                    top + y,
                    image::Rgba(if on { [200, 100, 50, 255] } else { [0; 4] }),
                );
            }
        }
        xml.push_str(&format!(
            "<Element name=\"artist_region_{i}\" u1=\"{}\" u2=\"{}\" v1=\"{}\" v2=\"{}\"/>",
            f64::from(left) / 512.0,
            f64::from(left + 64) / 512.0,
            1.0 - f64::from(top + 64) / 384.0,
            1.0 - f64::from(top) / 384.0
        ));
    }
    xml.push_str("</Elements></Atlas>");
    (pixels, xml.into_bytes())
}

fn ktex(pixels: &RgbaImage) -> Vec<u8> {
    let mut bytes = rgba_texture(pixels.width() as u16, pixels.height() as u16, [0; 4]);
    for y in 0..pixels.height() {
        for x in 0..pixels.width() {
            let offset = 18 + (((pixels.height() - 1 - y) * pixels.width() + x) * 4) as usize;
            bytes[offset..offset + 4].copy_from_slice(&pixels.get_pixel(x, y).0);
        }
    }
    bytes
}

#[test]
fn content_classifier_is_independent_of_names_uv_layout_and_center_fringe() {
    for reverse in [false, true] {
        let (mut pixels, xml) = fixture(reverse);
        // Several edge artworks may extend into their center; only a wholly
        // opaque crop represents the opaque interior, never a center probe.
        let index = if reverse { 0 } else { 47 };
        pixels.put_pixel(
            (index % 8) * 64 + 32,
            (index / 8) * 64 + 32,
            image::Rgba([200, 100, 50, 255]),
        );
        let parsed = atlas::parse_terrain_atlas(&xml).unwrap();
        let (opaque, samples) = classify(&pixels, &parsed, &mut ReadBudget::default()).unwrap();
        assert_eq!(samples.len(), 47);
        assert_eq!(samples.keys().copied().collect::<Vec<_>>(), masks());
        assert_ne!(opaque, samples[&255]);
        assert!(samples.contains_key(&0));
        let expected = if reverse {
            [0.875, 320.0 / 384.0, 0.125, 64.0 / 384.0]
        } else {
            [0.0, 0.0, 0.125, 64.0 / 384.0]
        };
        assert!(
            opaque
                .into_iter()
                .zip(expected)
                .all(|(actual, expected)| (actual - expected).abs() < 1e-9)
        );
    }
}

#[test]
fn ambiguous_incomplete_and_duplicate_shapes_are_rejected_under_sampling_budget() {
    let (pixels, xml) = fixture(false);
    let parsed = atlas::parse_terrain_atlas(&xml).unwrap();
    let mut bad = pixels.clone();
    // Second region is empty. Replace it with an opaque duplicate.
    for y in 0..64 {
        for x in 64..128 {
            bad.put_pixel(x, y, image::Rgba([200, 100, 50, 255]));
        }
    }
    assert!(
        classify(&bad, &parsed, &mut ReadBudget::default())
            .unwrap_err()
            .contains("opaque")
    );
    let mut bad = pixels.clone();
    for y in 0..8 {
        for x in 64..72 {
            bad.put_pixel(x, y, image::Rgba([0, 0, 0, 127]));
        }
    }
    assert!(
        classify(&bad, &parsed, &mut ReadBudget::default())
            .unwrap_err()
            .contains("ambiguous")
    );
    let mut budget = ReadBudget {
        crop_source_pixels: ReadBudget::MAX_CROP_SOURCE_PIXELS - 1,
        ..Default::default()
    };
    assert!(
        classify(&pixels, &parsed, &mut budget)
            .unwrap_err()
            .contains("sampling pixel limit")
    );
    let xml = String::from_utf8(xml).unwrap();
    let short = xml.replace(
        "<Element name=\"artist_region_47\"",
        "<Element name=\"artist_region_46\"",
    );
    assert!(atlas::parse_terrain_atlas(short.as_bytes()).is_err());
    // Keeping a complete set of 48 valid regions must not hide a 49th
    // ambiguous declaration, including the engine's ASCII-case aliases.
    for name in ["artist_region_0", "ARTIST_REGION_0"] {
        let duplicate = xml.replace(
            "</Elements>",
            &format!(
                "<Element name=\"{name}\" u1=\"0\" u2=\"0.125\" v1=\"0\" v2=\"0.16666666666666666\"/></Elements>"
            ),
        );
        assert!(atlas::parse_terrain_atlas(duplicate.as_bytes()).is_err());
    }
}

#[test]
fn explicit_enabled_texture_wins_over_stale_xml_and_bad_tuple_is_local() {
    let root = TestRoot::new();
    let installed = TestRoot::new();
    let (pixels, xml) = fixture(true);
    installed.write("layers/edges.xml", &xml);
    installed.write("layers/explicit.tex", &ktex(&pixels));
    installed.write("layers/bad.tex", b"not KTEX");
    root.write(
        "data/levels/textures/mini_known.tex",
        &rgba_texture(2, 2, [40; 4]),
    );
    let requests = [
        (
            "../mods/dynamic_pack/layers/edges.xml".into(),
            "../mods/dynamic_pack/layers/bad.tex".into(),
        ),
        (
            "../mods/dynamic_pack/layers/edges.xml".into(),
            "../mods/dynamic_pack/layers/explicit.tex".into(),
        ),
    ];
    let assets = super::super::load_dst_map_assets(
        &root.0,
        &[super::super::DstMapModSource {
            name: "dynamic_pack".into(),
            root: installed.0.clone(),
        }],
        crate::DstMapIconSources {
            registered_atlases: &[],
            ..Default::default()
        },
        &["levels/textures/mini_known.tex".into()],
        &[],
        &[],
        &requests,
    )
    .unwrap();
    assert_eq!(assets.terrain_edges.len(), 1);
    assert_eq!(assets.terrain_edges[0].texture, requests[1].1);
    assert_eq!(assets.terrain_edges[0].samples.len(), 47);
    assert_eq!(
        image::load_from_memory(&assets.terrain_edges[0].image)
            .unwrap()
            .to_rgba8(),
        pixels
    );
    assert_eq!(assets.textures.len(), 1);
    assert_eq!(assets.terrain_base_color, Some([100, 80, 60, 255]));
    assert_eq!(assets.issues.len(), 1);
    assert!(assets.issues[0].contains("bad.tex"));
}

#[test]
fn edge_request_limit_deduplicates_tuples_and_preserves_other_art() {
    let root = TestRoot::new();
    let requests: Vec<_> = (0..=super::super::MAX_TEXTURES)
        .map(|i| {
            (
                format!("levels/tiles/missing{i}.xml"),
                "levels/tiles/map_edge.tex".into(),
            )
        })
        .collect();
    let assets = super::super::load_dst_map_assets(
        &root.0,
        &[],
        crate::DstMapIconSources {
            registered_atlases: &[],
            ..Default::default()
        },
        &[],
        &[],
        &[],
        &requests,
    )
    .unwrap();
    assert!(assets.terrain_edges.is_empty());
    assert_eq!(assets.terrain_base_color, Some([100, 80, 60, 255]));
    assert!(
        assets
            .issues
            .iter()
            .any(|issue| issue.contains("edge item limit (128)"))
    );
    assert_eq!(assets.issues.len(), 129);
}

#[test]
#[ignore = "requires an explicitly provided original installed material"]
fn installed_custom_material_uses_real_explicit_texture_and_complete_alpha_shapes() {
    let path = std::path::PathBuf::from(
        std::env::var_os("LANGAME_DST_EDGE_SMOKE_TEXTURE")
            .expect("LANGAME_DST_EDGE_SMOKE_TEXTURE must identify an authorized original KTEX"),
    );
    let parent = path.parent().unwrap();
    let name = path.file_name().unwrap().to_str().unwrap();
    let xml = path.with_extension("xml");
    assert!(xml.is_file());
    let atlas_name = xml.file_name().unwrap().to_str().unwrap();
    let root = TestRoot::new();
    let requests = [(
        format!("../mods/native_probe/{atlas_name}"),
        format!("../mods/native_probe/{name}"),
    )];
    let output = super::super::load_dst_map_assets(
        &root.0,
        &[super::super::DstMapModSource {
            name: "native_probe".into(),
            root: parent.into(),
        }],
        crate::DstMapIconSources {
            registered_atlases: &[],
            ..Default::default()
        },
        &[],
        &[],
        &[],
        &requests,
    )
    .unwrap();
    assert!(output.issues.is_empty(), "{:?}", output.issues);
    assert_eq!(output.terrain_edges.len(), 1);
    let edge = &output.terrain_edges[0];
    assert_eq!(edge.samples.len(), 47);
    assert_eq!(edge.texture, requests[0].1);
    let decoded = image::load_from_memory(&edge.image).unwrap();
    assert!(decoded.width() >= 32 && decoded.height() >= 32);
    println!(
        "actual original material: source_bytes={} png_bytes={} dimensions={}x{} shapes=47+opaque",
        std::fs::metadata(&path).unwrap().len(),
        edge.image.len(),
        decoded.width(),
        decoded.height()
    );
}
