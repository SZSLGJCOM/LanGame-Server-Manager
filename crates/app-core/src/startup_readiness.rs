use serde::{Deserialize, Serialize};

/// A module declares evidence; the host owns the startup deadline, process
/// identity, cancellation and cleanup. A live process or socket is not evidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleStartupReadinessSpec {
    pub timeout_ms: u64,
    pub probe: StartupReadinessProbe,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum StartupReadinessProbe {
    RuntimeLog {
        source: StartupLogSource,
        #[serde(default)]
        fallback: Option<StartupReadinessFallback>,
    },
    A2s {
        port_name: String,
        /// Per-process argv takes precedence over the instance's primary port.
        #[serde(default)]
        port_argument: Option<String>,
        /// A query can be initialized before the world; require the module's
        /// final native phase as well when that phase is observable.
        #[serde(default)]
        ready_log: Option<StartupLogSource>,
    },
    AstroneerWorld,
    ReturntomoriaStatus,
    DstWorld,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartupReadinessFallback {
    PalworldRest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum StartupLogSource {
    Managed,
    /// Native logs redirected by the final launch plan (including map children).
    LaunchArgument {
        name: String,
        #[serde(default)]
        fallback_to_managed: bool,
    },
    Instance {
        relative_path: String,
    },
    Install {
        relative_path: String,
    },
    CorekeeperNative,
}
