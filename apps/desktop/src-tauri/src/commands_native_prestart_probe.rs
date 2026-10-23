#![cfg(test)]

use super::*;

const MARKER: &str = "native_acceptance.prestart";
const PREFIX: &str = "instance.prestart_update.";

pub(super) struct PrestartProbe {
    marker: String,
    expected: &'static [&'static str],
    archive_update: Option<RimworldArchiveUpdateProbe>,
}

impl PrestartProbe {
    pub(super) fn begin(
        storage: &StorageBootstrap,
        instance: &InstanceDetails,
        descriptor: &ModuleDescriptor,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let policy =
            app_core::InstanceProgramUpdatePolicy::from_settings_json(&instance.settings_json)?;
        let supports_update = descriptor.install.as_ref().is_some_and(|install| {
            if install.download_url_windows.is_some() {
                install.download_integrity_windows.is_some()
            } else {
                descriptor.summary.steam_app_id.is_some_and(|id| id > 0)
                    || install.source == Some(app_core::InstallSource::MinecraftJava)
            }
        });
        let expected: &[&str] = if policy == app_core::InstanceProgramUpdatePolicy::Pinned {
            &["pinned"]
        } else if supports_update {
            &["started", "completed"]
        } else {
            &["unavailable"]
        };
        let marker = uuid::Uuid::new_v4().to_string();
        // A fresh persisted marker excludes prior cycles without depending on
        // wall-clock resolution or retaining unrelated diagnostic contents.
        crate::desktop_app_log::append(
            &storage.paths.app_log_path(),
            &json!({"action": MARKER, "context": {"probe_id": marker}}),
        )?;
        let archive_update = RimworldArchiveUpdateProbe::begin(storage, instance, descriptor)?;
        Ok(Self {
            marker,
            expected,
            archive_update,
        })
    }

    pub(super) fn verify(
        self,
        storage: &StorageBootstrap,
        instance: &InstanceDetails,
        cycle: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let entries = crate::desktop_app_log::recent_lines(
            &storage.paths.app_log_path(),
            None,
            512,
            1024 * 1024,
        )?
        .iter()
        .map(|line| serde_json::from_str::<Value>(line))
        .collect::<Result<Vec<_>, _>>()?;
        verify_events(&entries, &self.marker, &instance.summary.id, self.expected)?;
        if let Some(probe) = self.archive_update {
            probe.verify()?;
        }
        println!(
            "NATIVE_LIFECYCLE module={} phase=prestart_update cycle={cycle} events={} persisted_evidence=verified download_bytes=not_measured",
            instance.summary.module_id,
            self.expected.join(","),
        );
        Ok(())
    }
}

/// Opt-in evidence for a real old official ZIP passing through the production
/// startup updater. The surrounding native fixture owns every touched path.
struct RimworldArchiveUpdateProbe {
    executable: PathBuf,
    expected_new: String,
    retained: Vec<(PathBuf, String)>,
    native_config: PathBuf,
    configuration: serde_json::Map<String, Value>,
}

impl RimworldArchiveUpdateProbe {
    fn begin(
        storage: &StorageBootstrap,
        instance: &InstanceDetails,
        descriptor: &ModuleDescriptor,
    ) -> Result<Option<Self>, Box<dyn std::error::Error>> {
        let old = std::env::var("LANGAME_NATIVE_RIMWORLD_UPDATE_OLD_SHA256").ok();
        let new = std::env::var("LANGAME_NATIVE_RIMWORLD_UPDATE_NEW_SHA256").ok();
        if old.is_none() && new.is_none() {
            return Ok(None);
        }
        let (Some(expected_old), Some(expected_new)) = (old, new) else {
            return Err("native archive update requires both verified executable hashes".into());
        };
        if descriptor.summary.id != "rimworld"
            || app_core::InstanceProgramUpdatePolicy::from_settings_json(&instance.settings_json)?
                != app_core::InstanceProgramUpdatePolicy::Automatic
            || expected_old == expected_new
            || [&expected_old, &expected_new]
                .iter()
                .any(|hash| hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
        {
            return Err(
                "native archive update requires one automatic RimWorld version transition".into(),
            );
        }
        let scratch = PathBuf::from(
            std::env::var_os("LANGAME_SMOKE_RUNTIME_ROOT")
                .ok_or("native archive update requires an explicit disposable root")?,
        );
        let fixture = seed_probe::checked_path(&scratch, &storage.paths.games_root, true)?;
        let program = seed_probe::checked_path(&fixture, &creation::program_root(instance)?, true)?;
        let instance_root =
            seed_probe::checked_path(&fixture, creation::instance_root(instance)?, true)?;
        let executable = seed_probe::checked_path(&fixture, &program.join("RTServer.exe"), false)?;
        if file_sha256(&executable)? != expected_old.to_ascii_lowercase()
            || !app_storage::archive_program_has_baseline(&program, descriptor)?
        {
            return Err(
                "native archive update did not begin with its verified old program and inventory"
                    .into(),
            );
        }
        let mut retained = Vec::new();
        for relative in [
            "Assets/update-preservation.fixture",
            "unknown-mods/update-preservation.fixture",
            "Configs/update-preservation.fixture",
        ] {
            let path = program.join(relative);
            let parent = path
                .parent()
                .ok_or("native archive sentinel has no parent")?;
            fs::create_dir_all(parent)?;
            seed_probe::checked_path(&fixture, parent, true)?;
            use std::io::Write;
            let mut file = fs::File::create_new(&path)?;
            file.write_all(b"native archive update must preserve this operator data\n")?;
            drop(file);
            retained.push((path.clone(), file_sha256(&path)?));
        }
        let mut ownership = 0;
        let runtime_root = instance_root.join("runtime");
        for root in [&program, &runtime_root] {
            for name in [
                ".langame-program-identity.json",
                ".langame-program-usage.json",
                ".langame-private-runtime",
                ".langame-exclusive-program.json",
                ".langame-shared-program.json",
            ] {
                let path = root.join(name);
                if path.try_exists()? && !retained.iter().any(|(existing, _)| existing == &path) {
                    seed_probe::checked_path(&fixture, &path, false)?;
                    retained.push((path.clone(), file_sha256(&path)?));
                    ownership += 1;
                }
            }
        }
        if ownership == 0 {
            return Err("native archive update has no runtime ownership evidence".into());
        }
        let config =
            seed_probe::checked_path(&fixture, Path::new(&instance.config_file_path), false)?;
        retained.push((config.clone(), file_sha256(&config)?));
        let native_config =
            seed_probe::checked_path(&fixture, &program.join("Configs/ServerConfig.json"), false)?;
        let configuration = serde_json::from_slice(&fs::read(&native_config)?)?;
        println!(
            "NATIVE_ARCHIVE_UPDATE phase=before module=rimworld executable_sha256={expected_old} expected_sha256={expected_new} retained_files={} ownership_records={ownership}",
            retained.len()
        );
        Ok(Some(Self {
            executable,
            expected_new: expected_new.to_ascii_lowercase(),
            retained,
            native_config,
            configuration,
        }))
    }

    fn verify(self) -> Result<(), Box<dyn std::error::Error>> {
        if file_sha256(&self.executable)? != self.expected_new {
            return Err(
                "native archive update did not install the verified current executable".into(),
            );
        }
        for (path, digest) in &self.retained {
            if file_sha256(path)? != *digest {
                return Err("native archive update changed retained data or ownership".into());
            }
        }
        // The upstream server may add newly introduced default fields. Every
        // pre-existing configured value must still be present and unchanged.
        let actual: serde_json::Map<String, Value> =
            serde_json::from_slice(&fs::read(&self.native_config)?)?;
        if self
            .configuration
            .iter()
            .any(|(key, value)| actual.get(key) != Some(value))
        {
            return Err(
                "native archive update changed existing native configuration values".into(),
            );
        }
        println!(
            "NATIVE_ARCHIVE_UPDATE phase=after module=rimworld executable_sha256={} retained_files={} native_configuration=preserved ownership_metadata=preserved",
            self.expected_new,
            self.retained.len()
        );
        Ok(())
    }
}

fn file_sha256(path: &Path) -> Result<String, std::io::Error> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut file = fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn verify_events(
    entries: &[Value],
    marker: &str,
    instance_id: &str,
    expected: &[&str],
) -> Result<(), String> {
    let start = entries
        .iter()
        .position(|entry| entry["action"] == MARKER && entry["context"]["probe_id"] == marker)
        .ok_or("native prestart evidence lost its current-cycle marker")?;
    let observed = entries[start + 1..]
        .iter()
        .filter(|entry| {
            entry["context"]["instance_id"] == instance_id && entry["context"]["source"] == "manual"
        })
        .filter_map(|entry| entry["action"].as_str()?.strip_prefix(PREFIX))
        .collect::<Vec<_>>();
    if observed != expected {
        return Err(format!(
            "native prestart evidence differs: expected={expected:?} observed={observed:?}"
        ));
    }
    Ok(())
}

#[test]
fn prestart_probe_requires_current_instance_ordered_completion() {
    let event = |instance, outcome: &str| {
        json!({"action": format!("{PREFIX}{outcome}"),
            "context": {"instance_id": instance, "source": "manual"}})
    };
    let marker = json!({"action": MARKER, "context": {"probe_id": "current"}});
    let entries = vec![
        event("sample", "started"),
        event("sample", "completed"),
        marker.clone(),
        event("peer", "started"),
        event("peer", "completed"),
    ];
    assert!(verify_events(&entries, "current", "sample", &["started", "completed"]).is_err());
    for outcomes in [
        vec!["started"],
        vec!["completed", "started"],
        vec!["started", "failed"],
        vec!["started", "completed", "completed"],
    ] {
        let mut entries = vec![marker.clone()];
        entries.extend(outcomes.into_iter().map(|outcome| event("sample", outcome)));
        assert!(verify_events(&entries, "current", "sample", &["started", "completed"]).is_err());
    }
    let mut completed = entries;
    completed.extend([event("sample", "started"), event("sample", "completed")]);
    verify_events(&completed, "current", "sample", &["started", "completed"]).unwrap();
    assert!(verify_events(&completed, "missing", "sample", &["started", "completed"]).is_err());
}

#[test]
fn prestart_probe_distinguishes_pinned_and_unsupported_sources() {
    for outcome in ["pinned", "unavailable"] {
        let entries = vec![
            json!({"action": MARKER, "context": {"probe_id": "current"}}),
            json!({"action": format!("{PREFIX}{outcome}"),
                "context": {"instance_id": "sample", "source": "manual"}}),
        ];
        verify_events(&entries, "current", "sample", &[outcome]).unwrap();
        assert!(verify_events(&entries, "current", "sample", &["started", "completed"]).is_err());
    }
}
