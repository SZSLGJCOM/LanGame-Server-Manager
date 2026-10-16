use super::*;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::os::windows::fs::MetadataExt;

pub(super) type ProbeResult<T> = Result<T, Box<dyn std::error::Error>>;
pub(super) type Files = BTreeMap<PathBuf, Vec<u8>>;

pub(super) struct OwnedNative {
    pub(super) process: Option<app_runtime::SpawnedProcess>,
    pub(super) cleanup_allowed: Arc<AtomicBool>,
}

impl Drop for OwnedNative {
    fn drop(&mut self) {
        if let Some(mut process) = self.process.take() {
            // Failure recovery owns only this disposable launch's Job tree.
            // It never counts as the successful graceful shutdown assertion.
            let stopped = app_runtime::stop_spawned_process(&mut process).is_ok()
                && app_runtime::process_matches_identity(process.pid, &process.process_identity)
                    .is_ok_and(|running| !running);
            self.cleanup_allowed
                .store(stopped && !std::thread::panicking(), Ordering::SeqCst);
            eprintln!(
                "ENSHROUDED_COMPATIBILITY failure_recovery=true owned_tree_stopped={stopped}"
            );
        }
        // A failed assertion must retain native logs and the disposable world,
        // including failures after a successful stop already released the Job.
        if std::thread::panicking() {
            self.cleanup_allowed.store(false, Ordering::SeqCst);
        }
    }
}

#[test]
fn failed_native_assertion_retains_its_disposable_evidence() {
    let cleanup = Arc::new(AtomicBool::new(true));
    let observed = Arc::clone(&cleanup);
    let failed = std::panic::catch_unwind(move || {
        let _owned = OwnedNative {
            process: None,
            cleanup_allowed: observed,
        };
        panic!("synthetic failed native assertion");
    });
    assert!(failed.is_err());
    assert!(!cleanup.load(Ordering::SeqCst));
}

async fn native_network(
    process: &app_runtime::SpawnedProcess,
) -> ProbeResult<app_platform_win::ProcessNetworkInspectionResult> {
    if !app_runtime::process_matches_identity(process.pid, &process.process_identity)? {
        return Err("owned native server exited before network observation".into());
    }
    let target = app_platform_win::WindowInspectionTarget {
        pid: process.pid,
        process_key: "main".into(),
        display_name: "Isolated Enshrouded native probe".into(),
        process_identity: process.process_identity.clone(),
    };
    Ok(tokio::task::spawn_blocking(move || {
        WindowsPlatform::inspect_process_network_endpoints(&[target], &[])
    })
    .await??)
}

fn check_network_scope(
    network: &app_platform_win::ProcessNetworkInspectionResult,
    query_port: u16,
    allow_native_udp: bool,
    native_udp_ports: &mut BTreeSet<u16>,
) -> ProbeResult<()> {
    if network.inspected_process_count == 0 || network.endpoints.len() > 64 {
        return Err("native endpoint ownership is incomplete or exceeds its bound".into());
    }
    for endpoint in &network.endpoints {
        if endpoint.local_address == "127.0.0.1" || endpoint.local_address == "::1" {
            continue;
        }
        // Explicit per-run acknowledgement permits only the two observed
        // native UDP transport ports. The game query and all TCP listeners
        // remain loopback; this does not change the system firewall.
        if !allow_native_udp
            || endpoint.protocol != "udp"
            || endpoint.local_port == query_port
            || !matches!(endpoint.local_address.as_str(), "0.0.0.0" | "::")
        {
            return Err("native endpoint exceeds the acknowledged UDP scope".into());
        }
        native_udp_ports.insert(endpoint.local_port);
    }
    if native_udp_ports.len() > 2 {
        return Err("native transport opened more than two UDP ports during this run".into());
    }
    Ok(())
}

#[test]
fn acknowledged_native_udp_preserves_query_and_tcp_loopback_limits() {
    use app_platform_win::{ProcessNetworkEndpoint, ProcessNetworkInspectionResult};
    let endpoint = |protocol: &str, address: &str, port| ProcessNetworkEndpoint {
        protocol: protocol.into(),
        local_address: address.into(),
        local_port: port,
        owning_pid: 42,
        process_key: "main".into(),
        relation: "tracked_process".into(),
    };
    let mut network = ProcessNetworkInspectionResult {
        inspected_process_count: 1,
        endpoints: vec![
            endpoint("udp", "127.0.0.1", 50000),
            endpoint("udp", "0.0.0.0", 51000),
            endpoint("udp", "::", 51000),
            endpoint("udp", "0.0.0.0", 51001),
        ],
    };
    assert!(check_network_scope(&network, 50000, false, &mut BTreeSet::new()).is_err());
    let mut ports = BTreeSet::new();
    check_network_scope(&network, 50000, true, &mut ports).unwrap();
    assert_eq!(ports.len(), 2);
    for forbidden in [
        endpoint("tcp", "0.0.0.0", 51000),
        endpoint("udp", "::", 50000),
        endpoint("udp", "0.0.0.0", 51002),
    ] {
        network.endpoints.push(forbidden);
        assert!(check_network_scope(&network, 50000, true, &mut ports.clone()).is_err());
        network.endpoints.pop();
    }
    network.endpoints = vec![endpoint("udp", "0.0.0.0", 52000)];
    assert!(
        check_network_scope(&network, 50000, true, &mut ports).is_err(),
        "a run cannot rotate through unbounded temporary ports"
    );
}

pub(super) struct NativeObservation<'a> {
    pub(super) query_port: u16,
    pub(super) evidence: &'a Path,
    pub(super) phase: &'a str,
    pub(super) observation_seconds: u64,
    pub(super) allow_native_udp: bool,
    pub(super) log_path: &'a Path,
    pub(super) expected_flame_level: u64,
    pub(super) world_path: &'a Path,
}

pub(super) async fn observe_native(
    process: &app_runtime::SpawnedProcess,
    observation: NativeObservation<'_>,
) -> ProbeResult<Vec<u8>> {
    let NativeObservation {
        query_port: port,
        evidence,
        phase,
        observation_seconds,
        allow_native_udp,
        log_path,
        expected_flame_level,
        world_path,
    } = observation;
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut ready_since = None;
    let mut native_udp_ports = BTreeSet::new();
    loop {
        let network = native_network(process).await?;
        if let Err(error) =
            check_network_scope(&network, port, allow_native_udp, &mut native_udp_ports)
        {
            write_new(
                &evidence.join(format!("{phase}-network-rejected.json")),
                &serde_json::to_vec_pretty(&network)?,
            )?;
            return Err(error);
        }
        let owned_query = network.endpoints.iter().any(|endpoint| {
            endpoint.protocol == "udp"
                && endpoint.local_port == port
                && endpoint.local_address == "127.0.0.1"
        });
        let query = if owned_query {
            tokio::task::spawn_blocking(move || {
                app_storage::query_a2s_player_count(([127, 0, 0, 1], port).into())
            })
            .await?
            .is_ok()
        } else {
            false
        };
        let log = fs::read_to_string(log_path)?;
        if query && check_native_world(&log, expected_flame_level)? {
            let stable = ready_since.get_or_insert_with(Instant::now);
            if stable.elapsed() >= Duration::from_secs(3) {
                write_new(
                    &evidence.join(format!("{phase}-network-ready.json")),
                    &serde_json::to_vec_pretty(&network)?,
                )?;
                break;
            }
        } else {
            ready_since = None;
        }
        if Instant::now() >= deadline {
            return Err(
                "owned loopback A2S and native world did not become ready within 120 seconds"
                    .into(),
            );
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    println!(
        "ENSHROUDED_COMPATIBILITY phase={phase} action=ready pid={} scope=native_loopback_probe",
        process.pid
    );
    let ready_world = fs::read(world_path)?;
    let observed_until = Instant::now() + Duration::from_secs(observation_seconds);
    let mut observations = 0;
    while Instant::now() < observed_until {
        tokio::time::sleep(
            observed_until
                .saturating_duration_since(Instant::now())
                .min(Duration::from_secs(5)),
        )
        .await;
        let network = native_network(process).await?;
        if let Err(error) =
            check_network_scope(&network, port, allow_native_udp, &mut native_udp_ports)
        {
            write_new(
                &evidence.join(format!("{phase}-network-observation-rejected.json")),
                &serde_json::to_vec_pretty(&network)?,
            )?;
            return Err(error);
        }
        if !check_native_world(&fs::read_to_string(log_path)?, expected_flame_level)? {
            return Err("native world readiness disappeared during observation".into());
        }
        observations += 1;
    }
    println!(
        "ENSHROUDED_COMPATIBILITY phase={phase} action=observed seconds={observation_seconds} loopback_checks={observations}"
    );
    Ok(ready_world)
}

pub(super) fn plain_existing(path: &Path) -> ProbeResult<PathBuf> {
    if !path.is_absolute()
        || path.components().any(|part| {
            matches!(
                part,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        })
    {
        return Err("compatibility inputs must be absolute normalized paths".into());
    }
    for ancestor in path.ancestors() {
        let metadata = fs::symlink_metadata(ancestor)?;
        if metadata.file_type().is_symlink() || metadata.file_attributes() & 0x400 != 0 {
            return Err("compatibility inputs cannot cross links or reparse points".into());
        }
    }
    Ok(path.canonicalize()?)
}

pub(super) fn input(name: &str) -> ProbeResult<PathBuf> {
    plain_existing(&PathBuf::from(
        std::env::var_os(name).ok_or_else(|| format!("supply {name}"))?,
    ))
}

pub(super) fn read_files(root: &Path) -> ProbeResult<Files> {
    let inventory = save_inventory(root)?;
    if inventory.len() > 512
        || inventory.values().map(|(bytes, _)| bytes).sum::<u64>() > 256 * 1024 * 1024
    {
        return Err("compatibility evidence exceeds 512 entries / 256 MiB".into());
    }
    inventory
        .into_iter()
        .map(|(relative, (length, _))| {
            let mut bytes = Vec::new();
            fs::File::open(root.join(&relative))?
                .take(length + 1)
                .read_to_end(&mut bytes)?;
            if bytes.len() as u64 != length {
                return Err("compatibility file changed while reading".into());
            }
            Ok((relative, bytes))
        })
        .collect()
}

pub(super) fn write_new(path: &Path, bytes: &[u8]) -> ProbeResult<()> {
    fs::create_dir_all(path.parent().ok_or("evidence file has no parent")?)?;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?
        .write_all(bytes)?;
    Ok(())
}

pub(super) fn write_files(root: &Path, files: &Files) -> ProbeResult<()> {
    fs::create_dir(root)?;
    for (relative, bytes) in files {
        if relative.as_os_str().is_empty()
            || relative
                .components()
                .any(|part| !matches!(part, std::path::Component::Normal(_)))
        {
            return Err("evidence relative path is not a contained file".into());
        }
        write_new(&root.join(relative), bytes)?;
    }
    assert_eq!(
        &read_files(root)?,
        files,
        "evidence copy must retain exact bytes"
    );
    Ok(())
}

fn redact_log(bytes: &[u8], secrets: &[String]) -> Vec<u8> {
    let mut text = String::from_utf8_lossy(bytes).into_owned();
    for secret in secrets {
        text = text.replace(secret, "[redacted]");
    }
    text.lines()
        .map(|line| {
            let lower = line.to_ascii_lowercase();
            if [
                "password",
                "secret",
                "token",
                "authorization",
                "settings_json",
            ]
            .iter()
            .any(|key| lower.contains(key))
            {
                "[sensitive native log line omitted]"
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        .into_bytes()
}

pub(super) fn capture(
    evidence: &Path,
    phase: &str,
    details: &InstanceDetails,
    runtime_root: &Path,
    fixture: &Path,
    secrets: &[String],
) -> ProbeResult<()> {
    let target = evidence.join(phase);
    fs::create_dir(&target)?;
    let saves = seed_probe::checked_path(fixture, Path::new(&details.saves_path), true)?;
    write_files(&target.join("saves"), &read_files(&saves)?)?;
    for (name, path) in [
        (
            "managed-native-config.json",
            PathBuf::from(&details.config_file_path),
        ),
        (
            "runtime-native-config.json",
            runtime_root.join("enshrouded_server.json"),
        ),
    ] {
        if path.is_file() {
            let path = seed_probe::checked_path(fixture, &path, false)?;
            write_new(&target.join(name), &fs::read(path)?)?;
        }
    }
    let logs = creation::instance_root(details)?.join("logs");
    if logs.is_dir() {
        let logs = seed_probe::checked_path(fixture, &logs, true)?;
        let redacted = read_files(&logs)?
            .into_iter()
            .map(|(path, bytes)| (path, redact_log(&bytes, secrets)))
            .collect();
        write_files(&target.join("logs"), &redacted)?;
    }
    println!(
        "ENSHROUDED_COMPATIBILITY phase={phase} evidence={}",
        target.display()
    );
    Ok(())
}

// Read the effective native settings rather than the input JSON, which the
// native server can retain even when it clamps an out-of-range setting.
pub(super) fn check_native_world(log: &str, expected_flame_level: u64) -> ProbeResult<bool> {
    let lower = log.to_ascii_lowercase();
    for failure in [
        "[server] failed to load server save game",
        "[server] failed to save",
        "[save2] failed to migrate",
        "[savedata2] invalid container checksum",
    ] {
        if lower.contains(failure) {
            return Err(format!("native world reported {failure}").into());
        }
    }
    if !log.contains("[server] Savegame deserialization took ")
        || !log.contains("[game_server] Switching state from LoadEcsScene to Run")
    {
        return Ok(false);
    }
    let settings = log
        .split_once("[server] Game Settings 'Custom'")
        .ok_or("native world did not apply Custom settings")?
        .1
        .trim_start();
    let effective: Value = serde_json::Deserializer::from_str(settings)
        .into_iter::<Value>()
        .next()
        .ok_or("missing native effective settings")??;
    if effective["flameLevelFogRemoval"] != expected_flame_level
        || effective["randomSpawnerAmount"] != "Automatic"
        || effective["aggroPoolAmount"] != "Normal"
    {
        return Err(
            "native effective settings differ from the requested Flame / encounter settings".into(),
        );
    }
    Ok(true)
}

#[test]
fn world_readiness_requires_deserialization_run_and_effective_settings() {
    let config = "[server] Game Settings 'Custom'\n{\"flameLevelFogRemoval\":1,\"randomSpawnerAmount\":\"Automatic\",\"aggroPoolAmount\":\"Normal\"}\n";
    let loaded = format!("{config}[server] Savegame deserialization took 10 ms\n");
    assert!(!check_native_world("Host_Online", 1).unwrap());
    assert!(!check_native_world(&loaded, 1).unwrap());
    let running =
        format!("{loaded}[game_server] Switching state from LoadEcsScene to Run after 1 ms");
    assert!(check_native_world(&running, 1).unwrap());
    assert!(
        check_native_world(&running, 0).is_err(),
        "input 0 clamped to 1 is not effective 0"
    );
    assert!(
        check_native_world(
            &format!("{running}\n[server] Failed to load Server Save Game"),
            1
        )
        .is_err()
    );
}

/// The acceptance result belongs only to this exact acquired hotfix package.
pub(super) fn hotfix_package_identity(source: &Path) -> ProbeResult<Value> {
    use sha2::{Digest, Sha256};
    let manifest = fs::read_to_string(source.join("steamapps/appmanifest_2278520.acf"))?;
    let has_pair = |key: &str, expected: &str| {
        manifest.lines().any(|line| {
            let parts = line.split_whitespace().collect::<Vec<_>>();
            parts.as_slice() == [format!("\"{key}\""), format!("\"{expected}\"")]
        })
    };
    if !has_pair("appid", "2278520")
        || !has_pair("buildid", "25967645")
        || !has_pair("TargetBuildID", "25967645")
        || !has_pair("StateFlags", "4")
        || !has_pair("UpdateResult", "0")
        || !manifest.contains("\"2278521\"")
        || !has_pair("manifest", "3715394091293008757")
    {
        return Err("expected validated official 1.0.0.1 build/depot manifest".into());
    }
    let exe = plain_existing(&source.join("enshrouded_server.exe"))?;
    let sha256 = Sha256::digest(fs::read(exe)?)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if sha256 != "4de2acdd3b18e562f129bc488a2daea2da16b00341497304423e1e18c49a83ba" {
        return Err("1.0.0.1 executable differs from the acquired official package".into());
    }
    Ok(
        json!({"version": "1.0.0.1", "steam_app": 2278520, "steam_build": 25967645,
        "depot": 2278521, "depot_manifest": "3715394091293008757", "exe_sha256": sha256}),
    )
}
