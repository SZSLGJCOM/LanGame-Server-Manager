use super::*;

const NONCE: &str = "0000019a01234567abcdef0123456789";

#[test]
fn checkpoint_receipt_requires_exact_request_owned_line_and_positive_length() {
    let mut receipt = Receipt::new(NONCE);
    for line in [
        "[LanGame native control] native_save_completed bytes=123",
        "[LanGame native control] native_save_completed nonce=old bytes=123",
        &format!("player: [LanGame native control] native_save_completed nonce={NONCE} bytes=123"),
        &format!(" [LanGame native control] native_save_completed nonce={NONCE} bytes=123"),
    ] {
        receipt.observe(line).unwrap();
        assert_eq!(receipt.bytes, None);
    }
    receipt
        .observe(&format!(
            "[LanGame native control] native_save_completed nonce={NONCE} bytes=123"
        ))
        .unwrap();
    assert_eq!(receipt.bytes, Some(123));
}

#[test]
fn rejected_malformed_and_overlapping_checkpoint_results_never_confirm() {
    for suffix in [
        "",
        "0",
        "-1",
        "+1",
        "1 trailing",
        "1 ",
        "18446744073709551616",
    ] {
        let mut receipt = Receipt::new(NONCE);
        assert!(
            receipt
                .observe(&format!(
                    "[LanGame native control] native_save_completed nonce={NONCE} bytes={suffix}"
                ))
                .is_err()
        );
        assert_eq!(receipt.bytes, None);
    }
    for code in [
        "world_not_ready",
        "expired",
        "replayed",
        "invoke_IOException",
        "identity_checkpoint_unchanged",
    ] {
        let mut receipt = Receipt::new(NONCE);
        assert!(
            receipt
                .observe(&format!(
                    "[LanGame native control] native_save_failed nonce={NONCE} code={code}"
                ))
                .is_err()
        );
        assert_eq!(receipt.bytes, None);
    }
    let mut receipt = Receipt::new(NONCE);
    let completed = format!("[LanGame native control] native_save_completed nonce={NONCE} bytes=1");
    receipt.observe(&completed).unwrap();
    assert!(receipt.observe(&completed).is_err());
    assert!(
        receipt
            .observe(&format!(
                "[LanGame native control] native_save_failed nonce={NONCE} code=replayed"
            ))
            .is_err()
    );
}

fn fixture() -> InstanceDetails {
    serde_json::from_value(json!({
        "summary":{"id":"fixture","name":"Forest","module_id":"theforest","status":"Running",
            "active_process_count":1,"bind_ip":"127.0.0.1","port_count":0,"autostart":false},
        "config_file_path":"D:/fixture/config/server.cfg","saves_path":"D:/fixture/saves",
        "auto_backup_on_stop":false,"backup_retention_count":0,"settings_json":"{}","ports":[],
        "active_run":{"run_id":1,"session_id":"session","pid":42,"process_count":1,
            "processes":[{"run_id":1,"session_id":"session","process_key":"main","display_name":"Forest",
                "pid":42,"process_identity":{"image_path":"D:/fixture/TheForestDedicatedServer.exe","creation_time":123},
                "status":"running","crash_flag":false,"is_primary":true}]}
    })).unwrap()
}

#[test]
fn native_checkpoint_admission_rejects_wrong_process_or_run_generation() {
    assert!(target(&fixture()).is_ok());
    for change in 0..9 {
        let mut details = fixture();
        match change {
            0 => details.summary.module_id = "unturned".into(),
            1 => details.summary.status = InstanceStatus::Stopped,
            2 => details.summary.active_process_count = 2,
            3 => details.active_run.as_mut().unwrap().processes[0].run_id = 2,
            4 => {
                details.active_run.as_mut().unwrap().processes[0].session_id = Some("other".into())
            }
            5 => details.active_run.as_mut().unwrap().processes[0].crash_flag = true,
            6 => details.active_run.as_mut().unwrap().pid = Some(43),
            7 => details.active_run.as_mut().unwrap().processes[0].process_identity = None,
            8 => details.active_run.as_mut().unwrap().processes[0].process_key = "other".into(),
            _ => unreachable!(),
        }
        assert!(target(&details).is_err(), "{change}");
    }
}

#[test]
fn native_save_requires_verified_payload_and_owned_managed_log() {
    let root = std::env::temp_dir().join(format!("lgsm-forest-save-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let path = root.join("control.dll");
    std::fs::write(&path, b"unknown payload").unwrap();
    assert!(pin_hash(&path, GAME_SHA256, 1024, Instant::now() + BUDGET).is_err());
    assert!(pin_hash(&path, GAME_SHA256, 1, Instant::now() + BUDGET).is_err());
    assert!(pin_hash(&path, GAME_SHA256, 1024, Instant::now()).is_err());
    assert!(require_managed_log(&path).is_err());
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(root).unwrap();
    #[cfg(windows)]
    {
        let before = unsafe { os_uptime_milliseconds() };
        let nonce = request_nonce().unwrap();
        let after = unsafe { os_uptime_milliseconds() };
        assert_eq!(nonce.len(), 32);
        assert!(
            nonce
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        );
        let encoded_uptime = u64::from_str_radix(&nonce[..16], 16).unwrap();
        assert!((before..=after).contains(&encoded_uptime));
    }
}
