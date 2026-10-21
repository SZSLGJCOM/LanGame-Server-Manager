//! Opt-in archive checks around real Sons saves and normal native shutdown.
//! Read-only; never calls saving or repairs a game file to make acceptance pass.
use super::*;
use crate::commands::commands_managed_save_control::{guard_pin_directories, guard_read_regular};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs::{File, OpenOptions};
use std::io::{Cursor, Read, Seek, SeekFrom};

const LIMIT: usize = 64 * 1024 * 1024;
const SLOT: &str = "0000000001";

pub(super) struct Snapshot {
    slot: PathBuf,
    log: File,
    log_prefix: LogPrefix,
    primary_sha256: String,
    game_id: Value,
    backups: BTreeSet<String>,
}

struct LogPrefix {
    length: usize,
    sha256: String,
}

fn read_log(file: &mut File) -> Result<Vec<u8>, String> {
    let metadata = file
        .metadata()
        .map_err(|_| "Cannot inspect pinned Sons log")?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("Pinned Sons log is redirected".into());
        }
    }
    if !metadata.is_file() || metadata.len() > 16 * 1024 * 1024 {
        return Err("Pinned Sons log is not a bounded ordinary file".into());
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|_| "Cannot rewind pinned Sons log")?;
    let mut bytes = Vec::new();
    file.take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read pinned Sons log")?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err("Pinned Sons log exceeds its bound".into());
    }
    Ok(bytes)
}

fn log_prefix(bytes: &[u8]) -> LogPrefix {
    let length = bytes.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
    LogPrefix {
        length,
        sha256: hash(&bytes[..length]),
    }
}

fn verify_log(prefix: &LogPrefix, bytes: &[u8]) -> Result<(), String> {
    if bytes.len() < prefix.length || hash(&bytes[..prefix.length]) != prefix.sha256 {
        return Err("Sons loader log was truncated or its captured prefix changed".into());
    }
    let full = std::str::from_utf8(bytes).map_err(|_| "Sons loader log is not UTF-8")?;
    let tail = std::str::from_utf8(&bytes[prefix.length..])
        .map_err(|_| "Sons loader log tail is not UTF-8")?;
    if !full.contains("LGSM_SONS_BACKUP_PATCH_READY")
        || tail.matches("LGSM_SONS_BACKUP_PUBLISHED").count() < 2
        || tail.contains("LGSM_SONS_BACKUP_FAILED")
        || tail.contains("IOException")
    {
        return Err(
            "Sons native backup patch did not record a healthy publication lifecycle".into(),
        );
    }
    Ok(())
}

fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn primary(bytes: &[u8]) -> Result<Value, String> {
    sons_world_fixture::validate_archive(bytes, &hash(bytes))?;
    let mut archive =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| "Invalid primary ZIP")?;
    let mut entry = archive
        .by_name("GameStateSaveData.json")
        .map_err(|_| "Sons primary has no game-state entry")?;
    let mut body = Vec::new();
    (&mut entry)
        .take(1024 * 1024 + 1)
        .read_to_end(&mut body)
        .map_err(|_| "Cannot read Sons game state")?;
    if body.len() > 1024 * 1024 {
        return Err("Sons game state exceeds its acceptance bound".into());
    }
    let outer: Value = serde_json::from_slice(&body).map_err(|_| "Invalid Sons game state")?;
    let state = outer
        .pointer("/Data/GameState")
        .ok_or("Missing Sons native game state")?;
    let state = match state.as_str() {
        Some(text) => {
            serde_json::from_str::<Value>(text).map_err(|_| "Invalid nested Sons game state")?
        }
        None => state.clone(),
    };
    state
        .get("GameId")
        .filter(|id| id.is_string() || id.is_number())
        .cloned()
        .ok_or_else(|| "Missing Sons native world identity".into())
}

fn backups(slot: &Path) -> Result<BTreeSet<String>, String> {
    let parent = slot.parent().ok_or("Sons slot has no parent")?;
    let _pins = guard_pin_directories(parent)?;
    let mut names = BTreeSet::new();
    let entries = fs::read_dir(parent).map_err(|_| "Cannot enumerate native Sons backups")?;
    for (index, entry) in entries.enumerate() {
        if index >= 512 {
            return Err("Sons backup acceptance directory exceeds its bound".into());
        }
        let name = entry
            .map_err(|_| "Cannot inspect native Sons backup")?
            .file_name()
            .into_string()
            .map_err(|_| "Native Sons backup name is not Unicode")?;
        if name.starts_with(".lgsm-backup-") && name.ends_with(".partial") {
            return Err("Sons backup publication left an incomplete archive".into());
        }
        if name.starts_with(&format!("{SLOT}_backup_")) && name.ends_with(".zip") {
            names.insert(name);
        }
    }
    Ok(names)
}

pub(super) async fn capture<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    running: &InstanceDetails,
) -> Result<Option<Snapshot>, Box<dyn std::error::Error>> {
    match std::env::var("LANGAME_NATIVE_SONS_BACKUP_ACCEPTANCE") {
        Err(std::env::VarError::NotPresent) => return Ok(None),
        Ok(value) if value == "true" && running.summary.module_id == "sonsoftheforest" => {}
        _ => {
            return Err(
                "Sons backup acceptance requires its explicit module-specific opt-in".into(),
            );
        }
    }
    let fixture = runtime.package.root.clone();
    let saves = PathBuf::from(&running.saves_path);
    let program = creation::program_root(running)?;
    let checkpoint = PathBuf::from(
        std::env::var_os("LANGAME_NATIVE_SONS_WORLD_ZIP")
            .ok_or("Sons backup acceptance requires the explicit task-owned checkpoint")?,
    );
    let snapshot = tokio::task::spawn_blocking(move || -> Result<Snapshot, String> {
        let _pins = guard_pin_directories(&saves)?;
        let fixture = dunce::canonicalize(fixture).map_err(|_| "Cannot resolve Sons fixture")?;
        let saves = dunce::canonicalize(saves).map_err(|_| "Cannot resolve Sons saves")?;
        let program = dunce::canonicalize(program).map_err(|_| "Cannot resolve Sons program")?;
        if !saves.starts_with(&fixture) || !program.starts_with(&fixture) {
            return Err("Sons archive acceptance escaped its disposable fixture".into());
        }
        let slot = saves.join("DedicatedServer/Multiplayer").join(SLOT);
        let log_path = program.join("_Redloader/Latest.log");
        let _log_pins = guard_pin_directories(log_path.parent().ok_or("Sons log has no parent")?)?;
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            // The same handle spans formal save through stopped verification.
            // Native append is allowed; deletion/replacement is denied.
            options.share_mode(1 | 2).custom_flags(0x0020_0000);
        }
        let mut log = options
            .open(log_path)
            .map_err(|_| "Cannot pin Sons loader log")?;
        let log_prefix = log_prefix(&read_log(&mut log)?);
        let _slot_pins = guard_pin_directories(&slot)?;
        let bytes = guard_read_regular(&slot.join("SaveData.zip"), LIMIT)?;
        let game_id = primary(&bytes)?;
        let _source_pins =
            guard_pin_directories(checkpoint.parent().ok_or("Sons checkpoint has no parent")?)?;
        if game_id != primary(&guard_read_regular(&checkpoint, LIMIT)?)? {
            return Err("Sons startup did not reload the task-owned native world identity".into());
        }
        Ok(Snapshot {
            backups: backups(&slot)?,
            slot,
            log,
            log_prefix,
            primary_sha256: hash(&bytes),
            game_id,
        })
    })
    .await
    .map_err(|_| "Sons archive capture worker failed")??;
    Ok(Some(snapshot))
}

pub(super) async fn verify_stopped(
    snapshot: Option<Snapshot>,
    cycle: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(mut snapshot) = snapshot else {
        return Ok(());
    };
    let cycle = match cycle {
        "initial" => "initial",
        "restart" => "restart",
        _ => return Err("Sons archive acceptance has an unknown cycle".into()),
    };
    let (count, created) = tokio::task::spawn_blocking(move || -> Result<(usize, usize), String> {
        let _pins = guard_pin_directories(&snapshot.slot)?;
        let stopped_primary = guard_read_regular(&snapshot.slot.join("SaveData.zip"), LIMIT)?;
        if primary(&stopped_primary)? != snapshot.game_id {
            return Err("Sons stopped primary changed native world identity".into());
        }
        let names = backups(&snapshot.slot)?;
        let created = names.difference(&snapshot.backups).count();
        if names.len() > 3 || created < 2 {
            return Err("Sons did not retain two new native recovery points within its three-backup limit".into());
        }
        let mut previous_primary_retained = false;
        for name in &names {
            let path = snapshot.slot.parent().ok_or("Invalid Sons slot")?.join(name);
            let bytes = guard_read_regular(&path, LIMIT)?;
            sons_world_fixture::validate_archive(&bytes, &hash(&bytes))?;
            let mut archive = zip::ZipArchive::new(Cursor::new(&bytes))
                .map_err(|_| "Invalid Sons backup ZIP")?;
            let mut entry = archive
                .by_name(&format!("{SLOT}/SaveData.zip"))
                .map_err(|_| "Sons backup is incompatible with native slot recovery")?;
            let mut nested = Vec::new();
            (&mut entry)
                .take(LIMIT as u64 + 1)
                .read_to_end(&mut nested)
                .map_err(|_| "Cannot read Sons backup primary")?;
            if nested.len() > LIMIT || primary(&nested)? != snapshot.game_id {
                return Err("Sons backup primary failed CRC or native world identity validation".into());
            }
            previous_primary_retained |= hash(&nested) == snapshot.primary_sha256;
        }
        if !previous_primary_retained {
            return Err("Sons backup lost the exact primary present before the formal save request".into());
        }
        verify_log(&snapshot.log_prefix, &read_log(&mut snapshot.log)?)?;
        Ok((names.len(), created))
    })
    .await
    .map_err(|_| "Sons stopped archive validation worker failed")??;
    println!(
        "NATIVE_SONS_BACKUP cycle={cycle} phase=verified primary_crc=passed backup_outer_inner_crc=passed native_slot_layout=passed loaded_world_identity=matched pre_request_primary_retained=true backups={count} created={created} formal_save_durability=not_claimed"
    );
    Ok(())
}

#[test]
fn native_sons_backup_log_excludes_only_the_preserved_preformal_prefix() {
    let before =
        b"LGSM_SONS_BACKUP_PATCH_READY\nSystem.IO.IOException expected locked-primary fault\n";
    let prefix = log_prefix(before);
    let mut healthy = before.to_vec();
    healthy.extend_from_slice(b"LGSM_SONS_BACKUP_PUBLISHED\nLGSM_SONS_BACKUP_PUBLISHED\n");
    assert!(verify_log(&prefix, &healthy).is_ok());
    let mut failed = healthy.clone();
    failed.extend_from_slice(b"System.IO.IOException unexpected formal save failure\n");
    assert!(verify_log(&prefix, &failed).is_err());
    let mut failed = healthy.clone();
    failed.extend_from_slice(b"LGSM_SONS_BACKUP_FAILED\n");
    assert!(verify_log(&prefix, &failed).is_err());
    let mut insufficient = before.to_vec();
    insufficient.extend_from_slice(b"LGSM_SONS_BACKUP_PUBLISHED\n");
    assert!(verify_log(&prefix, &insufficient).is_err());
    assert!(verify_log(&prefix, &before[..before.len() - 1]).is_err());
    healthy[0] = b'X';
    assert!(verify_log(&prefix, &healthy).is_err());
}

#[test]
fn native_sons_archive_probe_reads_nested_world_identity_and_rejects_crc_damage() {
    use std::io::Write;
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .start_file(
            "GameStateSaveData.json",
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored),
        )
        .unwrap();
    let body = serde_json::to_vec(&json!({
        "Version": 1, "Data": {"GameState": "{\"GameId\":123456}"}
    }))
    .unwrap();
    writer.write_all(&body).unwrap();
    let bytes = writer.finish().unwrap().into_inner();
    assert_eq!(primary(&bytes).unwrap(), json!(123456));
    let mut damaged = bytes.clone();
    let position = damaged
        .windows(6)
        .position(|part| part == b"123456")
        .unwrap();
    damaged[position] = b'9';
    assert!(primary(&damaged).is_err());
}

#[test]
fn native_sons_archive_probe_rejects_zip_without_native_game_state() {
    use std::io::Write;
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .start_file("unrelated.json", zip::write::SimpleFileOptions::default())
        .unwrap();
    writer.write_all(b"{}").unwrap();
    assert!(primary(&writer.finish().unwrap().into_inner()).is_err());
}
