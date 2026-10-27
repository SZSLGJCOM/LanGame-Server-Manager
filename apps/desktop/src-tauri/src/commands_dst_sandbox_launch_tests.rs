use super::*;
use app_core::{AppSettings, InstanceDetails, InstanceStatus, InstanceSummary, ModuleDetails};

const PERMISSIONS: [&str; 3] = [
    "allow_ioopenwrite_sandbox_escape",
    "allow_persistentstorage_read_sandbox_escape",
    "allow_persistentstorage_write_sandbox_escape",
];

#[test]
fn dst_sandbox_permissions_are_independent_opt_ins_on_every_shard() {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let descriptor = app_modules::discover_modules(repository.join("modules"))
        .expect("discover repository modules")
        .into_iter()
        .find(|module| module.summary.id == "dontstarve")
        .expect("DST module");
    let module = ModuleDetails {
        summary: descriptor.summary,
        schema_json: descriptor.schema_json,
        default_ports: descriptor.default_ports,
        install: descriptor.install,
        process: descriptor.process,
        workshop: descriptor.workshop,
        mods: None,
        runtime: descriptor.runtime,
    };
    // No server package or file is created: only the production launch planner runs.
    let root = std::env::temp_dir().join("dst-sandbox-launch-contract");
    let settings = AppSettings {
        archives_root: String::new(),
        servers_root: root.join("instances").to_string_lossy().into_owned(),
        games_root: root.join("games").to_string_lossy().into_owned(),
        modules_root: repository.join("modules").to_string_lossy().into_owned(),
        steamcmd_root: root.join("steamcmd").to_string_lossy().into_owned(),
    };
    let mut instance = InstanceDetails {
        summary: InstanceSummary {
            id: String::from("dst-sandbox-launch-contract"),
            name: String::from("DST sandbox launch contract"),
            module_id: String::from("dontstarve"),
            status: InstanceStatus::Stopped,
            bind_ip: String::from("127.0.0.1"),
            port_count: module.default_ports.len(),
            autostart: false,
            active_process_count: 0,
        },
        config_file_path: root
            .join("config/instance.json")
            .to_string_lossy()
            .into_owned(),
        saves_path: root
            .join("config/clusters/main")
            .to_string_lossy()
            .into_owned(),
        backup_uses_declared_saves_path: true,
        auto_backup_on_stop: false,
        backup_retention_count: 10,
        settings_json: String::new(),
        ports: module.default_ports.clone(),
        active_run: None,
    };

    // Missing settings and every explicit true/false combination must preserve
    // the independent permissions, including the earlier ioopenwrite switch.
    for mask in 0..=8 {
        let mut configuration = serde_json::json!({
            "shard_layout": "island_adventures",
            "offline_cluster": true
        });
        if mask < 8 {
            for (index, permission) in PERMISSIONS.iter().enumerate() {
                configuration[*permission] = serde_json::json!(mask & (1 << index) != 0);
            }
        }
        instance.settings_json = configuration.to_string();
        for shard in &app_core::dst_shards::DST_SHARDS {
            let plan = build_dst_shard_launch_plan(&settings, &module, &instance, None, shard)
                .expect("build DST shard launch plan");
            assert!(
                plan.args
                    .windows(2)
                    .any(|pair| { pair[0] == "-shard" && pair[1] == shard.directory })
            );
            assert!(plan.args.windows(2).any(|pair| {
                pair[0] == "-ugc_directory"
                    && pair[1]
                        .replace('\\', "/")
                        .ends_with(&format!("/ugc/{}", shard.directory))
            }));
            for (index, permission) in PERMISSIONS.iter().enumerate() {
                let flag = format!("-{permission}");
                let expected = usize::from(mask < 8 && mask & (1 << index) != 0);
                assert_eq!(
                    plan.args
                        .iter()
                        .filter(|argument| **argument == flag)
                        .count(),
                    expected,
                    "{} mask {mask}: {permission}",
                    shard.directory
                );
            }
        }
    }
}
