use std::collections::BTreeMap;
use std::io::{BufRead, Read, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use app_core::{LaunchPlan, ProcessHostSurface, ProcessWindowPolicy};
use app_runtime::SpawnedProcess;
use serde_json::json;

use super::config::{Config, MARKER, Scenario, checked_path, read_json, write_new};

pub(super) struct OwnedChild(pub SpawnedProcess);

impl OwnedChild {
    pub fn spawn(
        config: &Config,
        role: &str,
        config_path: &std::path::Path,
    ) -> Result<Self, String> {
        let root = config.root.to_string_lossy().into_owned();
        let plan = LaunchPlan {
            instance_id: role.into(),
            instance_name: role.into(),
            module_id: "runtime-fixture".into(),
            install_root: root.clone(),
            install_state: app_core::InstallState::Installed,
            uses_private_runtime: false,
            working_directory: root,
            executable_path: std::env::current_exe()
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .into_owned(),
            executable_exists: true,
            ready_to_launch: true,
            validation_issues: Vec::new(),
            args: vec![role.into(), config_path.to_string_lossy().into_owned()],
            environment: BTreeMap::new(),
            command_line: String::new(),
            window_policy: ProcessWindowPolicy::Background,
            uses_script_entrypoint: false,
            requires_admin: false,
            host_surface: ProcessHostSurface::ManagedTerminal,
            host_notes: None,
            performance_policy: Default::default(),
            performance_preview: Default::default(),
        };
        let log = config
            .root
            .join("logs")
            .join(format!("{}.log", role.trim_start_matches('-')));
        app_runtime::spawn_launch_plan(&plan, &log)
            .map(Self)
            .map_err(|e| e.to_string())
    }

    pub fn wait(&mut self, seconds: u64) -> Result<(), String> {
        let deadline = Instant::now() + Duration::from_secs(seconds);
        loop {
            let status = self
                .0
                .child
                .as_mut()
                .ok_or("Fixture lost its child owner")?
                .try_wait()
                .map_err(|e| e.to_string())?;
            if let Some(status) = status {
                return if status.success() {
                    Ok(())
                } else {
                    Err(format!("Fixture child {} exited {status}", self.0.pid))
                };
            }
            if Instant::now() >= deadline {
                return Err(format!("Fixture child {} exceeded {seconds}s", self.0.pid));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    pub fn finish(&mut self) -> Result<(), String> {
        app_runtime::stop_spawned_process(&mut self.0).map_err(|e| e.to_string())?;
        // Closing the completed owner also joins its captured output reader.
        self.0.child.take();
        Ok(())
    }
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        // The owner is retained until every descendant has completed; dropping
        // a UI-client owner cannot close the independently owned service Job.
        if self.0.child.is_some()
            && let Err(error) = self.finish()
        {
            eprintln!("Runtime fixture child cleanup failed: {error}");
        }
    }
}

pub(super) fn run_game_if_requested() -> Option<Result<(), String>> {
    let executable = match std::env::current_exe() {
        Ok(path) => path,
        Err(_) => return None,
    };
    if executable.file_name().and_then(|name| name.to_str()) != Some("java.exe") {
        return None;
    }
    let root = std::env::var_os("LANGAME_RUNTIME_SERVICE_FIXTURE_ROOT")?;
    let nonce = std::env::var("LANGAME_RUNTIME_SERVICE_FIXTURE_NONCE").ok()?;
    Some((|| {
        let root = PathBuf::from(root);
        let config: Config = read_json(&root.join(MARKER))?;
        if config.root != root || config.nonce != nonce {
            return Err("Synthetic game namespace does not match its marker".into());
        }
        run_game(config, executable)
    })())
}

fn run_game(config: Config, executable: PathBuf) -> Result<(), String> {
    config.verify()?;
    checked_path(&executable)?;
    if !executable.starts_with(config.root.join("runtime/instances")) {
        return Err("Synthetic game must use an instance-owned fixture runtime".into());
    }
    let args: Vec<String> = std::env::args().collect();
    let arg = |name: &str| -> Result<&str, String> {
        args.iter()
            .position(|item| item == name)
            .and_then(|index| args.get(index + 1))
            .map(String::as_str)
            .ok_or_else(|| format!("Synthetic game missing {name}"))
    };
    let ip: std::net::IpAddr = arg("-ip")?
        .parse()
        .map_err(|e: std::net::AddrParseError| e.to_string())?;
    if !ip.is_loopback() {
        return Err("Synthetic game requires loopback bind".into());
    }
    let port: u16 = arg("-port")?
        .parse()
        .map_err(|e: std::num::ParseIntError| e.to_string())?;
    let _socket = std::net::UdpSocket::bind((ip, port)).map_err(|e| e.to_string())?;
    let data = PathBuf::from(arg("-datadir")?);
    checked_path(&data)?;
    if !data.starts_with(config.root.join("runtime/instances")) {
        return Err("Synthetic game data escaped fixture".into());
    }
    std::fs::create_dir_all(data.join("saves")).map_err(|e| e.to_string())?;
    write_new(
        &config.root.join("data/game-ready.json"),
        &json!({"pid":std::process::id(),"port":port,"data":data}),
    )?;
    let mut output = std::io::stdout().lock();
    writeln!(output, "LGSM_SYNTHETIC_GAME_READY")
        .and_then(|()| output.flush())
        .map_err(|e| e.to_string())?;
    let mut input = std::io::stdin().lock();
    let mut saved = false;
    let mut save_commands = 0;
    loop {
        let mut line = Vec::new();
        let count = Read::by_ref(&mut input)
            .take(256)
            .read_until(b'\n', &mut line)
            .map_err(|e| e.to_string())?;
        if count == 0 {
            return Err("Synthetic game stdin closed before managed stop".into());
        }
        if line.len() >= 256 {
            return Err("Synthetic game command exceeds limit".into());
        }
        let text = String::from_utf8(line).map_err(|e| e.to_string())?;
        match text.trim() {
            "save" => {
                save_commands += 1;
                if saved || save_commands != 1 {
                    return Err("Final exit sent the synthetic save command more than once".into());
                }
                let request = config.root.join("data/game-save-requested.json");
                if !request.exists() {
                    write_new(
                        &request,
                        &json!({"pid":std::process::id(),"nonce":config.nonce,"received":true,"save_commands":save_commands}),
                    )?;
                    // Publish the signal only after the JSON writer has flushed
                    // and closed; an observer must not read a partial receipt.
                    std::fs::File::options()
                        .write(true)
                        .create_new(true)
                        .open(config.root.join("data/game-save-requested.ready"))
                        .map_err(|e| e.to_string())?;
                }
                if config.scenario == Scenario::TrayExitHangSave {
                    // The request reached the real stdin boundary, but native
                    // saving never completes. Only the owner's deadline ends us.
                    loop {
                        std::thread::sleep(Duration::from_secs(1));
                    }
                }
                if config.scenario == Scenario::TrayExit {
                    // The independent owner releases this only after observing
                    // interface process exit. No timing guess can prove decoupling.
                    let deadline = Instant::now() + Duration::from_secs(10);
                    while !config.root.join("data/allow-game-save.ready").exists() {
                        if Instant::now() >= deadline {
                            return Err(
                                "Interface did not exit before the synthetic save barrier".into()
                            );
                        }
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    let release: serde_json::Value =
                        read_json(&config.root.join("data/allow-game-save.json"))?;
                    if release["nonce"] != config.nonce || release["client_exited"] != true {
                        return Err("Invalid synthetic save release evidence".into());
                    }
                }
                write_new(
                    &data.join("saves/fixture-world.json"),
                    &json!({"nonce":config.nonce,"saved":true}),
                )?;
                write_new(
                    &config.root.join("data/game-saved.json"),
                    &json!({"pid":std::process::id(),"saved":true,"save_commands":save_commands}),
                )?;
                saved = true;
                writeln!(output, "LGSM_SYNTHETIC_GAME_SAVED")
                    .and_then(|()| output.flush())
                    .map_err(|e| e.to_string())?;
            }
            "stop" if saved => {
                write_new(
                    &config.root.join("data/game-stopped.json"),
                    &json!({"pid":std::process::id(),"saved_before_stop":true,"save_commands":save_commands}),
                )?;
                return Ok(());
            }
            _ => {
                return Err(format!(
                    "Unexpected synthetic game command: {}",
                    text.trim()
                ));
            }
        }
    }
}
