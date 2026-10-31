//! Two owned Satisfactory worlds; no clients, Mods, downloads or shared profiles.
//! Select native_satisfactory_instance_isolation with local_package,
//! independent_pair and both manual-save flags. Keep FRM in its separate
//! single-instance acceptance. Three program copies need about 44 GiB scratch.
use std::collections::{BTreeMap, BTreeSet};

use crate::commands::commands_satisfactory_world::native_world::isolation::Snapshot;

use super::*;

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
type Files = BTreeMap<PathBuf, (u64, u64)>;

pub(super) fn validate(
    module_id: &str,
    creation: &creation::CreationMode,
    observation_seconds: u64,
) -> Result {
    if module_id != "satisfactory"
        || creation.acquisition != creation::Acquisition::LocalPackage
        || creation.instances != creation::InstanceMode::IndependentPair
        || observation_seconds != 0
        || std::env::var("LANGAME_NATIVE_MANUAL_SAVE").as_deref() != Ok("true")
        || std::env::var("LANGAME_NATIVE_MANUAL_SAVE_RESTART").as_deref() != Ok("true")
        || std::env::var("LANGAME_NATIVE_SATISFACTORY_FRM").as_deref() == Ok("true")
        || std::env::var("LANGAME_NATIVE_EXACT_SAVE_READBACK").as_deref() == Ok("true")
        || std::env::var_os("LANGAME_NATIVE_PROGRAM_MAINTENANCE").is_some()
        || std::env::var_os("LANGAME_NATIVE_SETTINGS_JSON").is_some()
    {
        return Err("Satisfactory isolation requires a local independent_pair, manual save and restart, default native settings, no diagnostic delay, FRM, checkpoint profile or program maintenance.".into());
    }
    Ok(())
}

pub(super) async fn run<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    primary: &InstanceDetails,
    peer: &InstanceDetails,
    peer_world: &NativeWorld,
    cleanup: &mut NativeCleanup<'_>,
) -> Result {
    let primary_world = runtime
        .satisfactory_world
        .ok_or("Satisfactory isolation requires its primary native world fixture")?;
    let peer = app_storage::update_instance(
        &runtime.storage.paths,
        UpdateInstanceInput {
            id: peer.summary.id.clone(),
            bind_ip: primary.summary.bind_ip.clone(),
            auto_backup_on_stop: false,
            backup_retention_count: 2,
            settings_json: serde_json::to_string(runtime.settings)?,
            ports: peer.ports.clone(),
        },
    )
    .await?;
    multi_instance::verify_isolation(primary, &peer)?;
    let peer_root = creation::program_root(&peer)?;
    let peer_runtime = runtime::NativeRuntime {
        app: runtime.app,
        state: runtime.state,
        storage: runtime.storage,
        descriptor: runtime.descriptor,
        smoke: runtime.smoke,
        package: runtime.package,
        settings: runtime.settings,
        effective_install_root: &peer_root,
        offline_dst: false,
        elevated_fixture: false,
        satisfactory_world: Some(peer_world),
        satisfactory_frm: None,
    };

    let peer_running = start(&peer_runtime, &peer, "initial", cleanup).await?;
    prepare_save(&peer_runtime, peer_world, &peer_running, "initial").await?;
    let peer_snapshot = Capture::read(&peer_runtime, peer_world, &peer_running).await?;
    let mut primary_stopped = primary.clone();
    let mut primary_run = None;
    let mut primary_snapshot: Option<Capture> = None;
    for cycle in ["initial", "restart"] {
        let running = start(runtime, &primary_stopped, cycle, cleanup).await?;
        let run_id = running
            .active_run
            .as_ref()
            .ok_or("Satisfactory primary has no active run")?
            .run_id;
        if primary_run == Some(run_id) {
            return Err("Satisfactory primary restart reused the earlier run identity".into());
        }
        primary_run = Some(run_id);
        prepare_save(runtime, primary_world, &running, cycle).await?;
        primary_world
            .verify_isolated_credentials(runtime.state, &running, peer_world, &peer_running)
            .await?;
        let captured = Capture::read(runtime, primary_world, &running).await?;
        if let Some(previous) = &primary_snapshot {
            previous.verify_retained_checkpoints(&captured)?;
        }
        if captured.world.session_name == peer_snapshot.world.session_name
            || !captured
                .world
                .manual_saves
                .is_disjoint(&peer_snapshot.world.manual_saves)
        {
            return Err("Satisfactory instances exposed another world's save identity".into());
        }
        peer_snapshot
            .verify_live(&peer_runtime, peer_world, &peer_running)
            .await?;
        primary_stopped = stop(runtime, &running).await?;
        peer_snapshot
            .verify_live(&peer_runtime, peer_world, &peer_running)
            .await?;
        // Native INI writes during this instance's own normal shutdown are
        // legitimate. Establish its stopped-file baseline only afterwards.
        primary_snapshot = Some(captured.after_stop(&primary_stopped)?);
        println!(
            "NATIVE_SATISFACTORY_ISOLATION phase=primary_cycle cycle={cycle} worlds=distinct peer_run=unchanged peer_https_world=unchanged peer_manual_save_family=unchanged credentials=distinct cross_token_reads=rejected normal_stop=passed"
        );
    }

    let primary_snapshot = primary_snapshot.ok_or("Satisfactory primary snapshot is absent")?;
    let peer_stopped = stop(&peer_runtime, &peer_running).await?;
    let peer_stopped_snapshot = peer_snapshot.after_stop(&peer_stopped)?;
    primary_snapshot
        .verify_stopped(runtime, primary_world, &primary_stopped)
        .await?;
    let peer_restarted = start(&peer_runtime, &peer_stopped, "restart", cleanup).await?;
    if peer_restarted.active_run.as_ref().map(|run| run.run_id)
        == peer_running.active_run.as_ref().map(|run| run.run_id)
    {
        return Err("Satisfactory peer restart reused its earlier run identity".into());
    }
    prepare_save(&peer_runtime, peer_world, &peer_restarted, "restart").await?;
    let restarted = Capture::read(&peer_runtime, peer_world, &peer_restarted).await?;
    peer_stopped_snapshot.verify_retained_checkpoints(&restarted)?;
    primary_snapshot
        .verify_stopped(runtime, primary_world, &primary_stopped)
        .await?;
    let peer_stopped = stop(&peer_runtime, &peer_restarted).await?;
    primary_snapshot
        .verify_stopped(runtime, primary_world, &primary_stopped)
        .await?;
    multi_instance::verify_isolation(&primary_stopped, &peer_stopped)?;
    runtime
        .package
        .cleanup_allowed
        .store(true, Ordering::SeqCst);
    println!(
        "NATIVE_SATISFACTORY_ISOLATION phase=worlds_verified instances=2 cycles_each=2 primary_stopped_data=unchanged peer_restart=own_checkpoint_reload normal_stops=4 native_clients=0 player_content_readback=not_claimed"
    );
    Ok(())
}

async fn start<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    details: &InstanceDetails,
    cycle: &str,
    cleanup: &mut NativeCleanup<'_>,
) -> Result<InstanceDetails> {
    manual_save_probe::before_start(runtime, details, cycle).await?;
    // Revoke the never-started proof before any await that may spawn a child.
    cleanup.mark_start_attempted(&details.summary.id)?;
    let running = Box::pin(multi_instance::start_owned(runtime, details, cycle)).await?;
    let run = running
        .active_run
        .as_ref()
        .ok_or("Satisfactory has no owned run")?;
    multi_instance::verify_peer_running(runtime, &running, run).await?;
    let process = run
        .processes
        .iter()
        .find(|process| process.is_primary)
        .and_then(|process| process.process_identity.as_ref())
        .ok_or("Satisfactory has no primary native process identity")?;
    if !Path::new(&process.image_path)
        .canonicalize()?
        .starts_with(runtime.effective_install_root.canonicalize()?)
    {
        return Err(
            "Satisfactory isolation started outside this instance's private program".into(),
        );
    }
    Ok(running)
}

async fn prepare_save<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    world: &NativeWorld,
    running: &InstanceDetails,
    cycle: &str,
) -> Result {
    Box::pin(world.prepare(runtime.state, running, cycle)).await?;
    Box::pin(manual_save_probe::verify(runtime, running, cycle)).await?;
    Ok(())
}

async fn stop<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    running: &InstanceDetails,
) -> Result<InstanceDetails> {
    let result = Box::pin(multi_instance::stop_owned(runtime, running)).await;
    // One stopped tree does not authorize deleting a package with a live peer.
    runtime
        .package
        .cleanup_allowed
        .store(false, Ordering::SeqCst);
    result
}

struct Capture {
    world: Snapshot,
    files: Files,
}

impl Capture {
    async fn read<R: tauri::Runtime>(
        runtime: &runtime::NativeRuntime<'_, '_, R>,
        world: &NativeWorld,
        running: &InstanceDetails,
    ) -> Result<Self> {
        let observed = world.isolation_snapshot(runtime.state, running).await?;
        let files = stable_files(running, &observed.manual_saves)?;
        Ok(Self {
            world: observed,
            files,
        })
    }

    fn after_stop(mut self, stopped: &InstanceDetails) -> Result<Self> {
        self.files = stable_files(stopped, &self.world.manual_saves)?;
        Ok(self)
    }

    fn verify_retained_checkpoints(&self, current: &Self) -> Result {
        if current.world.session_name != self.world.session_name
            || !current
                .world
                .manual_saves
                .is_superset(&self.world.manual_saves)
            || current.world.credential_fingerprints != self.world.credential_fingerprints
            || self.files.iter().any(|(path, value)| {
                belongs_to_save_family(path, &self.world.manual_saves)
                    && current.files.get(path) != Some(value)
            })
        {
            return Err("Satisfactory restart did not retain its own world, manual-save files and credentials".into());
        }
        Ok(())
    }

    async fn verify_live<R: tauri::Runtime>(
        &self,
        runtime: &runtime::NativeRuntime<'_, '_, R>,
        world: &NativeWorld,
        running: &InstanceDetails,
    ) -> Result {
        let expected = running
            .active_run
            .as_ref()
            .ok_or("Satisfactory peer run is absent")?;
        multi_instance::verify_peer_running(runtime, running, expected).await?;
        let current = Self::read(runtime, world, running).await?;
        if self.world != current.world || self.files != current.files {
            return Err("Satisfactory peer world, manual-save family, stable INI or credentials changed during the other instance's cycle".into());
        }
        multi_instance::verify_peer_running(runtime, running, expected).await?;
        Ok(())
    }

    async fn verify_stopped<R: tauri::Runtime>(
        &self,
        runtime: &runtime::NativeRuntime<'_, '_, R>,
        world: &NativeWorld,
        stopped: &InstanceDetails,
    ) -> Result {
        let current = read_instance_details(&runtime.storage.paths, &stopped.summary.id).await?;
        if current.active_run.is_some()
            || runtime
                .state
                .runtime_supervisor
                .lock()
                .map_err(|_| "runtime supervisor lock poisoned")?
                .is_tracked(&stopped.summary.id)
            || stable_files(&current, &self.world.manual_saves)? != self.files
            || world.isolation_credential_fingerprints().await?
                != self.world.credential_fingerprints
        {
            return Err("Satisfactory stopped peer data or credentials changed during the other instance's restart".into());
        }
        Ok(())
    }
}

fn stable_files(instance: &InstanceDetails, names: &BTreeSet<String>) -> Result<Files> {
    let root = creation::instance_root(instance)?;
    let mut files = Files::new();
    for path in std::iter::once(PathBuf::from(&instance.config_file_path)).chain(
        ["Engine.ini", "Game.ini", "GameUserSettings.ini"]
            .map(|name| root.join("data/Saved/Config/WindowsServer").join(name)),
    ) {
        if !path.is_file() {
            return Err(
                "Satisfactory isolation is missing a managed or native configuration file".into(),
            );
        }
        for (_, value) in save_inventory(&path)? {
            files.insert(path.clone(), value);
        }
    }
    let saves = Path::new(&instance.saves_path);
    let inventory = save_inventory(saves)?;
    for name in names {
        let filename = format!("{name}.sav");
        if !inventory.keys().any(|path| {
            path.file_name().and_then(|value| value.to_str()) == Some(filename.as_str())
        }) {
            return Err(
                "Satisfactory confirmed checkpoint has no corresponding native .sav file".into(),
            );
        }
    }
    // Official autosaves use SessionName_autosave_N.sav, while these explicit
    // API checkpoints have unique LGSM_manual_save_* names. Compare only the
    // latter families; do not impose an autosave deadline on long native starts.
    // Logs, certificates and session-authentication files have other owners.
    // https://satisfactory.wiki.gg/wiki/Dedicated_servers/Configuration_files#Number_of_Kept_Autosaves
    for (path, value) in inventory {
        if belongs_to_save_family(&path, names) {
            files.insert(saves.join(path), value);
        }
    }
    Ok(files)
}

fn belongs_to_save_family(path: &Path, names: &BTreeSet<String>) -> bool {
    path.file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|file| {
            names
                .iter()
                .any(|name| file == name || file.starts_with(&format!("{name}.")))
        })
}

#[test]
fn manual_save_family_excludes_prefix_collisions_and_automatic_saves() {
    let names = BTreeSet::from(["LGSM_manual_save_one".into()]);
    for path in [
        "server/LGSM_manual_save_one.sav",
        "server/LGSM_manual_save_one.sav.bak",
    ] {
        assert!(belongs_to_save_family(Path::new(path), &names));
    }
    for path in [
        "LGSM_manual_save_one_other.sav",
        "World_autosave_0.sav",
        "ServerSettings.15777.sav",
    ] {
        assert!(!belongs_to_save_family(Path::new(path), &names));
    }
}
