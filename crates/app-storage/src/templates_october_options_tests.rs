use super::*;
use crate::settings_validation::{SettingsValidationPhase, collect_settings_schema_diagnostics};

const RUST_OPTIONS: [(&str, &str); 4] = [
    ("vehicle_pad_repairs_required", "vehicle.padrepairsrequired"),
    ("powergrid_fuse_decay_low_pop", "powergrid.fusedecaylowpop"),
    (
        "powergrid_fuse_decay_high_pop",
        "powergrid.fusedecayhighpop",
    ),
    (
        "powergrid_fuse_decay_low_pop_scale",
        "powergrid.fusedecaylowpopscale",
    ),
];

fn render_rust_configuration(settings: &Map<String, Value>) -> String {
    let schema_defaults = collect_schema_defaults_from_schema_json(
        Some(include_str!("../../../modules/rust/schema.json")),
        SchemaDefaultContext::default(),
    )
    .unwrap();
    let root = Path::new("C:/LanGame/configuration-test");
    let ports = [PortBinding {
        name: "rcon".into(),
        protocol: "tcp".into(),
        port: 28016,
    }];
    render_template_text(
        include_str!("../../../modules/rust/templates/server.cfg.hbs"),
        &TemplateRenderContext {
            instance_root: root,
            config_dir: root,
            install_root: root,
            data_dir: root,
            logs_dir: root,
            saves_dir: root,
            instance_id: "rust-options",
            instance_name: "Rust options",
            module_id: "rust",
            bind_ip: "127.0.0.1",
            autostart: false,
            schema_defaults: &schema_defaults,
            settings,
            ports: &ports,
        },
    )
    .unwrap()
}

fn check_field(schema_text: &str, key: &str, value: Value) -> Vec<String> {
    let schema: Value = serde_json::from_str(schema_text).unwrap();
    let properties = Map::from_iter([(key.to_owned(), schema["properties"][key].clone())]);
    let field_schema = json!({"type":"object","properties":properties});
    let settings = Map::from_iter([(key.to_owned(), value)]);
    collect_settings_schema_diagnostics(&field_schema, &settings, SettingsValidationPhase::Complete)
        .into_iter()
        .map(|diagnostic| diagnostic.code)
        .collect()
}

#[test]
fn rust_livestock_unset_options_preserve_existing_raw_configuration() {
    let raw = "vehicle.padrepairsrequired false\npowergrid.fusedecaylowpop 7\npowergrid.fusedecayhighpop 80\npowergrid.fusedecaylowpopscale 0.5";
    let settings = json!({"server_cfg_extra":raw}).as_object().unwrap().clone();
    let rendered = render_rust_configuration(&settings);
    assert!(rendered.contains(raw));
    for (_, convar) in RUST_OPTIONS {
        assert_eq!(
            rendered
                .lines()
                .filter(|line| line.split_whitespace().next() == Some(convar))
                .count(),
            1,
            "an unset override must not add a second directive for {convar}"
        );
    }
    let empty = render_rust_configuration(&Map::new());
    for (_, convar) in RUST_OPTIONS {
        assert!(
            !empty
                .lines()
                .any(|line| line.split_whitespace().next() == Some(convar))
        );
    }
}

#[test]
fn rust_livestock_overrides_render_false_zero_and_fraction_without_implicit_defaults() {
    let settings = json!({
        "vehicle_pad_repairs_required":false,
        "powergrid_fuse_decay_low_pop":0,
        "powergrid_fuse_decay_high_pop":150,
        "powergrid_fuse_decay_low_pop_scale":0.125
    })
    .as_object()
    .unwrap()
    .clone();
    let rendered = render_rust_configuration(&settings);
    for line in [
        "vehicle.padrepairsrequired false",
        "powergrid.fusedecaylowpop 0",
        "powergrid.fusedecayhighpop 150",
        "powergrid.fusedecaylowpopscale 0.125",
    ] {
        assert!(
            rendered.lines().any(|candidate| candidate == line),
            "{line}"
        );
    }
    let schema: Value =
        serde_json::from_str(include_str!("../../../modules/rust/schema.json")).unwrap();
    for (key, _) in RUST_OPTIONS {
        assert!(schema["properties"][key].get("default").is_none());
    }
}

#[test]
fn rust_livestock_types_and_native_numeric_representation_are_enforced() {
    let schema = include_str!("../../../modules/rust/schema.json");
    assert!(check_field(schema, "vehicle_pad_repairs_required", json!(false)).is_empty());
    assert_eq!(
        check_field(schema, "vehicle_pad_repairs_required", json!("false")),
        ["type"]
    );
    for key in [
        "powergrid_fuse_decay_low_pop",
        "powergrid_fuse_decay_high_pop",
    ] {
        for value in [json!(-2147483648_i64), json!(0), json!(2147483647_i64)] {
            assert!(check_field(schema, key, value).is_empty());
        }
        for value in [
            json!(-2147483649_i64),
            json!(2147483648_i64),
            json!(1.5),
            json!("10"),
        ] {
            assert!(!check_field(schema, key, value).is_empty());
        }
    }
    assert!(check_field(schema, "powergrid_fuse_decay_low_pop_scale", json!(0.125)).is_empty());
    assert!(!check_field(schema, "powergrid_fuse_decay_low_pop_scale", json!(1e39)).is_empty());
}

#[test]
fn moria_worker_threads_accept_only_native_one_to_four_values() {
    let schema = include_str!("../../../modules/returntomoria/schema.json");
    for value in 1..=4 {
        assert!(check_field(schema, "server_worker_threads", json!(value)).is_empty());
    }
    for value in [json!(0), json!(5), json!(2.5), json!("2")] {
        assert!(!check_field(schema, "server_worker_threads", value).is_empty());
    }
}

#[test]
fn rimworld_server_image_url_accepts_clear_and_http_urls_and_rejects_unsafe_syntax() {
    let schema = include_str!("../../../modules/rimworld/schema.json");
    for value in [
        "",
        "http://images.example.test/colony.png",
        "https://images.example.test/殖民地.png?size=400",
    ] {
        assert!(check_field(schema, "server_image_url", json!(value)).is_empty());
    }
    for value in [
        "javascript:alert(1)",
        "file:///image.png",
        "https://",
        "https://images.example.test/a b.png",
        "https://images.example.test/\nimage.png",
        "https://images.example.test/\u{0000}image.png",
        "https://images.example.test/\u{009f}image.png",
    ] {
        assert!(
            !check_field(schema, "server_image_url", json!(value)).is_empty(),
            "{value:?}"
        );
    }
    assert!(
        !check_field(
            schema,
            "server_image_url",
            json!("https://images.example.test/".to_owned() + &"x".repeat(2048))
        )
        .is_empty()
    );
}
