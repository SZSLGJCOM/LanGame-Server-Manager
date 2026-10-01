use super::*;

pub(super) type LiveResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
pub(super) const DEPENDENCY_GLOBAL: &str = "LGSM_ASSISTANT_DEPENDENCY_LOADED";
pub(super) const CONSUMER_GLOBAL: &str = "LGSM_ASSISTANT_CONSUMER_LOADED";
pub(super) const MISSING_DEPENDENCY_MARKER: &str = "LGSM_ASSISTANT_MISSING_DEPENDENCY";

#[derive(Clone, Copy, Debug)]
pub(super) enum LiveFault {
    WorldConfiguration,
    MissingModDependency,
}

impl LiveFault {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::WorldConfiguration => "world-config",
            Self::MissingModDependency => "mod-dependency",
        }
    }

    pub(super) fn setting_key(self) -> &'static str {
        match self {
            Self::WorldConfiguration => "master_worldgenoverride_lua",
            Self::MissingModDependency => "master_modoverrides_lua",
        }
    }

    pub(super) fn failure_marker(self) -> &'static str {
        match self {
            Self::WorldConfiguration => "Failed to load ../worldgenoverride.lua",
            Self::MissingModDependency => MISSING_DEPENDENCY_MARKER,
        }
    }
}

pub(super) struct LiveEnvironment {
    pub(super) games_root: PathBuf,
    pub(super) install_root: PathBuf,
    pub(super) provider: AssistantProviderSettings,
}

impl LiveEnvironment {
    pub(super) fn from_saved_deepseek() -> LiveResult<Self> {
        let environment = Self::from_env_with_provider(AssistantProviderSettings {
            provider: "openai-compatible".into(),
            model: "deepseek-flash".into(),
            base_url: "https://api.deepseek.com".into(),
            // Production resolves the already saved SystemKeyring credential.
            // Keep its value out of the fixture, environment and evidence.
            api_key: String::new(),
        })?;
        let descriptor = crate::assistant::AssistantSecretDescriptor {
            provider: environment.provider.provider.clone(),
            base_url: environment.provider.base_url.clone(),
        };
        if !crate::assistant::read_secret_status(&descriptor)?.stored {
            return Err("the saved authorized DeepSeek credential is unavailable".into());
        }
        Ok(environment)
    }

    pub(super) fn from_env() -> LiveResult<Self> {
        let model = match env::var("LANGAME_ASSISTANT_LIVE_MODEL") {
            Ok(model) if !model.trim().is_empty() => model.trim().to_owned(),
            Err(env::VarError::NotPresent) => String::from("qwen3.5:9b"),
            _ => return Err("LANGAME_ASSISTANT_LIVE_MODEL must be a nonempty model name".into()),
        };
        Self::from_env_with_provider(AssistantProviderSettings {
            provider: "ollama".into(),
            model,
            base_url: "http://127.0.0.1:11434/v1".into(),
            api_key: String::new(),
        })
    }

    pub(super) fn from_env_with_provider(provider: AssistantProviderSettings) -> LiveResult<Self> {
        if env::var("LANGAME_ASSISTANT_LIVE").as_deref() != Ok("1") {
            return Err("set LANGAME_ASSISTANT_LIVE=1 to run native game acceptance".into());
        }
        let runtime_root = env::var_os("LANGAME_SMOKE_RUNTIME_ROOT")
            .ok_or("LANGAME_SMOKE_RUNTIME_ROOT must identify disposable evidence storage")?;
        let games_root = env::var_os("LANGAME_DST_SMOKE_GAMES_ROOT")
            .ok_or("LANGAME_DST_SMOKE_GAMES_ROOT must identify an owned official package copy")?;
        let install_root =
            validate_owned_install(Path::new(&runtime_root), Path::new(&games_root))?;
        for relative in [
            "bin64/dontstarve_dedicated_server_nullrenderer_x64.exe",
            "data/databundles/scripts.zip",
            "version.txt",
        ] {
            if !install_root.join(relative).is_file() {
                return Err(format!("owned official DST package is missing {relative}").into());
            }
            reject_reparse_ancestors(&install_root.join(relative))?;
        }
        Ok(Self {
            games_root: PathBuf::from(games_root).canonicalize()?,
            install_root,
            provider,
        })
    }

    pub(super) fn settings(&self, run_root: &Path) -> AppSettings {
        AppSettings {
            archives_root: String::new(),
            servers_root: run_root.join("i").to_string_lossy().into_owned(),
            games_root: self.games_root.to_string_lossy().into_owned(),
            modules_root: workspace_root()
                .join("modules")
                .to_string_lossy()
                .into_owned(),
            steamcmd_root: run_root.join("steamcmd").to_string_lossy().into_owned(),
        }
    }
}

pub(super) fn reject_reparse_ancestors(path: &Path) -> LiveResult {
    for ancestor in path.ancestors() {
        let metadata = fs::symlink_metadata(ancestor)?;
        if metadata.file_type().is_symlink() {
            return Err("live acceptance paths cannot cross symbolic links".into());
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err("live acceptance paths cannot cross reparse points".into());
            }
        }
    }
    Ok(())
}

pub(super) fn validate_owned_install(
    runtime_root: &Path,
    games_root: &Path,
) -> LiveResult<PathBuf> {
    reject_reparse_ancestors(runtime_root)?;
    reject_reparse_ancestors(games_root)?;
    let runtime_root = runtime_root.canonicalize()?;
    let games_root = games_root.canonicalize()?;
    let install_root = games_root.join("dontstarve");
    reject_reparse_ancestors(&install_root)?;
    if games_root == runtime_root || !games_root.starts_with(&runtime_root) {
        return Err("owned game copy must be a child of LANGAME_SMOKE_RUNTIME_ROOT".into());
    }
    // A broad runtime root must never authorize writing the user's default installation.
    if let Ok(persistent_root) = Path::new(DEFAULT_LANGAME_SERVER_FILES_ROOT).canonicalize()
        && (runtime_root.starts_with(&persistent_root)
            || persistent_root.starts_with(&runtime_root))
    {
        return Err(
            "disposable evidence storage must be separate from persistent game files".into(),
        );
    }
    if runtime_root.starts_with(workspace_root()) {
        return Err("native acceptance output must stay outside the repository".into());
    }
    reject_reparse_ancestors(&install_root.join("mods"))?;
    Ok(install_root)
}

pub(super) struct FixtureMods {
    pub(super) dependency: String,
    pub(super) consumer: String,
}

impl FixtureMods {
    pub(super) fn install(install_root: &Path) -> LiveResult<Self> {
        let mods = Self {
            dependency: format!(
                "lgsm_local_{}",
                &uuid::Uuid::new_v4().simple().to_string()[..12]
            ),
            consumer: format!(
                "lgsm_local_{}",
                &uuid::Uuid::new_v4().simple().to_string()[..12]
            ),
        };
        let mods_root = install_root.join("mods");
        reject_reparse_ancestors(&mods_root)?;
        for (name, priority, body) in [
            (&mods.dependency, 10, Self::dependency_script()),
            (&mods.consumer, 0, mods.consumer_script()),
        ] {
            let root = mods_root.join(name);
            // Never replace an existing mod, even inside the explicitly owned package.
            fs::create_dir(&root)?;
            let dependencies = if name == &mods.consumer {
                format!(
                    "mod_dependencies = {{ {{ [{:?}] = false }} }}\n",
                    mods.dependency
                )
            } else {
                String::new()
            };
            fs::write(
                root.join("modinfo.lua"),
                format!(
                    "name = {name:?}\ndescription = 'Local assistant acceptance fixture'\nauthor = 'LanGame'\nversion = '1.0'\napi_version = 10\ndst_compatible = true\nall_clients_require_mod = false\nclient_only_mod = false\nserver_only_mod = true\npriority = {priority}\n{dependencies}"
                ),
            )?;
            fs::write(root.join("modmain.lua"), body)?;
        }
        Ok(mods)
    }

    pub(super) fn dependency_script() -> String {
        format!("GLOBAL.{DEPENDENCY_GLOBAL} = true\n")
    }

    pub(super) fn consumer_script(&self) -> String {
        // DST's mod environment omits error/rawget; access them through GLOBAL.
        // rawget lets this fixture report the optional dependency before strict.lua.
        format!(
            "if not GLOBAL.rawget(GLOBAL, '{DEPENDENCY_GLOBAL}') then\n  GLOBAL.error('{MISSING_DEPENDENCY_MARKER}: {} required mod initialization was not observed')\nend\nGLOBAL.{CONSUMER_GLOBAL} = true\n",
            self.consumer
        )
    }

    pub(super) fn broken_overrides(&self) -> String {
        format!(
            "return {{ [{:?}] = {{ enabled = true }} }}\n",
            self.consumer
        )
    }
}

pub(super) struct NativeCleanup<'a> {
    pub(super) state: &'a DesktopState,
    pub(super) instance_id: String,
}

impl NativeCleanup<'_> {
    pub(super) async fn finalize(
        &self,
        state: tauri::State<'_, DesktopState>,
        storage: &StorageBootstrap,
        require_clean_stop: bool,
        evidence: &mut Value,
    ) -> LiveResult {
        let mut errors = Vec::new();
        evidence["cleanup"] = json!({"status": "finalizing"});
        match read_active_instance_run(&storage.paths, &self.instance_id).await {
            Ok(Some(_)) => match stop_instance_process(state, self.instance_id.clone()).await {
                Ok(stopped) => {
                    if require_clean_stop
                        && (stopped.process_count != 1
                            || stopped
                                .processes
                                .iter()
                                .any(|process| process.exit_code != Some(0)))
                    {
                        errors.push(String::from(
                            "isolated native acceptance process did not stop cleanly",
                        ));
                    }
                    evidence["cleanup"]["normalStop"] = json!(stopped);
                }
                Err(error) => errors.push(format!("normal stop: {error}")),
            },
            Ok(None) if require_clean_stop => errors.push(String::from(
                "verified native run disappeared before cleanup",
            )),
            Ok(None) => {}
            Err(error) => errors.push(format!("read active run before cleanup: {error}")),
        }
        match self.stop_remaining() {
            Ok(count) => evidence["cleanup"]["fallbackStoppedProcessCount"] = json!(count),
            Err(error) => errors.push(format!(
                "fallback stop (runtime ownership retained): {error}"
            )),
        }
        match read_active_instance_run(&storage.paths, &self.instance_id).await {
            Ok(active) => {
                evidence["cleanup"]["activeRunAfter"] = json!(active);
                if active.is_some() {
                    errors.push(String::from(
                        "isolated acceptance run remains registered after cleanup",
                    ));
                }
            }
            Err(error) => errors.push(format!("read active run after cleanup: {error}")),
        }
        evidence["cleanup"]["status"] = json!(if errors.is_empty() {
            "complete"
        } else {
            "failed"
        });
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; ").into())
        }
    }

    fn stop_remaining(&self) -> Result<usize, String> {
        self.stop_remaining_using(|runtime| {
            stop_managed_instance(runtime)
                .map(|stopped| stopped.len())
                .map_err(|error| error.to_string())
        })
    }

    fn stop_remaining_using(
        &self,
        stop: impl FnOnce(&mut app_runtime::ManagedInstance) -> Result<usize, String>,
    ) -> Result<usize, String> {
        let mut supervisor = self
            .state
            .runtime_supervisor
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let Some(mut runtime) = supervisor.take_running_for_stop(&self.instance_id) else {
            return Ok(0);
        };
        match stop(&mut runtime) {
            Ok(count) => Ok(count),
            Err(error) => {
                // Retaining this lock prevents replacement of the removed entry while stopping.
                // Restore ownership before returning so Drop can retry a failed explicit cleanup.
                let restored = supervisor.restore_running_after_failed_stop(runtime);
                assert!(
                    restored,
                    "cleanup must restore its exclusively removed runtime"
                );
                Err(error)
            }
        }
    }
}

impl Drop for NativeCleanup<'_> {
    fn drop(&mut self) {
        if let Err(error) = self.stop_remaining() {
            eprintln!(
                "native acceptance cleanup failed; runtime ownership retained for {}: {error}",
                self.instance_id
            );
        }
    }
}

#[test]
fn failed_native_cleanup_restores_supervisor_ownership() -> LiveResult {
    let state = DesktopState::default();
    let id = String::from("cleanup-fixture");
    let summary = InstanceSummary {
        id: id.clone(),
        name: String::from("cleanup"),
        module_id: String::from("dontstarve"),
        status: InstanceStatus::Running,
        active_process_count: 0,
        bind_ip: String::from("127.0.0.1"),
        port_count: 0,
        autostart: false,
    };
    state
        .runtime_supervisor
        .lock()
        .unwrap()
        .insert_running(summary, None, Vec::new());
    let cleanup = NativeCleanup {
        state: &state,
        instance_id: id.clone(),
    };
    let error = cleanup
        .stop_remaining_using(|_| Err(String::from("deliberate stop failure")))
        .unwrap_err();
    assert_eq!(error, "deliberate stop failure");
    assert!(state.runtime_supervisor.lock().unwrap().is_tracked(&id));
    drop(cleanup);
    assert!(!state.runtime_supervisor.lock().unwrap().is_tracked(&id));
    Ok(())
}

pub(super) async fn create_faulted_instance(
    state: tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    environment: &LiveEnvironment,
    fault: LiveFault,
) -> LiveResult<(InstanceDetails, Option<FixtureMods>)> {
    let version = fs::read_to_string(environment.install_root.join("version.txt"))?;
    app_storage::sync_game_installs(
        &storage.paths,
        &[app_storage::GameInstallSyncRecord {
            module_id: "dontstarve".into(),
            install_root: environment.install_root.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: Some(version.trim().into()),
            mark_verified: true,
        }],
    )
    .await?;
    let created = create_instance_record_inner(
        state.clone(),
        CreateInstanceInput {
            name: "ai".into(),
            module_id: "dontstarve".into(),
        },
    )
    .await?;
    let initial = read_instance_details(&storage.paths, &created.summary.id).await?;
    let mut settings: Value = serde_json::from_str(&initial.settings_json)?;
    for (key, value) in [
        ("offline_cluster", json!(true)),
        ("lan_only_cluster", json!(true)),
        ("disable_data_collection", json!(true)),
        ("cluster_token", json!("")),
        ("enable_caves", json!(false)),
        ("master_world_size", json!("small")),
        ("pause_when_empty", json!(false)),
        ("shared_workshop_mod_ids", json!("")),
        ("shared_workshop_collection_ids", json!("")),
        ("master_enabled_workshop_mod_ids", json!("")),
        ("caves_enabled_workshop_mod_ids", json!("")),
    ] {
        settings[key] = value;
    }
    let mods = match fault {
        LiveFault::WorldConfiguration => {
            settings[fault.setting_key()] = json!("return { override_enabled = true, preset = }");
            None
        }
        LiveFault::MissingModDependency => {
            let mods = FixtureMods::install(&environment.install_root)?;
            settings[fault.setting_key()] = json!(mods.broken_overrides());
            Some(mods)
        }
    };
    let mut ports = initial.ports.clone();
    for port in &mut ports {
        port.port = match port.name.as_str() {
            "master" => 11015,
            "caves" => 11016,
            "shard_master" => 18889,
            "steam_query" => 28994,
            "steam_auth" => 18764,
            "caves_steam_query" => 28995,
            "caves_steam_auth" => 18765,
            _ => return Err(format!("unexpected DST port {}", port.name).into()),
        };
        let _probe = std::net::UdpSocket::bind(("127.0.0.1", port.port))?;
    }
    let details = update_instance_record_if_current(
        state,
        UpdateInstanceInput {
            id: initial.summary.id.clone(),
            bind_ip: "127.0.0.1".into(),
            auto_backup_on_stop: false,
            backup_retention_count: 1,
            settings_json: settings.to_string(),
            ports,
        },
        initial.settings_json,
    )
    .await?;
    Ok((details, mods))
}

pub(super) async fn native_probe(
    state: &DesktopState,
    instance_id: &str,
    process: &app_core::StartedProcess,
    run_id: i64,
    expression: &str,
) -> LiveResult<String> {
    let marker = format!("[LGSM-LIVE-{}]", uuid::Uuid::new_v4().simple());
    dispatch_managed_stdin_command(
        state,
        instance_id,
        Some(&process.process_key),
        &format!("print('{marker} '..({expression}))"),
        Some(run_id),
    )
    .await?;
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let snapshot = read_log_path_snapshot(process.log_path.clone(), 80);
        if let Some(error) = snapshot.read_error {
            return Err(format!("failed to read native acceptance log: {error}").into());
        }
        if let Some(line) = snapshot
            .lines
            .iter()
            .find(|line| line.contains(&marker) && !line.contains("print("))
        {
            return Ok(line.split_once(&marker).unwrap().1.trim().to_string());
        }
        if Instant::now() >= deadline {
            return Err("native console did not return the acceptance probe".into());
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}
