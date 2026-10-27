use super::*;
use serde_json::json;

fn render(settings: &Value, runtime: &Path) -> Vec<String> {
    let instance: InstanceDetails = serde_json::from_value(json!({
        "summary": {"id":"abiotic-mod-fixture","name":"Abiotic Mods","module_id":"abioticfactor",
            "status":"Stopped","bind_ip":"0.0.0.0","port_count":0,"autostart":false},
        "config_file_path":"D:/fixture/config/instance.json","saves_path":"D:/fixture/saves",
        "auto_backup_on_stop":false,"backup_retention_count":0,"settings_json":"{}","ports":[],"active_run":null
    })).unwrap();
    let context = TemplateContext {
        instance: &instance,
        settings,
        install_root: runtime,
        config_dir: Path::new("D:/fixture/config"),
        data_dir: Path::new("D:/fixture/data"),
        logs_dir: Path::new("D:/fixture/logs"),
        saves_dir: Path::new("D:/fixture/saves"),
    };
    render_abioticfactor_mod_args(&context)
}

#[test]
fn abiotic_launch_value_quotes_are_rejected_before_start_without_echoing_values() {
    for field in [
        "server_name",
        "world_save_name",
        "server_password",
        "admin_password",
    ] {
        let value = "synthetic\"test-only";
        let settings = json!({(field): value});
        let issues = collect_abioticfactor_launch_setting_issues(&settings);
        assert_eq!(issues.len(), 1);
        let issue = &issues[0];
        assert_eq!(issue.code, "abiotic_launch_value_quote_unsupported");
        assert_eq!(issue.severity, "error");
        assert_eq!(
            issue.context.get("setting_key").map(String::as_str),
            Some(field)
        );
        let public_issue = serde_json::to_string(issue).unwrap();
        assert!(!public_issue.contains(value));
        assert!(!public_issue.contains("synthetic"));
        assert!(issue.path.is_none());
    }
}

#[test]
fn abiotic_launch_values_keep_native_punctuation_and_password_backslashes() {
    for value in [
        "synthetic-test-only room",
        "synthetic-test-only,room",
        "synthetic-test-only)room",
        "合成世界 (one,two)",
    ] {
        let settings = json!({
            "server_name": value,
            "world_save_name": value,
            "server_password": value,
            "admin_password": value,
        });
        assert!(collect_abioticfactor_launch_setting_issues(&settings).is_empty());
    }
    for field in ["server_password", "admin_password"] {
        let settings = json!({(field): r"synthetic-test-only room\"});
        assert!(collect_abioticfactor_launch_setting_issues(&settings).is_empty());
    }
}

#[test]
fn abiotic_password_flags_preserve_literal_whitespace_and_backslashes() {
    for (field, prefix) in [
        ("server_password", "-ServerPassword="),
        ("admin_password", "-AdminPassword="),
    ] {
        for value in [" synthetic-test-only ", " ", r"synthetic-test-only room\"] {
            assert_eq!(
                render_abioticfactor_password_flag(&json!({(field): value}), field, prefix),
                format!("{prefix}{value}")
            );
        }
        for settings in [json!({}), json!({(field): ""})] {
            assert!(render_abioticfactor_password_flag(&settings, field, prefix).is_empty());
        }
    }
}

#[test]
fn abiotic_world_names_are_single_windows_directory_names() {
    for value in [
        "",
        ".",
        "..",
        "../other",
        "nested/world",
        r"nested\world",
        "CON",
        "NUL",
        "COM1.txt",
        "world.",
        "world ",
        "world:stream",
    ] {
        let issues =
            collect_abioticfactor_launch_setting_issues(&json!({"world_save_name": value}));
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code, "abiotic_world_save_name_invalid");
    }
    assert!(collect_abioticfactor_launch_setting_issues(&json!({})).is_empty());
}

#[test]
fn abiotic_official_mods_do_not_pass_an_unused_account_token() {
    assert!(
        render(
            &json!({"modio_token":"fixture-token"}),
            Path::new("D:/fixture/runtime")
        )
        .is_empty()
    );
    assert_eq!(
        render(
            &json!({"mod_ids":"6472275;6472552"}),
            Path::new("D:/fixture/runtime")
        ),
        ["-Mods=6472275;6472552", "-ModInstallTimeout=900"]
    );
}

#[test]
fn abiotic_official_download_arguments_match_documented_delimiters_and_paths() {
    let root = Path::new("D:/fixture/instance-one/runtime");
    let args = render(
        &json!({
            "mod_ids":"6472275;6472552\n6472275,6456360", "modio_token":"fixture-token",
            "mod_install_timeout_seconds":1200, "override_mod_warnings":true
        }),
        root,
    );
    assert_eq!(
        args,
        [
            "-Mods=6472275;6472552;6456360".to_string(),
            "-ModioToken=fixture-token".to_string(),
            format!(
                "-ModsDir={}",
                root.join("AbioticFactor")
                    .join("Saved")
                    .join("mod.io")
                    .display()
            ),
            "-ModInstallTimeout=1200".to_string(),
            "-OverrideModWarnings".to_string()
        ]
    );
}

#[test]
fn abiotic_sideloading_requires_ids_and_does_not_send_download_credentials() {
    let settings = json!({"mod_ids":"6472275", "sideload_mods":true,
        "sideload_mods_path":"AbioticFactor/Mods/native packages", "modio_token":"unused-fixture-token"});
    let root = Path::new("D:/fixture/runtime");
    assert_eq!(
        render(&settings, root),
        [
            "-Mods=6472275".to_string(),
            "-SideloadMods".to_string(),
            format!(
                "-SideloadModsPath={}",
                root.join("AbioticFactor")
                    .join("Mods")
                    .join("native packages")
                    .display()
            )
        ]
    );
    assert!(
        collect_abioticfactor_launch_setting_issues(&json!({"sideload_mods":true}))
            .iter()
            .any(|issue| issue.code == "abiotic_sideload_ids_required")
    );
}

#[test]
fn abiotic_mod_directories_belong_to_the_selected_instance() {
    let settings = json!({"mod_ids":"6472275", "modio_token":"fixture-token",
        "mods_directory":"AbioticFactor/Saved/curated mods"});
    let first = render(&settings, Path::new("D:/fixture/one/runtime"));
    let second = render(&settings, Path::new("D:/fixture/two/runtime"));
    assert_ne!(first[2], second[2]);
    assert!(first[2].contains("one"));
    assert!(!first[2].contains("two"));
    assert!(second[2].contains("two"));
}

#[test]
fn abiotic_rejects_external_traversal_and_windows_special_directory_names() {
    for path in [
        "D:/shared/mods",
        "//server/share",
        "/root/mods",
        "AbioticFactor/Saved/../Mods",
        "AbioticFactor/Saved/cache/../../Binaries",
        "AbioticFactor/Saved/NUL",
        "AbioticFactor/Saved/COM1.txt",
        "AbioticFactor/Saved/cache.",
        "AbioticFactor/Saved/cache:stream",
        "AbioticFactor/Saved//cache",
    ] {
        assert!(directory_parts(path, true).is_none(), "{path}");
    }
    assert!(directory_parts("AbioticFactor/Saved/cache", true).is_some());
    assert!(directory_parts("AbioticFactor/Mods/curated", false).is_some());
    assert!(directory_parts("AbioticFactor/Saved/cache", false).is_none());
    assert!(directory_parts("AbioticFactor/Mods", true).is_none());
}

#[test]
fn abiotic_rejects_partial_mod_lists_bad_tokens_and_unusable_download_overrides() {
    for settings in [
        json!({"mod_ids":"6472275;bad"}),
        json!({"mod_ids":"0"}),
        json!({"mod_ids":"+1"}),
        json!({"mod_ids":"18446744073709551616"}),
        json!({"mod_ids":"-Mods=6472275"}),
        json!({"mod_ids":"6472275", "modio_token":"fixture\n-token"}),
        json!({"mod_ids":"6472275", "mods_directory":"AbioticFactor/Saved/custom"}),
        json!({"mod_ids":"6472275", "mod_install_timeout_seconds":0}),
        json!({"mod_ids":"6472275", "mod_install_timeout_seconds":0.5}),
    ] {
        assert!(!collect_abioticfactor_launch_setting_issues(&settings).is_empty());
        assert!(render(&settings, Path::new("D:/fixture/runtime")).is_empty());
    }
}
