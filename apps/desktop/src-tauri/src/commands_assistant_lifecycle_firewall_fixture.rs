//! Test-only replacement for the external Windows Firewall API. Native launch,
//! program validation, bind verification, stop, and assistant confirmation stay real.
use super::*;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

struct Permit {
    database: PathBuf,
    instance_root: PathBuf,
    program_root: PathBuf,
    port: u16,
    executable: Vec<u8>,
}

fn permits() -> &'static Mutex<HashMap<String, Permit>> {
    static PERMITS: OnceLock<Mutex<HashMap<String, Permit>>> = OnceLock::new();
    PERMITS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(super) struct FirewallFixtureGuard(String);

impl FirewallFixtureGuard {
    pub(super) fn register(
        root: &Path,
        storage: &StorageBootstrap,
        instance: &InstanceDetails,
        compiled: &Path,
    ) -> Result<Self, String> {
        let root = root.canonicalize().map_err(|error| error.to_string())?;
        let temporary = std::env::temp_dir()
            .canonicalize()
            .map_err(|error| error.to_string())?;
        if !root.starts_with(&temporary)
            || !root
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| name.starts_with("lg-test-assistantlif-"))
            || storage
                .paths
                .instances_root
                .canonicalize()
                .map_err(|error| error.to_string())?
                != root.join("instances")
            || compiled.canonicalize().map_err(|error| error.to_string())?
                != root.join("games/necesse/jre/bin/java.exe")
        {
            return Err("The firewall fixture requires its owned disposable package roots.".into());
        }
        let permit = Permit {
            database: storage
                .paths
                .database_path
                .canonicalize()
                .map_err(|error| error.to_string())?,
            instance_root: storage
                .paths
                .instances_root
                .join(&instance.summary.id)
                .canonicalize()
                .map_err(|error| error.to_string())?,
            program_root: root.join("games/necesse"),
            port: instance
                .ports
                .first()
                .ok_or("Missing fixture UDP port")?
                .port,
            executable: program_bytes(compiled)?,
        };
        validate(&permit, storage, instance)?;
        let mut permits = permits()
            .lock()
            .map_err(|_| "Fixture firewall lock poisoned")?;
        if permits.contains_key(&instance.summary.id) {
            return Err("Fixture instance already has a firewall capability.".into());
        }
        permits.insert(instance.summary.id.clone(), permit);
        Ok(Self(instance.summary.id.clone()))
    }
}

impl Drop for FirewallFixtureGuard {
    fn drop(&mut self) {
        if let Ok(mut permits) = permits().lock() {
            permits.remove(&self.0);
        }
    }
}

pub(super) fn applies(
    storage: &StorageBootstrap,
    instance: &InstanceDetails,
) -> Result<bool, String> {
    let permits = permits()
        .lock()
        .map_err(|_| "Fixture firewall lock poisoned")?;
    let Some(permit) = permits.get(&instance.summary.id) else {
        return Ok(false);
    };
    validate(permit, storage, instance)?;
    Ok(true)
}

fn validate(
    permit: &Permit,
    storage: &StorageBootstrap,
    instance: &InstanceDetails,
) -> Result<(), String> {
    if instance.summary.module_id != "necesse"
        || instance.summary.bind_ip != "127.0.0.1"
        || instance.ports.len() != 1
        || instance.ports[0].name != "game"
        || instance.ports[0].protocol != "udp"
        || instance.ports[0].port == 0
        || instance.ports[0].port != permit.port
        || storage
            .paths
            .database_path
            .canonicalize()
            .map_err(|error| error.to_string())?
            != permit.database
        || Path::new(&instance.config_file_path)
            .canonicalize()
            .map_err(|error| error.to_string())?
            != permit.instance_root.join("config/instance.json")
    {
        return Err("The registered firewall fixture target or loopback port changed.".into());
    }
    let runtime = PathBuf::from(private_runtime_install_root(instance)?)
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if !app_storage::instance_uses_exclusive_program(&permit.instance_root)
        .map_err(|error| error.to_string())?
        || runtime != permit.program_root
        || program_bytes(&runtime.join("jre/bin/java.exe"))? != permit.executable
    {
        return Err("The registered firewall fixture program changed.".into());
    }
    Ok(())
}

fn program_bytes(path: &Path) -> Result<Vec<u8>, String> {
    use std::os::windows::fs::MetadataExt;
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !metadata.is_file()
        || metadata.file_attributes() & 0x400 != 0
        || metadata.len() == 0
        || metadata.len() > 1024 * 1024
    {
        return Err("Fixture executable must be a bounded ordinary compiled file.".into());
    }
    fs::read(path).map_err(|error| error.to_string())
}
