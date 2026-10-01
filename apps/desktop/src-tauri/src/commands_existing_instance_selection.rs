use super::*;
use std::collections::BTreeSet;

#[derive(Serialize)]
pub(super) struct Options {
    pub backend_executable: PathBuf,
    pub backend_pid: u32,
    pub backend_creation_time: u64,
    pub modules_root: PathBuf,
    instance_ids: BTreeSet<String>,
    expected_modules: BTreeSet<String>,
    output_root: PathBuf,
}

impl Options {
    pub(super) fn from_env() -> Result<Self, String> {
        if std::env::var("LANGAME_EXISTING_ACCEPTANCE").as_deref()
            != Ok("bounded-parallel-start-stop-existing")
        {
            return Err(
                "set LANGAME_EXISTING_ACCEPTANCE=bounded-parallel-start-stop-existing explicitly"
                    .into(),
            );
        }
        let required = |key| std::env::var(key).map_err(|_| format!("missing {key}"));
        Ok(Self {
            backend_executable: required("LANGAME_EXISTING_BACKEND_EXE")?.into(),
            backend_pid: required("LANGAME_EXISTING_BACKEND_PID")?
                .parse()
                .map_err(|_| "invalid backend PID")?,
            backend_creation_time: required("LANGAME_EXISTING_BACKEND_CREATION_TIME")?
                .parse()
                .map_err(|_| "invalid backend creation token")?,
            modules_root: plain_directory(Path::new(&required("LANGAME_EXISTING_MODULES_ROOT")?))?,
            instance_ids: list(&required("LANGAME_EXISTING_INSTANCE_IDS")?)?,
            expected_modules: list(&required("LANGAME_EXISTING_EXPECTED_MODULES")?)?,
            output_root: required("LANGAME_EXISTING_OUTPUT_ROOT")?.into(),
        })
    }

    pub(super) fn validate_catalog(&self, descriptors: &[ModuleDescriptor]) -> Result<(), String> {
        let actual: BTreeSet<_> = descriptors
            .iter()
            .map(|item| item.summary.id.clone())
            .collect();
        if actual.len() != 32
            || self.expected_modules.is_empty()
            || !self.expected_modules.is_subset(&actual)
            || self.instance_ids.len() != self.expected_modules.len()
        {
            return Err(
                "explicit selection must identify one instance per selected module in the current 32-module catalog".into(),
            );
        }
        if !self.output_root.is_absolute()
            || !self.backend_executable.is_absolute()
            || self.backend_pid == 0
            || self.backend_creation_time == 0
        {
            return Err("absolute paths and a nonzero backend identity are required".into());
        }
        Ok(())
    }

    pub(super) fn select(
        &self,
        instances: &[InstanceSummary],
    ) -> Result<Vec<InstanceSummary>, String> {
        if instances.iter().any(|instance| !is_inactive(instance)) {
            return Err("every existing instance must be inactive before acceptance".into());
        }
        let mut selected: Vec<_> = instances
            .iter()
            .filter(|item| self.instance_ids.contains(&item.id))
            .cloned()
            .collect();
        let modules: BTreeSet<_> = selected.iter().map(|item| item.module_id.clone()).collect();
        if self.expected_modules.is_empty()
            || selected.len() != self.expected_modules.len()
            || modules != self.expected_modules
        {
            return Err(
                "instance IDs must select exactly one instance of each expected module".into(),
            );
        }
        selected.sort_by(|left, right| left.module_id.cmp(&right.module_id));
        Ok(selected)
    }

    pub(super) fn create_output_directory(&self) -> Result<PathBuf, String> {
        // Never write receipts into a package, save tree, or the source checkout.
        // The caller supplies an isolated acceptance work directory explicitly.
        let work = self
            .output_root
            .ancestors()
            .any(|path| path.file_name().is_some_and(|name| name == "work"));
        if !work
            || self
                .output_root
                .components()
                .any(|part| matches!(part, std::path::Component::ParentDir))
        {
            return Err("acceptance output must be beneath an explicit task work directory".into());
        }
        for ancestor in self.output_root.ancestors() {
            if let Ok(metadata) = std::fs::symlink_metadata(ancestor) {
                use std::os::windows::fs::MetadataExt;
                if !metadata.is_dir() || metadata.file_attributes() & 0x400 != 0 {
                    return Err("acceptance output ancestors must be plain directories".into());
                }
            }
        }
        std::fs::create_dir_all(&self.output_root)
            .map_err(|_| "cannot create acceptance output root")?;
        let root = dunce::canonicalize(&self.output_root)
            .map_err(|_| "cannot resolve acceptance output root")?;
        let workspace = dunce::canonicalize(super::super::workspace_root())
            .map_err(|_| "cannot resolve source root")?;
        if root.starts_with(&workspace) {
            return Err("acceptance output cannot be inside the repository".into());
        }
        let directory = root.join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir(&directory)
            .map_err(|_| "cannot exclusively create acceptance run directory")?;
        Ok(directory)
    }
}

fn list(value: &str) -> Result<BTreeSet<String>, String> {
    let mut result = BTreeSet::new();
    for value in value.split(',') {
        let value = value.trim();
        if value.is_empty()
            || value.len() > 128
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
            || !result.insert(value.to_owned())
        {
            return Err("selection lists require unique, nonempty IDs separated by commas".into());
        }
    }
    Ok(result)
}

fn plain_directory(path: &Path) -> Result<PathBuf, String> {
    use std::os::windows::fs::MetadataExt;
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err("selected modules root must be an absolute plain directory".into());
    }
    for ancestor in path.ancestors() {
        let metadata = std::fs::symlink_metadata(ancestor)
            .map_err(|_| "selected modules root is unavailable")?;
        if !metadata.is_dir() || metadata.file_attributes() & 0x400 != 0 {
            return Err("selected modules root cannot traverse a reparse point".into());
        }
    }
    dunce::canonicalize(path).map_err(|_| "selected modules root cannot be resolved".into())
}

// A failed run may be retried without erasing its Error history. Callers also
// verify active_run is absent before admission; normal-stop evidence stays strict.
pub(super) fn is_inactive(summary: &InstanceSummary) -> bool {
    matches!(
        summary.status,
        InstanceStatus::Stopped | InstanceStatus::Error
    ) && summary.active_process_count == 0
}

pub(super) fn is_stopped(summary: &InstanceSummary) -> bool {
    matches!(summary.status, InstanceStatus::Stopped) && summary.active_process_count == 0
}

pub(super) fn preconditions(
    instance: &InstanceDetails,
    smoke: &SmokeManifest,
) -> Result<(), String> {
    if !matches!(instance.summary.bind_ip.as_str(), "0.0.0.0" | "127.0.0.1") {
        return Err("existing native probes require a loopback-reachable binding; settings were not changed".into());
    }
    let settings: serde_json::Value = serde_json::from_str(&instance.settings_json)
        .map_err(|_| "invalid stored instance settings")?;
    for condition in &smoke.preconditions {
        match condition.kind.as_str() {
            "operator_acknowledgement" => {
                if condition
                    .setting_key
                    .as_deref()
                    .and_then(|key| settings.get(key))
                    .and_then(serde_json::Value::as_bool)
                    != Some(true)
                {
                    return Err(format!("stored_precondition_missing:{}", condition.id));
                }
            }
            "secret" => {
                let offline = instance.summary.module_id == "dontstarve"
                    && settings
                        .get("offline_cluster")
                        .and_then(serde_json::Value::as_bool)
                        == Some(true);
                if !offline
                    && condition
                        .setting_key
                        .as_deref()
                        .and_then(|key| settings.get(key))
                        .and_then(serde_json::Value::as_str)
                        .is_none_or(|value| value.trim().is_empty())
                {
                    return Err(format!("stored_precondition_missing:{}", condition.id));
                }
            }
            // The real backend owns normal elevation admission. No test
            // relaunch, impersonation, token change, or bypass is allowed.
            "elevation" => {}
            _ => return Err("unsupported existing-instance prerequisite".into()),
        }
    }
    Ok(())
}

#[test]
fn existing_acceptance_selection_rejects_duplicates_and_paths() {
    assert!(list("a,a").is_err());
    assert!(list("a,,b").is_err());
    assert!(list("../instance").is_err());
    assert!(list("module-a,module_b").is_ok());
}

#[test]
fn existing_acceptance_selection_requires_explicit_stopped_instances() {
    let mut instances: Vec<_> = (0..32)
        .map(|index| InstanceSummary {
            id: format!("instance-{index}"),
            name: "fixture".into(),
            module_id: format!("module-{index:02}"),
            status: InstanceStatus::Stopped,
            active_process_count: 0,
            bind_ip: "0.0.0.0".into(),
            port_count: 1,
            autostart: false,
        })
        .collect();
    let mut options = Options {
        backend_executable: PathBuf::from(r"C:\fixture\backend.exe"),
        backend_pid: 1,
        backend_creation_time: 1,
        instance_ids: instances.iter().map(|item| item.id.clone()).collect(),
        modules_root: PathBuf::from(r"C:\fixture\release\modules"),
        expected_modules: instances
            .iter()
            .map(|item| item.module_id.clone())
            .collect(),
        output_root: PathBuf::from(r"C:\fixture\work\acceptance"),
    };
    instances.reverse();
    let selected = options.select(&instances).unwrap();
    assert_eq!(selected[0].module_id, "module-00");
    assert_eq!(selected[31].module_id, "module-31");
    // A resumed campaign selects only its remaining instances. Unselected
    // running instances still fail the initial global ownership boundary.
    options.instance_ids = ["instance-0".into()].into();
    options.expected_modules = ["module-00".into()].into();
    assert_eq!(options.select(&instances).unwrap().len(), 1);
    instances[0].status = InstanceStatus::Error;
    assert!(options.select(&instances).is_ok());
    assert!(!is_stopped(&instances[0]));
    instances[0].active_process_count = 1;
    assert!(options.select(&instances).is_err());
    instances[0].active_process_count = 0;
    instances[0].status = InstanceStatus::Running;
    assert!(options.select(&instances).is_err());
    instances[0].status = InstanceStatus::Stopped;
    instances[0].active_process_count = 1;
    assert!(options.select(&instances).is_err());
    instances[0].active_process_count = 0;
    options.instance_ids = instances.iter().map(|item| item.id.clone()).collect();
    options.expected_modules = instances
        .iter()
        .map(|item| item.module_id.clone())
        .collect();
    instances[0].module_id = instances[1].module_id.clone();
    assert!(options.select(&instances).is_err());
    options.instance_ids.clear();
    options.expected_modules.clear();
    assert!(options.select(&instances).is_err());
}
