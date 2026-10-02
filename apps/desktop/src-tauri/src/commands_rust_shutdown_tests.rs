use super::*;

#[tokio::test(flavor = "current_thread")]
async fn rust_shutdown_dispatches_save_then_quit_over_websocket_rcon()
-> Result<(), Box<dyn std::error::Error>> {
    assert_rust_shutdown_wire(true).await
}

#[tokio::test(flavor = "current_thread")]
async fn rust_shutdown_dispatches_save_then_quit_over_source_rcon()
-> Result<(), Box<dyn std::error::Error>> {
    assert_rust_shutdown_wire(false).await
}

async fn assert_rust_shutdown_wire(web: bool) -> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let run_root = temp_test_dir("rust-shutdown");
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let settings = isolated_smoke_app_settings(&run_root)?;
    prepare_fake_rust_install(&settings)?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(
            &bootstrap_storage().expect("bootstrap isolated fixture storage"),
        ))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    sync_modules_to_storage(app.state::<DesktopState>()).await?;
    let provisioning =
        create_fake_module_instance(app.state::<DesktopState>(), "rust", "Rust shutdown").await?;
    let storage = bootstrap_storage()?;
    let mut details = read_instance_details(&storage.paths, &provisioning.summary.id).await?;
    let mut settings_json: Value = serde_json::from_str(&details.settings_json)?;
    assert_eq!(settings_json["rcon_web"], true);
    let password = settings_json["rcon_password"].as_str().unwrap().to_string();
    assert!(!password.is_empty());
    settings_json["rcon_web"] = Value::Bool(web);
    details.settings_json = serde_json::to_string(&settings_json)?;

    let mut shutdown = load_module_shutdown_specs(&storage, InstanceShutdownSource::Manual)
        .remove("rust")
        .expect("Rust shutdown strategy");
    for command in &mut shutdown.commands {
        assert!(command.fallback_transport.is_none());
        command.wait_after_ms = 0;
    }
    if web {
        let valid_settings = details.settings_json.clone();
        for invalid in ["{", "{}", r#"{"rcon_web":"false"}"#, r#"{"rcon_web":0}"#] {
            details.settings_json = String::from(invalid);
            let error = dispatch_instance_shutdown_commands(
                &app.state::<DesktopState>(),
                &storage,
                &details,
                &shutdown,
                InstanceShutdownSource::Manual,
            )
            .await
            .expect_err("invalid Rust protocol settings must fail before dispatch");
            assert!(error.contains("Rust shutdown"), "{error}");
        }
        details.settings_json = valid_settings;
    }
    let (rcon_port, command_rx, server) = if web {
        spawn_websocket_rcon_capture_server_commands(vec!["saved", "quitting"])
    } else {
        spawn_rust_rcon_capture_server()
    };
    details
        .ports
        .iter_mut()
        .find(|port| port.name == "rcon")
        .unwrap()
        .port = rcon_port;
    dispatch_instance_shutdown_commands(
        &app.state::<DesktopState>(),
        &storage,
        &details,
        &shutdown,
        InstanceShutdownSource::Manual,
    )
    .await?;
    let captured = command_rx.iter().collect::<Vec<_>>();
    server.join().expect("mock RCON server");
    assert_eq!(
        captured,
        [
            (password.to_string(), String::from("server.save")),
            (password.to_string(), String::from("quit")),
        ]
    );
    let audit = fs::read_to_string(desktop_app_log_path(&storage))?;
    assert_eq!(audit.matches("instance.shutdown.command_sent").count(), 2);
    assert!(!audit.contains("instance.shutdown.command_failed"));
    assert!(!audit.contains("instance.shutdown.command_fallback"));
    let expected_transport = if web { "websocket_rcon" } else { "source_rcon" };
    assert_eq!(
        audit
            .matches(&format!("\"transport\":\"{expected_transport}\""))
            .count(),
        2
    );
    Ok(())
}

fn spawn_rust_rcon_capture_server() -> (
    u16,
    std::sync::mpsc::Receiver<(String, String)>,
    std::thread::JoinHandle<()>,
) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = std::sync::mpsc::channel();
    let server = std::thread::spawn(move || {
        for response in ["saved", ""] {
            let mut stream = accept_runtime_transport_test_client(&listener);
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let auth = source_rcon_read_packet(&mut stream).unwrap();
            assert_eq!(auth.packet_type, 3);
            source_rcon_write_packet(&mut stream, auth.id, 0, "").unwrap();
            source_rcon_write_packet(&mut stream, auth.id, 2, "").unwrap();
            let exec = source_rcon_read_packet(&mut stream).unwrap();
            assert_eq!(exec.packet_type, 2);
            tx.send((
                String::from_utf8(auth.body).unwrap(),
                String::from_utf8(exec.body).unwrap(),
            ))
            .unwrap();
            source_rcon_write_packet(&mut stream, 0, 4, "console broadcast").unwrap();
            if !response.is_empty() {
                source_rcon_write_packet(&mut stream, exec.id, 0, response).unwrap();
            }
            source_rcon_write_packet(&mut stream, -1, 0, "").unwrap();
            let mut additional_request = [0; 1];
            assert_eq!(
                stream.read(&mut additional_request).unwrap(),
                0,
                "Rust completion must not send an extra command or marker"
            );
        }
    });
    (port, rx, server)
}
