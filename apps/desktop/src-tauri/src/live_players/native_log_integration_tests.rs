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
    check_current_run_names("enshrouded", "[server] Machine '1': Player '0(0)' logged in\n[server] Player '测试玩家' logged in with Permissions:\n").await;
}

#[tokio::test]
async fn valheim_anonymous_a2s_response_uses_current_run_names() {
    check_current_run_names("valheim", "10/16/2026 20:55:19: Player history entry with index 0: Historical (Steam_123, abc)\n10/16/2026 20:55:19: Server: New peer connected,sending global keys\n10/16/2026 20:55:39: Got character ZDOID from 测试玩家 : 42:1\n").await;
}

async fn check_current_run_names(module_id: &str, native_log: &str) {
    let fixture = Fixture::new();
    let log_path = fixture.0.join("managed-console/run-91-main.log");
    let log = app_storage::managed_console_log::ManagedConsoleLog::open(&log_path)
        .expect("managed current-run log");
    log.write_all(native_log.as_bytes())
        .expect("native join evidence");
    let socket = UdpSocket::bind("127.0.0.1:0").expect("query fixture");
    socket
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("query timeout");
    let port = socket.local_addr().expect("query address").port();
    let valheim = module_id == "valheim";
    let server = std::thread::spawn(move || {
        let mut bytes = [0_u8; 128];
        let (_, peer) = socket.recv_from(&mut bytes).expect("PLAYER request");
        assert_eq!(bytes[4], b'U');
        // Valheim publishes no PLAYER rows; Enshrouded publishes an anonymous row.
        let player_reply: &[u8] = if valheim {
            b"\xff\xff\xff\xffD\x00"
        } else {
            b"\xff\xff\xff\xffD\x01\x00\x00\x00\x00\x00\x00\x55\x3f\x48\x43"
        };
        socket
            .send_to(player_reply, peer)
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
        "summary": { "id": "enshrouded-fixture", "name": "Fixture", "module_id": module_id,
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
        .find(|module| module.summary.id == module_id)
        .expect("Enshrouded declaration");
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("test app");
    let permit = app
        .state::<DesktopState>()
        .acquire_instance_mutation(&details.summary.id)
        .await;
    let snapshot = collect_direct_players(
        &app.state::<DesktopState>(),
        &details,
        module,
        1_000,
        permit,
    )
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
    if module_id == "valheim" {
        assert_eq!(snapshot.entries[0].identifiers.len(), 1);
        let identity = &snapshot.entries[0].identifiers[0];
        assert_eq!(
            identity.kind,
            app_core::RuntimePlayerIdentityKind::PlayerName
        );
        assert_eq!(identity.value, "测试玩家");
        assert!(!identity.stable);
    } else {
        assert!(snapshot.entries[0].identifiers.is_empty());
    }
    assert!(snapshot.entries[0].available_action_ids.is_empty());
    assert_eq!(snapshot.entries[0].session_started_at_unix_ms, None);
}
