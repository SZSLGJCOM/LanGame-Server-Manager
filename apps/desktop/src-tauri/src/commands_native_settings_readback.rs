//! Verify native settings projections without exempting settings from readback.
use super::*;
use crate::commands::commands_managed_save_control::{guard_pin_directories, guard_read_regular};

pub(super) fn verify_stopped(
    before: &InstanceDetails,
    after: &InstanceDetails,
    program_root: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    if after.summary.id != before.summary.id
        || after.summary.module_id != before.summary.module_id
        || !matches!(after.summary.status, app_core::InstanceStatus::Stopped)
        || after.summary.active_process_count != 0
        || after.active_run.is_some()
        || after.config_file_path != before.config_file_path
        || after.saves_path != before.saves_path
        || serde_json::to_value(&after.ports)? != serde_json::to_value(&before.ports)?
        || creation::program_root(after)? != program_root
    {
        return Err(
            "native stopped readback changed instance ownership or storage bindings".into(),
        );
    }
    verify(
        &before.summary.module_id,
        &before.settings_json,
        &after.settings_json,
        program_root,
    )
}

pub(super) fn verify(
    module: &str,
    before: &str,
    after: &str,
    program_root: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    if module == "astroneer" {
        return crate::astroneer_public_ip::verify_startup_settings(before, after)
            .map(|_| ())
            .map_err(Into::into);
    }
    if module != "returntomoria" {
        return if before == after {
            Ok(())
        } else {
            Err("native settings changed during stopped readback".into())
        };
    }

    let _pins = guard_pin_directories(program_root)?;
    let permissions = program_root.join("MoriaServerPermissions.txt");
    let mut expected: serde_json::Map<String, Value> = serde_json::from_str(before)?;
    let actual: serde_json::Map<String, Value> = serde_json::from_str(after)?;
    match fs::symlink_metadata(&permissions) {
        Ok(_) => {
            // The game owns this roster. Storage projects its exact UTF-8 bytes
            // into details, while leaving the persisted instance settings alone.
            // Match that contract; never just remove/ignore permissions_lines.
            let content = String::from_utf8(guard_read_regular(&permissions, 256 * 1024)?)?;
            expected.insert("permissions_lines".into(), Value::String(content));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    if expected != actual {
        // Values may contain credentials or player names. Report keys only.
        let changed: std::collections::BTreeSet<_> = expected
            .keys()
            .chain(actual.keys())
            .filter(|key| expected.get(*key) != actual.get(*key))
            .collect();
        return Err(format!("native Moria settings readback differs in keys: {changed:?}").into());
    }
    Ok(())
}

#[test]
fn moria_settings_readback_requires_exact_native_roster_and_other_settings() {
    let root = temp_test_dir("moria-settings-readback");
    let native = "\u{feff}Default = AllConstruction,AllStorage\r\nAlice = AllStorage\r\n";
    fs::write(root.join("MoriaServerPermissions.txt"), native).unwrap();
    let before = serde_json::json!({"permissions_lines":"", "difficulty_preset":"normal"});
    let mut after = before.clone();
    after["permissions_lines"] = native.into();
    let check = |value: &Value| {
        verify(
            "returntomoria",
            &before.to_string(),
            &value.to_string(),
            &root,
        )
    };
    check(&after).unwrap();
    assert!(check(&before).is_err(), "stale projected roster must fail");
    after["permissions_lines"] = native.trim().into();
    assert!(check(&after).is_err(), "roster bytes must match exactly");
    after["permissions_lines"] = native.into();
    after["difficulty_preset"] = "hard".into();
    assert!(check(&after).is_err(), "other settings are not exempt");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn moria_settings_readback_does_not_accept_missing_or_invalid_native_evidence() {
    let root = temp_test_dir("moria-settings-evidence");
    let path = root.join("MoriaServerPermissions.txt");
    let before = r#"{"permissions_lines":"original"}"#;
    let after = r#"{"permissions_lines":"changed"}"#;
    verify("returntomoria", before, before, &root).unwrap();
    assert!(verify("returntomoria", before, after, &root).is_err());
    for bytes in [vec![0xff], vec![b'a'; 256 * 1024 + 1]] {
        fs::write(&path, bytes).unwrap();
        assert!(verify("returntomoria", before, after, &root).is_err());
    }
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(verify("returntomoria", before, after, &root).is_err());
    fs::remove_dir_all(root).unwrap();
}
