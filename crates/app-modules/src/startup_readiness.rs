use crate::ModuleDiscoveryError;
use app_core::PortBinding;
use app_core::startup_readiness::{
    ModuleStartupReadinessSpec, StartupLogSource, StartupReadinessProbe,
};
use std::path::{Component, Path};

pub(super) fn validate(
    manifest: &Path,
    ports: &[PortBinding],
    spec: Option<&ModuleStartupReadinessSpec>,
) -> Result<(), ModuleDiscoveryError> {
    // Catalog discovery also serves uninstalled/custom modules. Missing evidence
    // is rejected before launching, not silently replaced by process liveness.
    let Some(spec) = spec else { return Ok(()) };
    let invalid = |message: &str| ModuleDiscoveryError::InvalidStartupReadiness {
        path: manifest.to_owned(),
        message: message.into(),
    };
    if !(1_000..=1_800_000).contains(&spec.timeout_ms) {
        return Err(invalid("timeout_ms must be between 1000 and 1800000"));
    }
    match &spec.probe {
        StartupReadinessProbe::RuntimeLog { source, .. } => match source {
            StartupLogSource::Instance { relative_path }
            | StartupLogSource::Install { relative_path } => {
                if !valid_relative_path(relative_path) {
                    return Err(invalid("log source must stay within its declared root"));
                }
            }
            StartupLogSource::LaunchArgument { name, .. } if !valid_argument(name) => {
                return Err(invalid("log argument name is invalid"));
            }
            _ => {}
        },
        StartupReadinessProbe::A2s {
            port_name,
            port_argument,
            ready_log,
        } => {
            if !ports
                .iter()
                .any(|port| port.name == *port_name && port.protocol == "udp")
            {
                return Err(invalid("A2S requires a declared UDP port"));
            }
            if port_argument
                .as_deref()
                .is_some_and(|name| !valid_argument(name))
            {
                return Err(invalid("port argument name is invalid"));
            }
            if let Some(source) = ready_log {
                let log_spec = ModuleStartupReadinessSpec {
                    timeout_ms: spec.timeout_ms,
                    probe: StartupReadinessProbe::RuntimeLog {
                        source: source.clone(),
                        fallback: None,
                    },
                };
                validate(manifest, ports, Some(&log_spec))?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn valid_argument(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-+_.".contains(&byte))
}

fn valid_relative_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && !value.contains([':', '\\'])
        && !value.chars().any(char::is_control)
        && Path::new(value)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bundled_module_declares_bounded_startup_evidence() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../modules");
        let modules = crate::discover_modules(root).unwrap();
        assert!(!modules.is_empty());
        for module in modules {
            assert!(
                module.runtime.startup_readiness.is_some(),
                "{}",
                module.summary.id
            );
            if let Some(ModuleStartupReadinessSpec {
                probe: StartupReadinessProbe::A2s { ready_log, .. },
                ..
            }) = &module.runtime.startup_readiness
            {
                assert!(
                    ready_log.is_some(),
                    "{} requires its final native loading phase",
                    module.summary.id
                );
            }
        }
    }

    #[test]
    fn rejects_escaping_paths_unknown_ports_and_unbounded_waits() {
        for path in [
            "../other.log",
            "/absolute.log",
            "C:/other.log",
            "a/../../b",
            "a\\b",
            "",
        ] {
            let spec = ModuleStartupReadinessSpec {
                timeout_ms: 60_000,
                probe: StartupReadinessProbe::RuntimeLog {
                    fallback: None,
                    source: StartupLogSource::Install {
                        relative_path: path.into(),
                    },
                },
            };
            assert!(
                validate(Path::new("module.toml"), &[], Some(&spec)).is_err(),
                "{path}"
            );
        }
        let mut spec = ModuleStartupReadinessSpec {
            timeout_ms: 0,
            probe: StartupReadinessProbe::DstWorld,
        };
        assert!(validate(Path::new("module.toml"), &[], Some(&spec)).is_err());
        spec.timeout_ms = 600_000;
        spec.probe = StartupReadinessProbe::A2s {
            port_name: "missing".into(),
            port_argument: None,
            ready_log: None,
        };
        assert!(validate(Path::new("module.toml"), &[], Some(&spec)).is_err());
    }
}
