use super::*;
use serde_json::json;

#[test]
fn enshrouded_invalid_account_hashes_fail_rendering_without_changing_files() {
    let root = unique_test_root();
    let templates = root.join("templates");
    let config = root.join("config");
    fs::create_dir_all(&templates).unwrap();
    fs::create_dir_all(&config).unwrap();
    fs::write(
        templates.join("enshrouded_server.json.hbs"),
        "{\"bans\":{{enshrouded.bans_json}}}",
    )
    .unwrap();
    let path = config.join("enshrouded_server.json");
    let original = b"{\"bans\":[{\"accountId\":18446744073709551615,\"displayName\":\"keep\"}]}";
    fs::write(&path, original).unwrap();
    for invalid in [
        json!("123,invalid,456"),
        json!("18446744073709551616"),
        json!(123),
        Value::Null,
    ] {
        let settings = json!({"banned_player_ids": invalid});
        let input = ModuleTemplateRenderInput {
            config_dir: &config,
            install_root: &root,
            saves_dir: &root,
            instance_id: "isolated-enshrouded",
            instance_name: "Isolated Enshrouded",
            module_id: "enshrouded",
            bind_ip: "127.0.0.1",
            autostart: false,
            settings: settings.as_object().unwrap(),
            ports: &[],
        };
        assert!(render_module_templates(&templates, &input).is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
    }
    fs::remove_dir_all(root).unwrap();
}
