use super::*;
use crate::settings_validation::{SettingsValidationPhase, collect_settings_schema_diagnostics};

const NATIVE_INI: &str = "RSDragonwilds/Saved/Config/WindowsServer/DedicatedServer.ini";
const SECTION: &str = "[/Script/Dominion.DedicatedServerSettings]";
const POLICIES: [&str; 5] = ["Crossplay", "PC", "PlayStation", "Xbox", "Nintendo"];

struct PlatformFixture {
    root: PathBuf,
    paths: StoragePaths,
    config: PathBuf,
    install: PathBuf,
}

impl PlatformFixture {
    fn new(existing: Option<&str>) -> Self {
        let root = unique_test_root();
        let paths = test_storage_paths(&root);
        let config = root.join("instance/config");
        let install = root.join("instance/runtime");
        fs::create_dir_all(&config).unwrap();
        fs::create_dir_all(&install).unwrap();
        if let Some(existing) = existing {
            let native = install.join(NATIVE_INI);
            fs::create_dir_all(native.parent().unwrap()).unwrap();
            fs::write(native, existing).unwrap();
        }
        Self {
            root,
            paths,
            config,
            install,
        }
    }

    fn materialize(&self, policy: Option<&str>) -> String {
        let mut settings = collect_schema_defaults_from_schema_json(
            Some(include_str!(
                "../../../modules/runescapedragonwilds/schema.json"
            )),
            SchemaDefaultContext {
                instance_id: Some("platform-policy"),
                instance_name: Some("Policy room"),
            },
        )
        .unwrap();
        settings.remove("platform_policy");
        if let Some(policy) = policy {
            settings.insert("platform_policy".into(), Value::String(policy.into()));
        }
        let saves = self.root.join("saves");
        let ports = [
            PortBinding {
                name: "game".into(),
                protocol: "udp".into(),
                port: 7777,
            },
            PortBinding {
                name: "query".into(),
                protocol: "udp".into(),
                port: 27057,
            },
        ];
        let templates = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../modules/runescapedragonwilds/templates");
        render_module_templates(
            &templates,
            &ModuleTemplateRenderInput {
                config_dir: &self.config,
                install_root: &self.install,
                saves_dir: &saves,
                instance_id: "platform-policy",
                instance_name: "Policy room",
                module_id: "runescapedragonwilds",
                bind_ip: "127.0.0.1",
                autostart: false,
                settings: &settings,
                ports: &ports,
            },
        )
        .unwrap();
        materialize_module_support_files(&ModuleSupportMaterializationContext {
            storage_paths: &self.paths,
            module_id: "runescapedragonwilds",
            install_root: &self.install,
            shared_install_root: &self.install,
            config_dir: &self.config,
            saves_dir: &saves,
            instance_id: "platform-policy",
            instance_running: false,
            settings: &settings,
        })
        .unwrap();
        fs::read_to_string(self.install.join(NATIVE_INI)).unwrap()
    }
}

impl Drop for PlatformFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn policy_lines(text: &str) -> Vec<&str> {
    text.lines()
        .filter(|line| line.starts_with("PlatformPolicy="))
        .collect()
}

#[test]
fn unset_and_cleared_overrides_preserve_existing_native_restriction_and_state() {
    let fixture = PlatformFixture::new(Some(&format!(
        "{SECTION}\r\nPlatformPolicy=PC\r\nServerGuid=stable-guid\r\nKnownPlayerList=retained-state\r\nUnknownFuture=keep\r\n"
    )));
    for policy in [None, Some(""), None] {
        let native = fixture.materialize(policy);
        assert_eq!(policy_lines(&native), ["PlatformPolicy=PC"]);
        for value in [
            "ServerGuid=stable-guid",
            "KnownPlayerList=retained-state",
            "UnknownFuture=keep",
        ] {
            assert!(native.contains(value));
        }
        assert!(native.contains("ServerName=Dragonwilds"));
    }
}

#[test]
fn every_native_platform_choice_replaces_prior_policy_without_duplicates() {
    let fixture = PlatformFixture::new(Some(&format!(
        "{SECTION}\nPlatformPolicy=PC\nServerGuid=stable-guid\n"
    )));
    for policy in POLICIES {
        let expected = format!("PlatformPolicy={policy}");
        assert_eq!(
            policy_lines(&fixture.materialize(Some(policy))),
            [expected.as_str()]
        );
        assert_eq!(
            policy_lines(&fixture.materialize(Some(""))),
            [expected.as_str()]
        );
    }
}

#[test]
fn new_instance_without_override_leaves_the_native_crossplay_default_unwritten() {
    let fixture = PlatformFixture::new(None);
    for policy in [None, Some("")] {
        assert!(policy_lines(&fixture.materialize(policy)).is_empty());
        assert!(
            !fs::read_to_string(fixture.config.join(NATIVE_INI))
                .unwrap()
                .contains("PlatformPolicy=")
        );
    }
}

#[test]
fn schema_accepts_only_unset_or_verified_native_names() {
    let schema: Value = serde_json::from_str(include_str!(
        "../../../modules/runescapedragonwilds/schema.json"
    ))
    .unwrap();
    for policy in std::iter::once("").chain(POLICIES) {
        let settings = json!({"platform_policy":policy})
            .as_object()
            .unwrap()
            .clone();
        assert!(
            collect_settings_schema_diagnostics(
                &schema,
                &settings,
                SettingsValidationPhase::Complete
            )
            .is_empty()
        );
    }
    for policy in [
        json!("Steam"),
        json!("Playstation"),
        json!("PC\nOwnerId=injected"),
        json!(0),
        json!(false),
    ] {
        let settings = json!({"platform_policy":policy})
            .as_object()
            .unwrap()
            .clone();
        assert!(
            collect_settings_schema_diagnostics(
                &schema,
                &settings,
                SettingsValidationPhase::Complete
            )
            .iter()
            .any(|issue| issue.field == "platform_policy")
        );
    }
}
