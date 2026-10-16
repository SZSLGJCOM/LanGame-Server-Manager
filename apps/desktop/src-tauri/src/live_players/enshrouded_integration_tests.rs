#![cfg(test)]
use super::*;
use std::net::UdpSocket;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::Manager;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("lgsm-enshrouded-players-{}", uuid::Uuid::new_v4())))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if self.0.exists() {
            std::fs::remove_dir_all(&self.0).expect("remove owned player fixture");
        }
    }
}

#[tokio::test]
async fn enshrouded_anonymous_a2s_response_uses_current_run_names() {
    let fixture = Fixture::new();
    let log_path = fixture.0.join("managed-console/run-91-main.log");
    let log = app_storage::managed_console_log::ManagedConsoleLog::open(&log_path)
        .expect("managed current-run log");
    log.write_all(
        "[server] Machine '1': Player '0(0)' logged in\n[server] Player '测试玩家' logged in with Permissions:\n".as_bytes(),
    ).expect("native join evidence");
    let socket = UdpSocket::bind("127.0.0.1:0").expect("query fixture");
    socket
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("query timeout");
    let port = socket.local_addr().expect("query address").port();
    let server = std::thread::spawn(move || {
        let mut bytes = [0_u8; 128];
        let (_, peer) = socket.recv_from(&mut bytes).expect("PLAYER request");
        assert_eq!(bytes[4], b'U');
        // Enshrouded returns one anonymous player with score and duration.
        socket
            .send_to(
                b"\xff\xff\xff\xffD\x01\x00\x00\x00\x00\x00\x00\x55\x3f\x48\x43",
                peer,
            )
            .expect("anonymous PLAYER response");
        let (_, peer) = socket.recv_from(&mut bytes).expect("INFO request");
        assert_eq!(bytes[4], b'T');
        socket
            .send_to(
                b"\xff\xff\xff\xffI\x11server\0map\0folder\0game\0\x01\x00\x01\x10",
                peer,
            )
            .expect("INFO count");
    });
    let details: InstanceDetails = serde_json::from_value(serde_json::json!({
        "summary": { "id": "enshrouded-fixture", "name": "Fixture", "module_id": "enshrouded",
            "status": "Running", "bind_ip": "127.0.0.1", "port_count": 1, "autostart": false },
        "config_file_path": "", "saves_path": "", "auto_backup_on_stop": false,
        "backup_retention_count": 1, "settings_json": "{}",
        "ports": [{ "name": "query", "protocol": "udp", "port": port }],
        "active_run": { "run_id": 91, "session_id": "run-91", "pid": 123, "log_path": log_path,
            "process_count": 1, "processes": [{ "run_id": 91, "session_id": "run-91",
                "process_key": "main", "display_name": "Server", "pid": 123,
                "process_identity": { "creation_time": 100, "image_path": "enshrouded_server.exe" },
                "status": "running", "started_at": null, "stopped_at": null, "exit_code": null,
                "crash_flag": false, "log_path": log_path, "is_primary": true }] }
    }))
    .expect("current run fixture");
    let modules = app_modules::discover_modules(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../modules"),
    )
    .expect("production module declarations");
    let module = modules
        .iter()
        .find(|module| module.summary.id == "enshrouded")
        .expect("Enshrouded declaration");
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("test app");
    let snapshot = collect_direct_players(&app.state::<DesktopState>(), &details, module, 1_000)
        .await
        .expect("player collection")
        .public_snapshot;
    server.join().expect("query fixture completed");
    assert_eq!(snapshot.current_players, Some(1));
    assert_eq!(
        snapshot.entries.len(),
        1,
        "anonymous A2S rows need the current run's native names"
    );
    assert_eq!(snapshot.entries[0].display_name, "测试玩家");
    assert!(snapshot.complete);
    assert!(snapshot.entries[0].identifiers.is_empty());
    assert!(snapshot.entries[0].available_action_ids.is_empty());
    assert_eq!(snapshot.entries[0].session_started_at_unix_ms, None);
}
