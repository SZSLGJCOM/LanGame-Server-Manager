use super::*;
use crate::templates::*;
use serde_json::{Map, Value, json};
use std::fs;
use std::path::PathBuf;

struct TestRoot(PathBuf);

impl TestRoot {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "langame-rust-november-{}",
            uuid::Uuid::new_v4().as_simple()
        ));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    fn config(&self) -> PathBuf {
        self.0.join("instance/config")
    }

    fn live(&self) -> PathBuf {
        self.0.join("install/server/rust-november/cfg/server.cfg")
    }

    fn write(&self, settings: Value, running: bool) -> ManagedConfigMutation {
        let config = self.config();
        let install = self.0.join("install");
        let saves = self.0.join("saves");
        fs::create_dir_all(&config).unwrap();
        fs::create_dir_all(&install).unwrap();
        let paths = StoragePaths {
            app_data_root: self.0.join("appdata"),
            settings_path: self.0.join("appdata/settings.json"),
            database_path: self.0.join("appdata/db/lgs.db"),
            logs_root: self.0.join("appdata/logs"),
            modules_root: self.0.join("modules"),
            migrations_root: self.0.join("migrations"),
            steamcmd_root: self.0.join("steamcmd"),
            games_root: self.0.join("games"),
            instances_root: self.0.join("instances"),
            archives_root: self.0.join("instances/.trash"),
        };
        let ports = [PortBinding {
            name: "rcon".into(),
            protocol: "tcp".into(),
            port: 28016,
        }];
        let settings = settings.as_object().unwrap();
        let input = ModuleTemplateRenderInput {
            config_dir: &config,
            install_root: &install,
            saves_dir: &saves,
            instance_id: "rust-november",
            instance_name: "Rust November",
            module_id: "rust",
            bind_ip: "127.0.0.1",
            autostart: false,
            settings,
            ports: &ports,
        };
        write_pending_instance_configuration(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../modules/rust/templates"),
            &input,
            &ModuleSupportMaterializationContext {
                storage_paths: &paths,
                module_id: "rust",
                install_root: &install,
                shared_install_root: &install,
                config_dir: &config,
                saves_dir: &saves,
                instance_id: "rust-november",
                instance_running: running,
                settings,
            },
            &config.join("instance.json"),
            InstanceConfigInput {
                instance_id: input.instance_id,
                instance_name: input.instance_name,
                module_id: "rust",
                bind_ip: input.bind_ip,
                autostart: false,
                settings: settings.clone(),
                ports: &ports,
            },
            None,
        )
        .unwrap()
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn render_settings(settings: &Map<String, Value>) -> String {
    let defaults = collect_schema_defaults_from_schema_json(
        Some(include_str!("../../../../modules/rust/schema.json")),
        SchemaDefaultContext::default(),
    )
    .unwrap();
    let root = Path::new("C:/LanGame/rust-november-fixture");
    let ports = [PortBinding {
        name: "rcon".into(),
        protocol: "tcp".into(),
        port: 28016,
    }];
    render_template_text(
        include_str!("../../../../modules/rust/templates/server.cfg.hbs"),
        &TemplateRenderContext {
            instance_root: root,
            config_dir: root,
            install_root: root,
            data_dir: root,
            logs_dir: root,
            saves_dir: root,
            instance_id: "rust-november",
            instance_name: "Rust November",
            module_id: "rust",
            bind_ip: "127.0.0.1",
            autostart: false,
            schema_defaults: &defaults,
            settings,
            ports: &ports,
        },
    )
    .unwrap()
}

#[test]
fn rust_november_unset_overrides_preserve_native_values_through_materialization_and_rollback() {
    let root = TestRoot::new();
    let source = root.0.join("rendered.cfg");
    let destination = root.0.join("server.cfg");
    let original = concat!(
        "\u{feff}position_lod \"2\"\r\n",
        "NET.POSITION_LOD_PLAYERS \"1\"\r\n",
        "batchpackets \"false\"\r\n",
        "UsePlayerUpdateJobs \"3\""
    );
    let rendered = render_settings(&Map::new());
    for command in OPTIONAL_PERFORMANCE_COMMANDS {
        assert!(!rendered.lines().any(|line| command_matches(line, command)));
    }
    fs::write(&source, rendered).unwrap();
    fs::write(&destination, original).unwrap();
    let mut mutation = ManagedConfigMutation::new("rust");
    materialize_server_configuration(&source, &destination, &Map::new(), &mut mutation).unwrap();
    let merged = fs::read_to_string(&destination).unwrap();
    for line in [
        "position_lod \"2\"",
        "NET.POSITION_LOD_PLAYERS \"1\"",
        "batchpackets \"false\"",
        "UsePlayerUpdateJobs \"3\"",
    ] {
        assert_eq!(merged.matches(line).count(), 1, "{line}");
        assert!(merged.find(line).unwrap() < merged.rfind("server.writecfg").unwrap());
    }
    materialize_server_configuration(&source, &destination, &Map::new(), &mut mutation).unwrap();
    assert_eq!(fs::read_to_string(&destination).unwrap(), merged);
    mutation.rollback().unwrap();
    assert_eq!(fs::read_to_string(destination).unwrap(), original);
}

#[test]
fn rust_november_explicit_values_and_expert_lines_keep_their_existing_precedence() {
    let root = TestRoot::new();
    let source = root.0.join("rendered.cfg");
    let destination = root.0.join("server.cfg");
    let settings = json!({
        "network_position_lod": 0,
        "network_position_lod_players": 2,
        "batch_packets": false,
        "player_update_jobs": 4,
        "server_cfg_extra": "net.position_lod 1"
    });
    let rendered = render_settings(settings.as_object().unwrap());
    for line in [
        "net.position_lod 0",
        "net.position_lod_players 2",
        "server.batchpackets false",
        "server.useplayerupdatejobs 4",
    ] {
        assert!(
            rendered.lines().any(|candidate| candidate == line),
            "{line}"
        );
    }
    assert!(
        rendered.find("net.position_lod 0").unwrap() < rendered.find("net.position_lod 1").unwrap()
    );
    fs::write(&source, &rendered).unwrap();
    fs::write(
        &destination,
        "position_lod 2\nposition_lod_players 1\nbatchpackets true\nUsePlayerUpdateJobs 3\n",
    )
    .unwrap();
    let mut mutation = ManagedConfigMutation::new("rust");
    materialize_server_configuration(
        &source,
        &destination,
        settings.as_object().unwrap(),
        &mut mutation,
    )
    .unwrap();
    assert_eq!(fs::read_to_string(&destination).unwrap(), rendered);
    mutation.commit();

    // Clearing structured overrides retains their native values, while removing
    // an expert override must still remove its separately owned occurrence.
    fs::write(&source, render_settings(&Map::new())).unwrap();
    let mut cleared = ManagedConfigMutation::new("rust");
    materialize_server_configuration(&source, &destination, &Map::new(), &mut cleared).unwrap();
    let preserved = fs::read_to_string(destination).unwrap();
    for line in [
        "net.position_lod 0",
        "net.position_lod_players 2",
        "server.batchpackets false",
        "server.useplayerupdatejobs 4",
    ] {
        assert_eq!(
            preserved
                .lines()
                .filter(|candidate| *candidate == line)
                .count(),
            1
        );
    }
    assert!(!preserved.lines().any(|line| line == "net.position_lod 1"));
    cleared.commit();
}

#[test]
fn rust_november_expert_only_override_replaces_native_value_without_injecting_other_defaults() {
    let rendered = render_settings(
        json!({"server_cfg_extra": "POSITION_LOD 0"})
            .as_object()
            .unwrap(),
    );
    let merged = preserve_optional_performance_lines(&rendered, "net.position_lod 2\n", &[]);
    assert_eq!(merged, rendered);
    for command in [
        "net.position_lod_players",
        "server.batchpackets",
        "server.useplayerupdatejobs",
    ] {
        assert!(!merged.lines().any(|line| command_matches(line, command)));
    }
}

#[test]
fn rust_november_only_native_console_aliases_match_performance_settings() {
    for command in OPTIONAL_PERFORMANCE_COMMANDS {
        let (_, bare) = command.rsplit_once('.').unwrap();
        assert!(command_matches(&format!("{command} 1"), command));
        assert!(command_matches(
            &format!("{} 1", bare.to_ascii_uppercase()),
            command
        ));
        assert!(!command_matches(&format!("global.{bare} 1"), command));
        assert!(!command_matches(&format!("// {command} 1"), command));
        assert!(!command_assigns(command, command));
        assert!(!command_assigns(&format!(" {bare} \t"), command));
        // Native parsing retains empty quoted arguments and does not treat //
        // as an inline comment; both forms still call the variable setter.
        assert!(command_assigns(&format!("{command} \"\""), command));
        assert!(command_assigns(&format!("{command} // comment"), command));
    }
}

#[test]
fn rust_november_consecutive_saves_remove_raw_assignments_and_keep_unmanaged_native_values() {
    let fixture = TestRoot::new();
    fs::create_dir_all(fixture.live().parent().unwrap()).unwrap();
    fs::write(
        fixture.live(),
        "net.position_lod_players 1\nserver.batchpackets false\n",
    )
    .unwrap();
    fixture
        .write(
            json!({"server_cfg_extra": "POSITION_LOD 2\nUsePlayerUpdateJobs 3"}),
            false,
        )
        .commit();
    let before = fs::read(fixture.live()).unwrap();
    let ownership_before = fs::read(fixture.config().join(RAW_OWNERSHIP_FILE)).unwrap();
    let cleared = fixture.write(json!({"server_cfg_extra": ""}), false);
    let output = fs::read_to_string(fixture.live()).unwrap();
    assert!(
        !output
            .lines()
            .any(|line| command_assigns(line, "net.position_lod"))
    );
    assert!(
        !output
            .lines()
            .any(|line| command_assigns(line, "server.useplayerupdatejobs"))
    );
    assert!(output.contains("net.position_lod_players 1"));
    assert!(output.contains("server.batchpackets false"));
    cleared.rollback().unwrap();
    assert_eq!(fs::read(fixture.live()).unwrap(), before);
    assert_eq!(
        fs::read(fixture.config().join(RAW_OWNERSHIP_FILE)).unwrap(),
        ownership_before
    );
    fixture
        .write(json!({"server_cfg_extra": ""}), false)
        .commit();
    fixture.write(json!({}), false).commit();
    assert_eq!(fs::read_to_string(fixture.live()).unwrap(), output);
}

#[test]
fn rust_november_legacy_raw_can_be_cleared_on_the_first_stopped_save() {
    let fixture = TestRoot::new();
    fixture
        .write(json!({"server_cfg_extra": "net.position_lod 2"}), false)
        .commit();
    fs::remove_file(fixture.config().join(RAW_OWNERSHIP_FILE)).unwrap();
    fixture
        .write(json!({"server_cfg_extra": ""}), false)
        .commit();
    assert!(
        !fs::read_to_string(fixture.live())
            .unwrap()
            .lines()
            .any(|line| command_assigns(line, "net.position_lod"))
    );
}

#[test]
fn rust_november_legacy_raw_ownership_survives_running_saves_until_restart() {
    let fixture = TestRoot::new();
    fixture
        .write(json!({"server_cfg_extra": "net.position_lod 2"}), false)
        .commit();
    // Earlier releases have the same generated cfg but no ownership record.
    fs::remove_file(fixture.config().join(RAW_OWNERSHIP_FILE)).unwrap();
    let live_before = fs::read(fixture.live()).unwrap();
    fixture
        .write(json!({"server_cfg_extra": ""}), true)
        .commit();
    assert_eq!(fs::read(fixture.live()).unwrap(), live_before);
    let captured = fs::read(fixture.config().join(RAW_OWNERSHIP_FILE)).unwrap();
    fixture
        .write(
            json!({"server_cfg_extra": "server.description Later"}),
            true,
        )
        .commit();
    assert_eq!(
        fs::read(fixture.config().join(RAW_OWNERSHIP_FILE)).unwrap(),
        captured
    );
    assert_eq!(fs::read(fixture.live()).unwrap(), live_before);
    fixture
        .write(
            json!({"server_cfg_extra": "server.description Later"}),
            false,
        )
        .commit();
    let output = fs::read_to_string(fixture.live()).unwrap();
    assert!(
        !output
            .lines()
            .any(|line| command_assigns(line, "net.position_lod"))
    );
    assert!(output.contains("server.description Later"));
}

#[test]
fn rust_november_raw_queries_never_replace_or_claim_native_assignments() {
    let fixture = TestRoot::new();
    fs::create_dir_all(fixture.live().parent().unwrap()).unwrap();
    fs::write(fixture.live(), "net.position_lod 2\n").unwrap();
    fixture
        .write(json!({"server_cfg_extra": "position_lod"}), false)
        .commit();
    let output = fs::read_to_string(fixture.live()).unwrap();
    assert!(output.lines().any(|line| line == "position_lod"));
    assert!(output.lines().any(|line| line == "net.position_lod 2"));
    fixture
        .write(json!({"server_cfg_extra": ""}), false)
        .commit();
    let output = fs::read_to_string(fixture.live()).unwrap();
    assert!(!output.lines().any(|line| line == "position_lod"));
    assert!(output.lines().any(|line| line == "net.position_lod 2"));
}

#[test]
fn rust_november_cleared_raw_keeps_identical_structured_occurrence_and_operator_edits() {
    let fixture = TestRoot::new();
    fixture.write(json!({"network_position_lod": 2, "server_cfg_extra": "net.position_lod 2\nserver.batchpackets false"}), false).commit();
    let edited = fs::read_to_string(fixture.live())
        .unwrap()
        .replace("server.batchpackets false", "server.batchpackets true");
    fs::write(fixture.live(), edited).unwrap();
    fixture
        .write(json!({"server_cfg_extra": ""}), false)
        .commit();
    let output = fs::read_to_string(fixture.live()).unwrap();
    assert_eq!(
        output
            .lines()
            .filter(|line| *line == "net.position_lod 2")
            .count(),
        1
    );
    assert!(
        output
            .lines()
            .any(|line| line == "server.batchpackets true")
    );
}
