use super::*;
use serde_json::json;

fn mod_arguments(active: &str, passive: &str) -> Vec<String> {
    let settings = json!({"mod_ids_csv": active, "passive_mod_ids_csv": passive});
    let instance: InstanceDetails = serde_json::from_value(json!({
        "summary": {"id": "asa-mods", "name": "ASA Mods", "module_id": "arksurvivalascended",
            "status": "Stopped", "bind_ip": "127.0.0.1", "port_count": 0, "autostart": false},
        "config_file_path": "config/settings.json", "saves_path": "saves",
        "auto_backup_on_stop": false, "backup_retention_count": 0,
        "settings_json": settings.to_string(), "ports": [], "active_run": null
    }))
    .unwrap();
    let path = Path::new("instance");
    let context = TemplateContext {
        instance: &instance,
        settings: &settings,
        install_root: path,
        config_dir: path,
        data_dir: path,
        logs_dir: path,
        saves_dir: path,
    };
    // These are the actual ASA manifest tokens; use the launch argument expansion,
    // not just its ID parser, so accidental newlines become observable arguments.
    ["{{arksa.mod_ids_flag}}", "{{arksa.official_launch_flags}}"]
        .into_iter()
        .flat_map(|token| expand_resolved_argument_segments(token, &context))
        .collect()
}

#[test]
fn ark_mods_multiline_passive_ids_expand_to_one_normalized_argument() {
    let args = mod_arguments("1346144", "1346145\n1346146\n1346145");
    assert_eq!(args, ["-mods=1346144", "-passivemods=1346145,1346146"]);
}

#[test]
fn ark_mods_passive_references_share_active_normalization_without_crossing_modes() {
    let args = mod_arguments(
        "1346144",
        "cf-1346145;https://www.curseforge.com/projects/1346146",
    );
    assert_eq!(args, ["-mods=1346144", "-passivemods=1346145,1346146"]);
    assert_eq!(mod_arguments("", ""), Vec::<String>::new());
    assert_eq!(mod_arguments("", "not-a-project"), Vec::<String>::new());
}
