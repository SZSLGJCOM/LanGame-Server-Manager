use super::tests::{TestRoot, rgba_texture};
use super::*;

#[test]
fn observed_base_textures_use_their_own_pixels_and_isolate_bad_or_ambiguous_materials() {
    let root = TestRoot::new();
    let enabled = TestRoot::new();
    enabled.write("levels/custom.tex", &rgba_texture(4, 2, [255; 4]));
    // Stale atlas XML must not override the texture actually applied by MiniMap.
    enabled.write(
        "levels/custom.xml",
        b"<Atlas><Texture filename=\"stale.tex\"/></Atlas>",
    );
    enabled.write("levels/stale.tex", &rgba_texture(2, 2, [20, 40, 60, 255]));
    enabled.write("levels/broken.tex", b"broken KTEX");
    let mut ambiguous = rgba_texture(2, 2, [255; 4]);
    ambiguous[18..26].copy_from_slice(&[1, 2, 3, 255, 1, 2, 3, 255]);
    enabled.write("levels/ambiguous.tex", &ambiguous);
    enabled.write(
        "levels/transparent.tex",
        &rgba_texture(2, 2, [250, 0, 0, 0]),
    );
    root.write(
        "data/levels/textures/mini_known.tex",
        &rgba_texture(2, 2, [40; 4]),
    );
    let paths = [
        "../mods/installed/levels/custom.tex",
        "../mods/installed/levels/broken.tex",
        "../mods/installed/levels/missing.tex",
        "../mods/installed/levels/ambiguous.tex",
        "../mods/installed/levels/transparent.tex",
        "../mods/not-enabled/levels/custom.tex",
        "../mods/installed/../outside.tex",
        "levels/tiles/map_edge.tex",
    ]
    .map(String::from);
    let result = load_dst_map_assets(
        &root.0,
        &[DstMapModSource {
            name: "installed".into(),
            root: enabled.0.clone(),
        }],
        crate::DstMapIconSources {
            registered_atlases: &[],
            ..Default::default()
        },
        &["levels/textures/mini_known.tex".into()],
        &paths,
        &[],
        &[],
    )
    .unwrap();
    assert_eq!(result.terrain_base_color, Some([100, 80, 60, 255]));
    assert_eq!(
        result.terrain_base_colors,
        BTreeMap::from([
            (paths[0].clone(), [255; 4]),
            (paths[7].clone(), [100, 80, 60, 255]),
        ])
    );
    assert_eq!(result.textures.len(), 1);
    assert_eq!(result.issues.len(), 6);
    assert!(
        result
            .issues
            .iter()
            .all(|issue| issue.ends_with("using the native base."))
    );
}

#[test]
fn base_item_limit_preserves_noise_and_counts_distinct_sources_under_the_shared_budget() {
    let root = TestRoot::new();
    root.write(
        "data/levels/textures/mini_known.tex",
        &rgba_texture(2, 2, [40; 4]),
    );
    let paths: Vec<_> = (0..=MAX_TEXTURES)
        .map(|index| format!("levels/tiles/base_{index:03}.tex"))
        .collect();
    for path in &paths {
        root.write(&format!("data/{path}"), &rgba_texture(2, 2, [255; 4]));
    }
    let result = load_dst_map_assets(
        &root.0,
        &[],
        crate::DstMapIconSources {
            registered_atlases: &[],
            ..Default::default()
        },
        &["levels/textures/mini_known.tex".into()],
        &paths,
        &[],
        &[],
    )
    .unwrap();
    assert_eq!(result.terrain_base_colors.len(), MAX_TEXTURES);
    assert_eq!(result.textures.len(), 1);
    assert!(
        result
            .issues
            .iter()
            .any(|issue| issue.contains("base texture item limit"))
    );
    let duplicates = vec![paths[0].clone(); MAX_TEXTURES * 2];
    let result = load_dst_map_assets(
        &root.0,
        &[],
        crate::DstMapIconSources {
            registered_atlases: &[],
            ..Default::default()
        },
        &[],
        &duplicates,
        &[],
        &[],
    )
    .unwrap();
    assert_eq!(result.terrain_base_colors.len(), 1);
    assert!(result.issues.is_empty());

    let mut budget = ReadBudget {
        decoded_pixels: ReadBudget::MAX_DECODED_PIXELS,
        ..Default::default()
    };
    let install_root = checked_root(&root.0).unwrap();
    assert!(
        read_resource(&install_root, &paths[0], MAX_ATLAS_BYTES, &mut budget)
            .unwrap()
            .is_some()
    );
    assert!(
        budget
            .decode(&rgba_texture(2, 2, [255; 4]))
            .unwrap_err()
            .contains("cumulative decoded pixel limit")
    );
}

#[test]
#[ignore = "reads an explicitly supplied owned Mod base texture"]
fn installed_observed_base_uses_texture_pixels_despite_stale_xml() {
    let source =
        PathBuf::from(std::env::var_os("LANGAME_DST_BASE_SMOKE_TEXTURE").expect("owned texture"));
    let bytes = fs::read(source).unwrap();
    assert!(bytes.len() <= 16 * 1024 * 1024 && bytes.starts_with(b"KTEX"));
    let root = TestRoot::new();
    let enabled = TestRoot::new();
    enabled.write("levels/observed.tex", &bytes);
    enabled.write(
        "levels/observed.xml",
        b"<Atlas><Texture filename=\"stale.tex\"/></Atlas>",
    );
    enabled.write("levels/stale.tex", &rgba_texture(2, 2, [20, 40, 60, 255]));
    let path = String::from("../mods/reference/levels/observed.tex");
    let result = load_dst_map_assets(
        &root.0,
        &[DstMapModSource {
            name: "reference".into(),
            root: enabled.0.clone(),
        }],
        crate::DstMapIconSources {
            registered_atlases: &[],
            ..Default::default()
        },
        &[],
        std::slice::from_ref(&path),
        &[],
        &[],
    )
    .unwrap();
    assert_eq!(result.terrain_base_colors[&path], [255; 4]);
    assert!(result.issues.is_empty(), "{:?}", result.issues);
    println!(
        "NATIVE_OBSERVED_BASE rgba={:?} bytes={} stale_xml_ignored=true",
        result.terrain_base_colors[&path],
        bytes.len()
    );
}
