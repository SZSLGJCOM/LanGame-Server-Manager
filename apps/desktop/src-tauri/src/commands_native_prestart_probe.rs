#![cfg(test)]

use super::*;

const MARKER: &str = "native_acceptance.prestart";
const PREFIX: &str = "instance.prestart_update.";

pub(super) struct PrestartProbe {
    marker: String,
    expected: &'static [&'static str],
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
            install.download_url_windows.is_none()
                && (descriptor.summary.steam_app_id.is_some_and(|id| id > 0)
                    || install.source == Some(app_core::InstallSource::MinecraftJava))
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
        Ok(Self { marker, expected })
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
        println!(
            "NATIVE_LIFECYCLE module={} phase=prestart_update cycle={cycle} events={} persisted_evidence=verified download_bytes=not_measured",
            instance.summary.module_id,
            self.expected.join(","),
        );
        Ok(())
    }
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
