//! Opt-in real-network diagnostics. Never installs a file or reads account data.
use super::*;

#[derive(Default)]
struct LiveAutomation(Mutex<Vec<serde_json::Value>>);

pub(super) fn record_automation(app: &tauri::AppHandle, automation: &Automation) {
    let Some(state) = app.try_state::<LiveAutomation>() else {
        return;
    };
    let Ok(mut values) = state.0.lock() else {
        return;
    };
    let reason = automation.reason.as_deref().filter(|reason| {
        matches!(
            *reason,
            "login"
                | "verification"
                | "file_selection"
                | "project_identity"
                | "website_dialog"
                | "website_controls"
                | "download_not_started"
        )
    });
    let value = json!({ "state": match automation.state {
        AutomationState::Loading => "loading", AutomationState::Working => "working", AutomationState::Blocked => "blocked"
    }, "phase": automation.phase.map(AutomationPhase::name), "reason": reason });
    if values.last() != Some(&value) && values.len() < 64 {
        values.push(value);
    }
}

fn target_from_url(value: &str) -> Result<NexusWebsiteTarget, String> {
    let input = reqwest::Url::parse(value).map_err(|_| "Invalid public project URL")?;
    let parts = input
        .path()
        .trim_matches('/')
        .split('/')
        .collect::<Vec<_>>();
    let parts = parts.strip_prefix(&["games"]).unwrap_or(&parts);
    let (domain, id, path_file) = match parts {
        [domain, "mods", id] | [domain, "mods", id, "files"] => (*domain, *id, None),
        [domain, "mods", id, "files", file] => (*domain, *id, Some(*file)),
        _ => return Err("Expected a public Nexus project URL".into()),
    };
    let valid_id = |value: &str| value.parse::<u64>().is_ok_and(|id| id > 0);
    if !valid_id(id)
        || domain.len() > 128
        || !domain
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_-".contains(&byte))
    {
        return Err("Invalid public Nexus project identity".into());
    }
    let mut target = NexusWebsiteTarget {
        page_url: format!("https://www.nexusmods.com/{domain}/mods/{id}?tab=files"),
        game_domain: domain.into(),
        mod_id: id.into(),
        accepts: vec!["zip".into()],
    };
    if !matches_project(value, &target) {
        return Err("Only an HTTPS official Nexus project can be probed".into());
    }
    let query_files = input
        .query_pairs()
        .filter(|(key, _)| key == "file_id")
        .map(|(_, value)| value.into_owned())
        .collect::<Vec<_>>();
    if query_files.len() > 1 {
        return Err("Multiple file identities are not supported".into());
    }
    let file = query_files.first().map(String::as_str).or(path_file);
    if let Some(file) = file {
        if !valid_id(file) || path_file.is_some_and(|path| path != file) {
            return Err("Invalid explicit file identity".into());
        }
        target.page_url.push_str("&file_id=");
        target.page_url.push_str(file);
    }
    Ok(target)
}

fn previous_probe_profile(root: &Path) -> Result<PathBuf, String> {
    use std::os::windows::{fs::MetadataExt, process::CommandExt};
    if !root.is_absolute() {
        return Err("Previous probe directory must be absolute".into());
    }
    let report = root.join("result.json");
    let profile = root.join("profile");
    for path in [root, report.as_path(), profile.as_path()] {
        let metadata =
            std::fs::symlink_metadata(path).map_err(|_| "Previous probe evidence is incomplete")?;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("Previous probe evidence cannot use reparse points".into());
        }
    }
    if !root.is_dir()
        || !profile.is_dir()
        || !report.is_file()
        || std::fs::metadata(&report)
            .map_err(|_| "Previous probe report is unavailable")?
            .len()
            > 8 * 1024 * 1024
    {
        return Err("Previous probe evidence has an invalid shape".into());
    }
    let report: serde_json::Value = serde_json::from_slice(
        &std::fs::read(report).map_err(|_| "Cannot read previous probe report")?,
    )
    .map_err(|_| "Previous probe report is invalid")?;
    if report["real_network"] != true
        || report["isolated_profile"] != true
        || report["runtime_install_called"] != false
        || report["fixture"] != false
        || report["status"] != "observed"
    {
        return Err("Only a completed isolated live probe profile can be reused".into());
    }
    let profile =
        dunce::canonicalize(profile).map_err(|_| "Cannot resolve previous probe profile")?;
    // Check only native/WebView2 processes. No command lines or profile data are
    // returned, and the path is passed as an environment value, never shell code.
    let status = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", r#"$ErrorActionPreference='Stop'; $profilePath=$env:LANGAME_NEXUS_PROBE_PROFILE.Replace('\','/'); $active=@(Get-CimInstance Win32_Process | Where-Object { $_.Name -in @('langame-desktop.exe','msedgewebview2.exe') -and $_.CommandLine -and $_.CommandLine.Replace('\','/').IndexOf($profilePath,[StringComparison]::OrdinalIgnoreCase) -ge 0 }); if($active.Count -gt 0) { exit 2 }; exit 0"#])
        .env("LANGAME_NEXUS_PROBE_PROFILE", &profile)
        .creation_flags(0x08000000)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|_| "Cannot verify previous profile process ownership")?;
    if !status.success() {
        return Err(
            "Previous probe profile is active or its ownership could not be verified".into(),
        );
    }
    Ok(profile)
}

pub(super) fn run(value: &str, root: PathBuf, profile_from: Option<PathBuf>) -> Result<(), String> {
    let target = target_from_url(value)?;
    if !root.is_absolute() {
        return Err("Live probe directory must be absolute".into());
    }
    let profile = match &profile_from {
        Some(previous) => previous_probe_profile(previous)?,
        None => root.join("profile"),
    };
    std::fs::create_dir(&root).map_err(|error| error.to_string())?;
    main_ipc::initialize_storage(&root)?;
    std::fs::create_dir(root.join("downloads")).map_err(|error| error.to_string())?;
    unsafe {
        std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", &profile);
        for key in [
            "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
            "WEBVIEW2_BROWSER_EXECUTABLE_FOLDER",
            "WEBVIEW2_PIPE_FOR_SCRIPT_DEBUGGER",
            "WEBVIEW2_WAIT_FOR_SCRIPT_DEBUGGER",
        ] {
            std::env::remove_var(key);
        }
    }
    let mut context = tauri::tauri_build_context!();
    context.config_mut().app.windows.clear();
    let app = tauri::Builder::default()
        .manage(BrowserState::new(root.join("logs/desktop-ui/active.jsonl")))
        .manage(LiveAutomation::default())
        .manage(crate::state::DesktopState::default())
        .invoke_handler(crate::command_registry::invoke_handler)
        .setup(move |app| {
            WebviewWindowBuilder::new(
                app,
                "main",
                WebviewUrl::External("about:blank".parse().unwrap()),
            )
            .inner_size(1100.0, 800.0)
            .visible(false)
            .focused(false)
            .skip_taskbar(true)
            .build()?;
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let result = probe(&handle, &root, &profile, profile_from.as_deref(), target).await;
                let report = match &result {
                    Ok(value) => value.clone(),
                    Err(error) => json!({ "status": "probe_failed", "error": error }),
                };
                let saved = std::fs::write(root.join("result.json"), report.to_string());
                handle.exit(if result.is_ok() && saved.is_ok() {
                    0
                } else {
                    1
                });
            });
            Ok(())
        })
        .build(context)
        .map_err(|error| error.to_string())?;
    app.run(|_, event| {
        if let tauri::RunEvent::ExitRequested {
            code: None, api, ..
        } = event
        {
            api.prevent_exit();
        }
    });
    Ok(())
}

async fn attach_observer(
    view: &Webview,
    root: &Path,
    profile: &Path,
    downloads: Arc<Mutex<Vec<serde_json::Value>>>,
) -> Result<(), String> {
    let profile = profile.to_path_buf();
    let destination = root.join("downloads/blocked-native-transfer");
    let (reply, response) = tokio::sync::oneshot::channel();
    view.with_webview(move |platform| {
        let result = (|| -> Result<(), String> {
            let environment: ICoreWebView2Environment7 = platform.environment().cast().map_err(|error| error.to_string())?;
            let mut actual = PWSTR::null();
            unsafe { environment.UserDataFolder(&mut actual) }.map_err(|error| error.to_string())?;
            let actual = PathBuf::from(take_pwstr(actual));
            if !dunce::simplified(&actual).to_string_lossy().eq_ignore_ascii_case(&profile.to_string_lossy()) {
                return Err("Live probe did not use its isolated profile".into());
            }
            let core = unsafe { platform.controller().CoreWebView2() }.map_err(|error| error.to_string())?;
            let profiled: ICoreWebView2_13 = core.cast().map_err(|error| error.to_string())?;
            let browser_profile = unsafe { profiled.Profile() }.map_err(|error| error.to_string())?;
            let directory = wide(&destination.parent().unwrap().to_string_lossy());
            unsafe { browser_profile.SetDefaultDownloadFolderPath(PCWSTR(directory.as_ptr())) }.map_err(|error| error.to_string())?;
            let downloadable: ICoreWebView2_4 = core.cast().map_err(|error| error.to_string())?;
            let mut token = 0;
            unsafe {
                downloadable.add_DownloadStarting(&DownloadStartingEventHandler::create(Box::new(move |_, args| {
                    let Some(args) = args else { return Ok(()) };
                    let mut cancelled = BOOL::default();
                    args.Cancel(&mut cancelled)?;
                    if let Ok(mut events) = downloads.lock() {
                        events.push(json!({ "native_event": true, "cancelled_by_product": cancelled.as_bool() }));
                    }
                    // Observe no URI. The selector owns the short-lived signed URL;
                    // both callbacks reject browser writes, and no runtime is started.
                    let destination = wide(&destination.to_string_lossy());
                    args.SetResultFilePath(PCWSTR(destination.as_ptr()))?;
                    args.SetCancel(true)?;
                    Ok(())
                })), &mut token)
            }.map_err(|error| error.to_string())?;
            Ok(())
        })();
        let _ = reply.send(result);
    }).map_err(|error| error.to_string())?;
    tokio::time::timeout(Duration::from_secs(3), response)
        .await
        .map_err(|_| "Live probe observer timed out")?
        .map_err(|_| "Live probe observer response lost")?
}

async fn snapshot(view: &Webview, script: &str) -> Option<serde_json::Value> {
    let (reply, mut response) = tokio::sync::mpsc::channel(1);
    view.eval_with_callback(script, move |value| {
        let _ = reply.try_send(value);
    })
    .ok()?;
    let value = tokio::time::timeout(Duration::from_secs(2), response.recv())
        .await
        .ok()??;
    if value.len() > 98304 {
        return None;
    }
    serde_json::from_str(&value).ok()
}

async fn observe(
    app: &tauri::AppHandle,
    root: &Path,
    profile: &Path,
    target: &NexusWebsiteTarget,
    done: &AtomicBool,
    downloads: Arc<Mutex<Vec<serde_json::Value>>>,
) -> Result<serde_json::Value, String> {
    let started = Instant::now();
    let script = format!(
        "(() => {{ const page = {}; const rendered = {}; return ({})({}, page, rendered); }})()",
        include_str!("nexus_page_extract.js"),
        include_str!("nexus_browser_readiness.js"),
        include_str!("nexus_browser_live_probe.js"),
        automatic_context(target)?
    );
    let mut attached = false;
    let mut geometry_checks = 0;
    let mut last_geometry = None;
    let mut samples = Vec::new();
    let mut previous = None;
    while !done.load(Ordering::Acquire) {
        let Some(view) = app
            .webviews()
            .into_values()
            .find(|view| is_browser_window(view.label()))
        else {
            tokio::time::sleep(Duration::from_millis(20)).await;
            continue;
        };
        if !attached {
            attach_observer(&view, root, profile, downloads.clone()).await?;
            attached = true;
        }
        match background_geometry(&view).await {
            Ok(geometry) => {
                geometry_checks += 1;
                last_geometry = Some(geometry);
            }
            Err(_) if app.get_webview(view.label()).is_none() => continue,
            Err(failure) => {
                close_active_host(app);
                return Err(failure);
            }
        }
        if let Some(value) = snapshot(&view, &script).await
            && previous.as_ref() != Some(&value)
        {
            previous = Some(value.clone());
            if samples.len() < 80 {
                let automation = app
                    .state::<LiveAutomation>()
                    .0
                    .lock()
                    .ok()
                    .and_then(|values| values.last().cloned());
                samples.push(json!({ "elapsed_ms": started.elapsed().as_millis(), "page": value, "automation": automation }));
                std::fs::write(
                    root.join("snapshots.json"),
                    serde_json::to_vec_pretty(&samples)
                        .map_err(|_| "Cannot encode safe snapshots")?,
                )
                .map_err(|error| error.to_string())?;
            }
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    Ok(
        json!({ "observer_attached": attached, "always_hidden": true, "geometry_checks": geometry_checks,
        "background_geometry": last_geometry, "snapshots": samples }),
    )
}

async fn probe(
    app: &tauri::AppHandle,
    root: &Path,
    profile: &Path,
    profile_from: Option<&Path>,
    target: NexusWebsiteTarget,
) -> Result<serde_json::Value, String> {
    let started = Instant::now();
    let id = open_nexus_website_host(app.clone())?;
    let session = SessionLease::begin(app, id)?;
    let phases = Mutex::new(Vec::new());
    let downloads = Arc::new(Mutex::new(Vec::new()));
    let done = AtomicBool::new(false);
    let progress = |phase: &str| -> Result<(), String> {
        phases
            .lock()
            .map_err(|_| "Live progress state unavailable")?
            .push(phase.to_owned());
        Ok(())
    };
    let select = async {
        let result = select_download_with_fixture(&session, target.clone(), &progress, None).await;
        done.store(true, Ordering::Release);
        result
    };
    let (selected, observed) = tokio::join!(
        select,
        observe(app, root, profile, &target, &done, downloads.clone())
    );
    drop(session);
    let observed = observed?;
    let terminal = match selected {
        Ok(selected) => {
            json!({ "code": "download_captured", "file_name": normalize_display_name(Some(&selected.file_name)),
            "metadata": { "title": selected.page.title, "icon_url": selected.page.icon_url,
                "game_domain": selected.page.game_domain, "mod_id": selected.page.mod_id } })
        }
        Err(value) => {
            let value: serde_json::Value = serde_json::from_str(&value)
                .map_err(|_| "Selector returned an unstructured error")?;
            let code = value["code"]
                .as_str()
                .filter(|code| code.starts_with("nexus_"))
                .ok_or("Selector returned no fixed code")?;
            json!({ "code": code })
        }
    };
    ensure_closed(app).await?;
    let no_download_files = std::fs::read_dir(root.join("downloads"))
        .map_err(|error| error.to_string())?
        .next()
        .is_none();
    if !no_download_files {
        return Err("Live probe browser wrote an unexpected download".into());
    }
    Ok(
        json!({ "status": "observed", "real_network": true, "fixture": false, "isolated_profile": true,
        "profile_reused": profile_from.is_some(), "profile_source": profile_from,
        "project": { "url": target.page_url, "game_domain": target.game_domain, "mod_id": target.mod_id },
        "elapsed_ms": started.elapsed().as_millis(), "phases": *phases.lock().map_err(|_| "Live progress unavailable")?,
        "terminal": terminal, "automation": *app.state::<LiveAutomation>().0.lock().map_err(|_| "Live automation unavailable")?,
        "downloads": *downloads.lock().map_err(|_| "Live download observations unavailable")?,
        "observation": observed, "no_download_files": no_download_files, "runtime_install_called": false,
        "top_level_windows": app.windows().len(), "child_views_remaining": app.webviews().into_values().filter(|view| is_browser_window(view.label())).count() }),
    )
}
