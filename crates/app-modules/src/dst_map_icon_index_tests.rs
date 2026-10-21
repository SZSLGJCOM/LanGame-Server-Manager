use super::super::{
    DstMapModSource, load_dst_map_assets,
    tests::{TestRoot, icon_atlas, rgba_texture},
};
use super::*;

fn source(root: &TestRoot) -> DstMapModSource {
    DstMapModSource {
        name: "enabled".into(),
        root: root.0.clone(),
    }
}

fn add_candidate(root: &TestRoot) -> Vec<u8> {
    let xml = icon_atlas("target.tex", "target.tex");
    root.write("target.xml", &xml);
    root.write("target.tex", &rgba_texture(2, 2, [20, 40, 60, 255]));
    xml
}

fn add_small_atlases(root: &TestRoot) {
    for index in 0..513 {
        root.write(
            &format!("small-{index}.xml"),
            &icon_atlas("unused.tex", "unrelated.tex"),
        );
    }
}

fn fill_metadata_budget(root: &TestRoot, candidate_bytes: usize) {
    assert_eq!(MAX_INDEX_XML_BYTES, 8 * 1024 * 1024);
    let mut remaining = MAX_INDEX_XML_BYTES - candidate_bytes;
    for index in 0..8 {
        let length = remaining.min(MAX_XML_BYTES);
        let mut xml = icon_atlas("unused.tex", "unrelated.tex");
        assert!(xml.len() <= length);
        xml.resize(length, b' ');
        root.write(&format!("payloads/{index}.xml"), &xml);
        remaining -= length;
    }
    assert_eq!(remaining, 0);
}

#[test]
fn more_than_512_small_atlases_complete_a_unique_exact_icon() {
    let native = TestRoot::new();
    let enabled = TestRoot::new();
    add_candidate(&enabled);
    add_small_atlases(&enabled);
    let output = load_dst_map_assets(
        &native.0,
        &[source(&enabled)],
        DstMapIconSources::default(),
        &[],
        &[],
        &["target.tex".into()],
        &[],
    )
    .unwrap();
    assert!(output.issues.is_empty(), "{:?}", output.issues);
    assert_eq!(output.icons.len(), 1);
    let image = image::load_from_memory(&output.icons["target.tex"])
        .unwrap()
        .into_rgba8();
    assert_eq!(image.dimensions(), (2, 2));
    assert_eq!(image.get_pixel(0, 0).0, [20, 40, 60, 255]);
    assert_eq!(
        output.icon_fallbacks,
        vec![DstMapIconFallback {
            icon: "target.tex".into(),
            element: "target.tex".into(),
            atlas: "../mods/enabled/target.xml".into(),
            kind: DstMapIconFallbackKind::IndexedExact,
        }]
    );
}

#[test]
fn duplicate_after_more_than_512_atlases_still_rejects_a_found_candidate() {
    let native = TestRoot::new();
    let enabled = TestRoot::new();
    add_candidate(&enabled);
    add_small_atlases(&enabled);
    // Breadth-first traversal must inspect every root XML before this later
    // directory, irrespective of the filesystem's root-entry order.
    enabled.write(
        "late/duplicate.xml",
        &icon_atlas("duplicate.tex", "TARGET.TEX"),
    );
    enabled.write("late/duplicate.tex", &rgba_texture(2, 2, [255; 4]));
    let output = load_dst_map_assets(
        &native.0,
        &[source(&enabled)],
        DstMapIconSources::default(),
        &[],
        &[],
        &["target.tex".into()],
        &[],
    )
    .unwrap();
    assert!(output.icons.is_empty());
    assert!(output.icon_fallbacks.is_empty());
    assert!(
        output
            .issues
            .iter()
            .any(|issue| { issue == "DST map icon completion is ambiguous: target.tex" }),
        "{:?}",
        output.issues
    );
    assert!(
        !output
            .issues
            .iter()
            .any(|issue| issue.contains("index is incomplete"))
    );
}

#[test]
fn exact_cumulative_xml_byte_boundary_is_accepted_without_decoding_unwanted_atlases() {
    let enabled = TestRoot::new();
    let xml = add_candidate(&enabled);
    fill_metadata_budget(&enabled, xml.len());
    let mods = ModAssets::new(&[source(&enabled)]).unwrap();
    let mut budget = ReadBudget::default();
    let catalog = indexed_catalog(
        &mods,
        &[],
        &BTreeSet::from(["target.tex".into()]),
        &mut budget,
    )
    .unwrap();
    assert!(matches!(
        catalog.0.get("target.tex"),
        Some(Choice::Unique(_))
    ));
    assert_eq!(budget.source_bytes, 8 * 1024 * 1024);
    assert_eq!(budget.decoded_pixels, 0);
    assert_eq!(budget.crop_source_pixels, 0);
}

#[test]
fn cumulative_xml_byte_allowance_is_not_reset_for_a_later_authorized_mod() {
    let first = TestRoot::new();
    let xml = add_candidate(&first);
    fill_metadata_budget(&first, xml.len());
    let later = TestRoot::new();
    later.write("one-byte.xml", b" ");
    let mods = ModAssets::new(&[
        source(&first),
        DstMapModSource {
            name: "z_later".into(),
            root: later.0.clone(),
        },
    ])
    .unwrap();
    let mut budget = ReadBudget::default();
    let error = indexed_catalog(&mods, &[], &BTreeSet::new(), &mut budget)
        .err()
        .unwrap();
    assert_eq!(error, "atlas index exceeds its cumulative XML byte limit");
    assert_eq!(budget.source_bytes, 8 * 1024 * 1024);
}

#[test]
fn byte_overflow_after_a_found_candidate_omits_only_completion_artwork() {
    let native = TestRoot::new();
    native.write(
        "data/minimap/minimap_data2.xml",
        &icon_atlas("minimap_atlas2.tex", "native.png"),
    );
    native.write(
        "data/minimap/minimap_atlas2.tex",
        &rgba_texture(2, 2, [255; 4]),
    );
    let enabled = TestRoot::new();
    let xml = add_candidate(&enabled);
    fill_metadata_budget(&enabled, xml.len());
    // The grandchild cannot be visited until all exact-budget parent XMLs
    // and the already-found root candidate have been scanned.
    enabled.write("payloads/late/overflow.xml", b" ");
    let output = load_dst_map_assets(
        &native.0,
        &[source(&enabled)],
        DstMapIconSources::default(),
        &[],
        &[],
        &["target.tex".into(), "native.png".into()],
        &[],
    )
    .unwrap();
    assert!(!output.icons.contains_key("target.tex"));
    assert!(output.icon_fallbacks.is_empty());
    assert!(output.icons.contains_key("native.png"));
    assert_eq!(output.terrain_base_color, Some([100, 80, 60, 255]));
    assert!(output.issues.iter().any(|issue| issue ==
        "Mod map icon completion index is incomplete: atlas index exceeds its cumulative XML byte limit"
    ), "{:?}", output.issues);
}

#[test]
fn xml_index_keeps_the_independent_per_file_and_shared_source_limits() {
    let enabled = TestRoot::new();
    add_candidate(&enabled);
    let mods = ModAssets::new(&[source(&enabled)]).unwrap();
    let mut budget = ReadBudget {
        source_bytes: ReadBudget::MAX_SOURCE_BYTES - 1,
        ..Default::default()
    };
    let error = indexed_catalog(&mods, &[], &BTreeSet::new(), &mut budget)
        .err()
        .unwrap();
    assert!(error.contains("cumulative source byte limit"), "{error}");
    assert_eq!(budget.source_bytes, ReadBudget::MAX_SOURCE_BYTES - 1);
    enabled.write("oversized.xml", &vec![b' '; MAX_XML_BYTES + 1]);
    let error = indexed_catalog(&mods, &[], &BTreeSet::new(), &mut ReadBudget::default())
        .err()
        .unwrap();
    assert!(error.contains("size limit"), "{error}");
    assert!(!error.contains("cumulative XML byte limit"));
}

#[test]
#[ignore = "reads an explicitly supplied original full Workshop resource tree"]
fn installed_large_xml_index_completes_the_unique_original_material() {
    let original = std::path::PathBuf::from(
        std::env::var_os("LANGAME_DST_INDEX_SMOKE_MOD_ROOT").expect("original Mod root"),
    );
    // Independent expectations from the pinned original 2979177306 package;
    // these names and sizes are not part of production resource selection.
    let relative = "images/screen/lg_introduction_book/terrain/hangzhou/lg_costhouse.xml";
    let mut pending = vec![original.clone()];
    let (mut entries, mut xml_files, mut xml_bytes) = (0, 0, 0u64);
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).unwrap() {
            entries += 1;
            assert!(entries <= MAX_INDEX_ENTRIES);
            let entry = entry.unwrap();
            let metadata = fs::symlink_metadata(entry.path()).unwrap();
            assert!(!resources::is_reparse(&metadata));
            if metadata.is_dir() {
                pending.push(entry.path());
            } else if entry.file_name().to_str().unwrap().ends_with(".xml") {
                assert!(metadata.is_file());
                assert!(metadata.len() <= MAX_XML_BYTES as u64);
                xml_files += 1;
                xml_bytes += metadata.len();
            }
        }
    }
    assert_eq!((xml_files, xml_bytes), (1128, 208940));
    assert_eq!(fs::metadata(original.join(relative)).unwrap().len(), 172);
    assert_eq!(
        fs::metadata(original.join(relative).with_extension("tex"))
            .unwrap()
            .len(),
        11048
    );
    let native = TestRoot::new();
    let root = super::super::checked_root(&native.0).unwrap();
    let mods = ModAssets::new(&[DstMapModSource {
        name: "original_index".into(),
        root: original,
    }])
    .unwrap();
    let mut budget = ReadBudget::default();
    let output = load(
        &root,
        &mods,
        DstMapIconSources::default(),
        &BTreeSet::from(["lg_costhouse.tex"]),
        &mut budget,
    );
    assert!(output.issues.is_empty(), "{:?}", output.issues);
    assert_eq!(output.icons.len(), 1);
    assert_eq!(
        output.provenance,
        vec![DstMapIconFallback {
            icon: "lg_costhouse.tex".into(),
            element: "lg_costhouse.tex".into(),
            atlas: format!("../mods/original_index/{relative}"),
            kind: DstMapIconFallbackKind::IndexedExact,
        }]
    );
    assert_eq!(budget.source_bytes as u64, xml_bytes + 11048);
    let png = &output.icons["lg_costhouse.tex"];
    let pixels = image::load_from_memory(png).unwrap().into_rgba8();
    assert!(pixels.width() > 0 && pixels.width() <= 64);
    assert!(pixels.height() > 0 && pixels.height() <= 64);
    assert!(pixels.pixels().any(|pixel| pixel[3] > 0));
    println!(
        "actual original atlas index: xml_files={xml_files} xml_bytes={xml_bytes} texture_bytes=11048 png_bytes={} dimensions={}x{} kind=indexed_exact",
        png.len(),
        pixels.width(),
        pixels.height()
    );
}
