use super::*;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

pub(super) struct TestRoot(pub PathBuf);

impl TestRoot {
    pub fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "langame-dst-icons-{}-{unique}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    pub fn write(&self, relative: &str, bytes: &[u8]) {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        assert!(self.0.starts_with(std::env::temp_dir()));
        fs::remove_dir_all(&self.0).expect("remove isolated DST icon fixture");
    }
}

pub(super) fn fixture_xml(atlas: &str, element: &str) -> Vec<u8> {
    format!("<Atlas><Texture filename=\"{atlas}.tex\"/><Elements><Element name=\"{element}\" u1=\"0\" u2=\"1\" v1=\"0\" v2=\"1\"/></Elements></Atlas>").into_bytes()
}

#[test]
fn installed_icons_use_native_keys_and_preserve_shard_fields() {
    let root = TestRoot::new();
    let settings_xml = fixture_xml("worldsettings_customization", "rain.tex");
    let worldgen_xml = fixture_xml("worldgen_customization", "world_map.tex");
    let texture = texture::tests::texture_fixture(4, 2, 2, &[255; 16]);
    root.write("data/images/worldsettings_customization.xml", &settings_xml);
    root.write("data/images/worldsettings_customization.tex", &texture);
    root.write("data/images/worldgen_customization.xml", &worldgen_xml);
    root.write("data/images/worldgen_customization.tex", &texture);
    let schema = serde_json::json!({ "properties": {
        "master_weather": {"x-lsgm-section":"mastersettings", "x-lsgm-source-key":"overrides.weather"},
        "caves_weather": {"x-lsgm-section":"cavessettings", "x-lsgm-source-key":"overrides.weather"},
        "master_task_set": {"x-lsgm-section":"mastergen", "x-lsgm-source-key":"overrides.task_set"},
        "master_settings_preset": {"x-lsgm-section":"mastersettings", "x-lsgm-source-key":"settings_preset"},
        "outside_world": {"x-lsgm-section":"mods", "x-lsgm-source-key":"overrides.weather"}
    }}).to_string();
    let icons = load_dst_configuration_icons(&root.0, &root.0, &schema)
        .unwrap()
        .icons;
    assert_eq!(
        icons.keys().map(String::as_str).collect::<Vec<_>>(),
        vec!["caves_weather", "master_task_set", "master_weather"]
    );
    assert_eq!(icons["master_weather"], icons["caves_weather"]);
    for bytes in icons.values() {
        let image = image::load_from_memory(bytes).unwrap().into_rgba8();
        assert_eq!(image.dimensions(), (2, 2));
        assert!(image.pixels().all(|pixel| pixel.0 == [255; 4]));
    }
}

#[test]
fn current_schema_has_a_source_mapping_for_every_native_world_option() {
    let schema = include_str!("../../../../modules/dontstarve/schema.json");
    let icons = requested_icons(schema).unwrap();
    assert_eq!(icons.len(), 278);
    assert_eq!(icons["master_task_set"].atlas, "worldgen_customization");
    assert_eq!(icons["master_task_set"].element, "world_map.tex");
    assert!(!icons.contains_key("master_settings_preset"));
    for key in ["bat_boss", "rocky_boss"] {
        let icon = &icons[&format!("caves_{key}")];
        assert_eq!(icon.atlas, "worldsettings_customization");
        assert_eq!(icon.element, format!("{key}.tex"));
        assert!(!icons.contains_key(&format!("master_{key}")));
    }
}

#[test]
#[ignore = "requires an owned 759671 or newer official package in LANGAME_DST_SMOKE_PACKAGE"]
fn installed_cave_boss_icons_load_with_the_complete_current_schema() {
    let package = PathBuf::from(
        std::env::var_os("LANGAME_DST_SMOKE_PACKAGE").expect("owned official DST package"),
    );
    let cache = TestRoot::new();
    let schema = include_str!("../../../../modules/dontstarve/schema.json");
    let result = load_dst_configuration_icons(&package, &cache.0, schema).unwrap();
    assert!(result.warnings.is_empty(), "{:?}", result.warnings);
    for field in ["caves_bat_boss", "caves_rocky_boss", "master_weather"] {
        let png = result.icons.get(field).expect("native configuration icon");
        let image = image::load_from_memory(png).unwrap().into_rgba8();
        assert!(image.width() > 0 && image.height() > 0);
        assert!(image.pixels().any(|pixel| pixel.0[3] != 0));
    }
}

#[test]
fn world_presets_do_not_consume_the_native_icon_field_budget() {
    let mut properties: serde_json::Map<_, _> = (0..MAX_ICON_FIELDS)
        .map(|index| {
            (
                format!("field_{index}"),
                serde_json::json!({"x-lsgm-section":"mastersettings", "x-lsgm-source-key":"overrides.weather"}),
            )
        })
        .collect();
    for (field, section, source) in [
        (
            "master_settings_preset",
            "mastersettings",
            "settings_preset",
        ),
        ("master_worldgen_preset", "mastergen", "worldgen_preset"),
        ("caves_settings_preset", "cavessettings", "settings_preset"),
        ("caves_worldgen_preset", "cavesgen", "worldgen_preset"),
        ("outside_world", "mods", "overrides.weather"),
    ] {
        properties.insert(
            field.into(),
            serde_json::json!({"x-lsgm-section":section, "x-lsgm-source-key":source}),
        );
    }
    let schema = serde_json::json!({"properties":&properties}).to_string();
    assert_eq!(requested_icons(&schema).unwrap().len(), MAX_ICON_FIELDS);
    for source in ["overrides.weather", "overrides.unknown_native_option"] {
        properties.insert(
            "excess_native".into(),
            serde_json::json!({"x-lsgm-section":"cavessettings", "x-lsgm-source-key":source}),
        );
        let schema = serde_json::json!({"properties":&properties}).to_string();
        assert!(
            requested_icons(&schema)
                .err()
                .is_some_and(|error| error.contains("count or name limit"))
        );
    }
}

#[test]
fn native_icon_schema_and_field_names_retain_their_byte_limits() {
    for length in [128, 129] {
        let properties = serde_json::Map::from_iter([(
            "x".repeat(length),
            serde_json::json!({"x-lsgm-section":"mastergen", "x-lsgm-source-key":"overrides.task_set"}),
        )]);
        let schema = serde_json::json!({"properties":properties}).to_string();
        assert_eq!(requested_icons(&schema).is_ok(), length == 128);
    }
    assert!(
        requested_icons(&" ".repeat(MAX_SCHEMA_BYTES + 1))
            .err()
            .is_some_and(|error| error.contains("schema exceeds its byte limit"))
    );
}

#[test]
fn missing_assets_are_optional_but_damaged_assets_report_an_error() {
    let root = TestRoot::new();
    let schema = include_str!("../../../../modules/dontstarve/schema.json");
    assert!(
        load_dst_configuration_icons(&root.0.join("not-installed"), &root.0, schema)
            .unwrap()
            .icons
            .is_empty()
    );
    assert!(
        load_dst_configuration_icons(&root.0, &root.0, schema)
            .unwrap()
            .icons
            .is_empty()
    );
    root.write("data/images/worldgen_customization.xml", b"not XML");
    assert!(
        load_dst_configuration_icons(&root.0, &root.0, schema)
            .unwrap_err()
            .contains("atlas")
    );
}

#[test]
fn repeated_schema_fields_cannot_amplify_png_output_past_the_budget() {
    let root = TestRoot::new();
    root.write(
        "data/images/worldsettings_customization.xml",
        &fixture_xml("worldsettings_customization", "rain.tex"),
    );
    let mut pixels = Vec::new();
    let mut noise = 0x1234_5678u32;
    for _ in 0..72 * 72 {
        noise ^= noise << 13;
        noise ^= noise >> 17;
        noise ^= noise << 5;
        pixels.extend_from_slice(&[noise as u8, (noise >> 8) as u8, (noise >> 16) as u8, 255]);
    }
    root.write(
        "data/images/worldsettings_customization.tex",
        &texture::tests::texture_fixture(4, 72, 72, &pixels),
    );
    let properties: serde_json::Map<_, _> = (0..MAX_ICON_FIELDS).map(|index| (format!("field_{index}"), serde_json::json!({"x-lsgm-section":"mastersettings", "x-lsgm-source-key":"overrides.weather"}))).collect();
    let schema = serde_json::json!({"properties":properties}).to_string();
    assert!(
        load_dst_configuration_icons(&root.0, &root.0, &schema)
            .unwrap_err()
            .contains("total PNG byte limit")
    );
}
