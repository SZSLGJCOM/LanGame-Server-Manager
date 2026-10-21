//! Opt-in provenance for disposable native smoke assets. A manifest is not
//! deletion authority: the registered consumer still pins and verifies leaves.
use std::fs::{self, File};
use std::io;
use std::path::{Component, Path, PathBuf};

use super::smoke_resource_copy_preflight::{SmokeCopyPreflight, read_policy};
use serde_json::{Value, json};

const MAX_COPIES: usize = 512;
const MAX_MANIFEST_BYTES: usize = 256 * 1024;
const POLICY_ENV: &str = "LANGAME_SMOKE_RESOURCE_POLICY";

#[derive(Clone, Copy)]
pub(super) enum CopyProfile {
    ServerAssets,
    WorkshopAssets,
}

impl CopyProfile {
    fn name(self) -> &'static str {
        match self {
            Self::ServerAssets => "dst-server-assets",
            Self::WorkshopAssets => "dst-workshop-assets",
        }
    }
}

pub(super) struct SmokeResourceCopyManifest {
    path: PathBuf,
    root: PathBuf,
    value: Value,
    published: Vec<u8>,
    preflight: SmokeCopyPreflight,
}

impl SmokeResourceCopyManifest {
    /// Unconfigured callers keep unmanaged smoke behavior. An explicitly
    /// configured policy must validate before any registration is created.
    pub(super) fn begin(root: &Path) -> io::Result<Option<Self>> {
        let policy = std::env::var_os(POLICY_ENV).map(PathBuf::from);
        Self::begin_with_policy(root, policy.as_deref())
    }

    fn begin_with_policy(root: &Path, policy: Option<&Path>) -> io::Result<Option<Self>> {
        let Some(policy) = policy else {
            return Ok(None);
        };
        if !normal_absolute(policy) {
            return Err(invalid(
                "smoke resource policy must be a normal absolute path",
            ));
        }
        let registry = registry_for(root, &read_policy(policy)?)?;
        Self::begin_in(root, &registry, policy).map(Some)
    }

    pub(super) fn copy_preflight(&self) -> SmokeCopyPreflight {
        self.preflight.clone()
    }

    fn begin_in(root: &Path, registry: &Path, policy: &Path) -> io::Result<Self> {
        require_plain_directory(root)?;
        let identity = directory_identity(root)?;
        let owner_start = current_process_start()?;
        let now = utc_now();
        let id = uuid::Uuid::new_v4().simple().to_string();
        let value = json!({
            "schemaVersion": 1,
            "kind": "managed-game-resource-copy",
            "producer": "langame-dst-native-smoke",
            "id": id,
            "state": "active",
            "consumerContract": "sealed-one-shot-v1",
            "ownerPid": std::process::id(),
            "ownerStartFileTime": owner_start,
            "createdAtUtc": now,
            "lastUsedAtUtc": now,
            "completedAtUtc": null,
            "activityRoot": path_text(root)?,
            "activityRootIdentity": identity,
            "copies": [],
        });
        let published = bounded_json(&value)?;
        create_plain_directory_chain(registry)?;
        let path = registry.join(format!("{id}.json"));
        app_storage::create_file_atomically(&path, &published)?;
        Ok(Self {
            path,
            root: root.to_owned(),
            value,
            published,
            preflight: SmokeCopyPreflight::new(root, policy),
        })
    }

    /// Called only after the actual independent copy has succeeded.
    pub(super) fn record_copy(
        &mut self,
        profile: CopyProfile,
        source: &Path,
        target: &Path,
    ) -> io::Result<()> {
        self.record_pairs(profile, &[(source.to_owned(), target.to_owned())])
    }

    /// Workshop destinations are known before deployment. Register them first
    /// so a failed partial copy remains discoverable. A declaration never
    /// bypasses the consumer's per-leaf source/identity/hash validation.
    pub(super) fn record_workshop_items(
        &mut self,
        source: &Path,
        target: &Path,
        ids: &[String],
    ) -> io::Result<()> {
        if ids.is_empty() || ids.len() > MAX_COPIES {
            return Err(invalid("invalid Workshop item count"));
        }
        let pairs = ids
            .iter()
            .map(|id| {
                if id.is_empty() || id.len() > 20 || !id.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(invalid("invalid Workshop item ID"));
                }
                let relative = Path::new("content/322330").join(id);
                Ok((source.join(&relative), target.join(relative)))
            })
            .collect::<io::Result<Vec<_>>>()?;
        self.record_pairs(CopyProfile::WorkshopAssets, &pairs)
    }

    fn record_pairs(
        &mut self,
        profile: CopyProfile,
        pairs: &[(PathBuf, PathBuf)],
    ) -> io::Result<()> {
        let mut next = self.active_value()?;
        let copies = next["copies"]
            .as_array_mut()
            .ok_or_else(|| invalid("invalid copies"))?;
        for (source, target) in pairs {
            require_plain_directory(source)?;
            require_plain_or_missing_directory(target)?;
            if !strict_descendant(target, &self.root) {
                return Err(invalid("resource copy target is outside its activity root"));
            }
            let entry = json!({"profile": profile.name(), "sourceRoot": path_text(source)?, "targetRoot": path_text(target)?});
            if !copies.contains(&entry) {
                if copies.len() >= MAX_COPIES {
                    return Err(invalid("too many resource copy mappings"));
                }
                copies.push(entry);
            }
        }
        self.publish(next)
    }

    /// The caller reaches this only after all native stop checks and the final
    /// evidence write succeed. Drop deliberately never changes the manifest.
    pub(super) fn complete(&mut self) -> io::Result<()> {
        let mut next = self.active_value()?;
        if next["copies"]
            .as_array()
            .is_none_or(|copies| copies.is_empty())
        {
            return Err(invalid(
                "cannot complete a smoke resource manifest without copy mappings",
            ));
        }
        next["state"] = json!("completed");
        next["completedAtUtc"] = json!(utc_now());
        self.publish(next)
    }

    fn active_value(&self) -> io::Result<Value> {
        if self.value["state"] != "active" {
            return Err(invalid("completed smoke resource root is sealed"));
        }
        require_plain_directory(&self.root)?;
        if directory_identity(&self.root)? != self.value["activityRootIdentity"] {
            return Err(invalid("smoke activity root identity changed"));
        }
        Ok(self.value.clone())
    }

    fn publish(&mut self, mut next: Value) -> io::Result<()> {
        let now = utc_now();
        // A backwards wall-clock adjustment must not make a root appear older.
        next["lastUsedAtUtc"] = json!(
            now.as_str()
                .max(self.value["lastUsedAtUtc"].as_str().unwrap())
        );
        if next["state"] == "completed" {
            next["completedAtUtc"] = next["lastUsedAtUtc"].clone();
        }
        require_plain_directory(
            self.path
                .parent()
                .ok_or_else(|| invalid("missing registry"))?,
        )?;
        let bytes = bounded_json(&next)?;
        if !app_storage::compare_and_swap_file_atomically(&self.path, &self.published, &bytes)? {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "smoke resource manifest changed; existing record preserved",
            ));
        }
        self.value = next;
        self.published = bytes;
        Ok(())
    }
}

fn bounded_json(value: &Value) -> io::Result<Vec<u8>> {
    let bytes = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(invalid("smoke resource manifest exceeds 256 KiB"));
    }
    Ok(bytes)
}

fn path_text(path: &Path) -> io::Result<String> {
    if !normal_absolute(path) {
        return Err(invalid("resource path must be a normal absolute path"));
    }
    let normalized = path.components().collect::<PathBuf>();
    normalized
        .to_str()
        .map(|text| text.replace('/', "\\"))
        .ok_or_else(|| invalid("resource path must be UTF-8"))
}

fn normal_absolute(path: &Path) -> bool {
    let Some(text) = path.to_str() else {
        return false;
    };
    path.is_absolute()
        && !text.starts_with(r"\\?\")
        && text.split(['\\', '/']).all(|part| {
            part != "."
                && part != ".."
                && !part.ends_with([' ', '.'])
                && !part.contains(['*', '?', '"', '\0'])
        })
        && path
            .components()
            .all(|part| !matches!(part, Component::ParentDir | Component::CurDir))
}

fn strict_descendant(path: &Path, root: &Path) -> bool {
    if !normal_absolute(path) || !normal_absolute(root) {
        return false;
    }
    let path = path.components().collect::<Vec<_>>();
    let root = root.components().collect::<Vec<_>>();
    path.len() > root.len()
        && path
            .iter()
            .zip(root.iter())
            .all(|(left, right)| left.as_os_str().eq_ignore_ascii_case(right.as_os_str()))
}

fn registry_for(root: &Path, policy: &Value) -> io::Result<PathBuf> {
    let mappings = policy["managedRoots"]
        .as_array()
        .filter(|roots| !roots.is_empty() && roots.len() <= 32)
        .ok_or_else(|| invalid("smoke resource policy requires 1-32 managedRoots"))?;
    let mut roots = Vec::new();
    for mapping in mappings {
        if mapping.as_object().is_none_or(|fields| fields.len() != 2) {
            return Err(invalid(
                "managed root accepts only activityRoot and registryRoot",
            ));
        }
        let activity = mapping["activityRoot"]
            .as_str()
            .map(PathBuf::from)
            .ok_or_else(|| invalid("managed root requires activityRoot"))?;
        let registry = mapping["registryRoot"]
            .as_str()
            .map(PathBuf::from)
            .ok_or_else(|| invalid("managed root requires registryRoot"))?;
        if !normal_absolute(&activity) || !normal_absolute(&registry) {
            return Err(invalid("managed roots must be normal absolute paths"));
        }
        roots.push((activity, registry));
    }
    for (index, (activity, registry)) in roots.iter().enumerate() {
        for (other_index, (other, _)) in roots.iter().enumerate() {
            if paths_overlap(registry, other) {
                return Err(invalid(
                    "resource registry must be outside every activity root",
                ));
            }
            if index != other_index && paths_overlap(activity, other) {
                return Err(invalid("managed activity roots must not overlap"));
            }
        }
    }
    let (activity, registry) = roots
        .into_iter()
        .find(|(activity, _)| strict_descendant(root, activity))
        .ok_or_else(|| invalid("smoke activity is outside the configured managed roots"))?;
    require_plain_directory(&activity)?;
    require_plain_or_missing_directory(&registry)?;
    Ok(registry)
}

fn paths_overlap(left: &Path, right: &Path) -> bool {
    (left.components().count() == right.components().count()
        && left
            .components()
            .zip(right.components())
            .all(|(left, right)| left.as_os_str().eq_ignore_ascii_case(right.as_os_str())))
        || strict_descendant(left, right)
        || strict_descendant(right, left)
}

fn require_plain_directory(path: &Path) -> io::Result<()> {
    if !normal_absolute(path) {
        return Err(invalid("expected a normal absolute directory"));
    }
    for ancestor in path.ancestors() {
        let metadata = fs::symlink_metadata(ancestor)?;
        #[cfg(windows)]
        let reparse = {
            use std::os::windows::fs::MetadataExt;
            metadata.file_attributes() & 0x400 != 0
        };
        #[cfg(not(windows))]
        let reparse = metadata.file_type().is_symlink();
        if reparse || !metadata.is_dir() {
            return Err(invalid(
                "resource directory or ancestor is not a plain directory",
            ));
        }
    }
    Ok(())
}

fn require_plain_or_missing_directory(path: &Path) -> io::Result<()> {
    if !normal_absolute(path) {
        return Err(invalid("expected a normal absolute directory"));
    }
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(_) => return require_plain_directory(ancestor),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Err(invalid("resource target has no existing plain ancestor"))
}

fn create_plain_directory_chain(path: &Path) -> io::Result<()> {
    let mut chain = path.ancestors().collect::<Vec<_>>();
    chain.reverse();
    for directory in chain {
        match fs::symlink_metadata(directory) {
            Ok(_) => require_plain_directory(directory)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                match fs::create_dir(directory) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(error),
                }
                require_plain_directory(directory)?;
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[cfg(windows)]
fn directory_identity(path: &Path) -> io::Result<Value> {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };
    let file = File::options()
        .access_mode(0x80)
        .share_mode(7)
        .custom_flags(0x0200_0000 | 0x0020_0000)
        .open(path)?;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the owned file handle and correctly sized output live across the call.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if info.dwFileAttributes & 0x400 != 0 || info.dwFileAttributes & 0x10 == 0 {
        return Err(invalid("activity root handle is not a plain directory"));
    }
    let index = (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow);
    Ok(
        json!({"scheme":"win32-file-index64", "volumeSerialHex8":format!("{:08x}", info.dwVolumeSerialNumber), "fileIdHex16":format!("{index:016x}")}),
    )
}

#[cfg(windows)]
fn current_process_start() -> io::Result<i64> {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
    let mut created = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: the current-process pseudo handle is valid and outputs are writable.
    if unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut created,
            &mut exit,
            &mut kernel,
            &mut user,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let value = (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime);
    i64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| invalid("invalid owner creation time"))
}

#[cfg(windows)]
fn utc_now() -> String {
    use windows_sys::Win32::Foundation::SYSTEMTIME;
    use windows_sys::Win32::System::SystemInformation::GetSystemTime;
    let mut time = SYSTEMTIME::default();
    // SAFETY: GetSystemTime initializes the supplied SYSTEMTIME.
    unsafe { GetSystemTime(&mut time) };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        time.wYear,
        time.wMonth,
        time.wDay,
        time.wHour,
        time.wMinute,
        time.wSecond,
        time.wMilliseconds
    )
}

#[cfg(not(windows))]
fn directory_identity(_path: &Path) -> io::Result<Value> {
    Err(invalid("Windows smoke resource identity required"))
}
#[cfg(not(windows))]
fn current_process_start() -> io::Result<i64> {
    Err(invalid("Windows smoke process identity required"))
}
#[cfg(not(windows))]
fn utc_now() -> String {
    String::new()
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn serialized_paths_are_canonical_for_the_registry_reader() {
        assert_eq!(
            path_text(Path::new("C:/temporary\\source/assets//")).unwrap(),
            r"C:\temporary\source\assets"
        );
    }

    #[test]
    fn completion_without_copy_mappings_preserves_active_record() {
        let root = fixture();
        let activity = root.join("activity");
        fs::create_dir(&activity).unwrap();
        let mut manifest = SmokeResourceCopyManifest::begin_in(
            &activity,
            &root.join("registry"),
            &root.join("policy.json"),
        )
        .unwrap();
        let before = fs::read(&manifest.path).unwrap();
        assert!(manifest.complete().is_err());
        assert_eq!(manifest.value["state"], "active");
        assert_eq!(fs::read(&manifest.path).unwrap(), before);
    }

    fn fixture() -> PathBuf {
        let temporary = std::env::var_os("LANGAME_SMOKE_RESOURCE_TEST_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("WINDIR").expect("Windows directory")).join("Temp")
            });
        let root = temporary.join(format!(
            "lg-resource-manifest-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir(&root).unwrap();
        root
    }

    #[test]
    fn only_configured_strict_children_enroll_and_ambiguous_roots_are_rejected() {
        let root = fixture();
        let activity = root.join("runs");
        let registry = root.join("registry");
        fs::create_dir(&activity).unwrap();
        let mapping = json!({"activityRoot": activity, "registryRoot": registry});
        let policy = json!({"managedRoots": [mapping.clone()]});
        assert_eq!(
            registry_for(&activity.join("run"), &policy).unwrap(),
            registry
        );
        for target in [
            &activity,
            &root.join("runs-other/run"),
            &activity.join("../other"),
            &activity.join("run."),
        ] {
            assert!(
                registry_for(target, &policy).is_err(),
                "{}",
                target.display()
            );
        }
        for mappings in [
            json!([]),
            json!([mapping.clone(), mapping]),
            json!([{"activityRoot": activity, "registryRoot": activity.join("registry")}]),
            json!([{"activityRoot": "relative", "registryRoot": registry}]),
            json!([{"activityRoot": activity, "registryRoot": registry, "unknown": true}]),
        ] {
            assert!(
                registry_for(&activity.join("run"), &json!({"managedRoots": mappings})).is_err()
            );
        }
    }

    #[test]
    fn unmanaged_begin_never_creates_a_registry() {
        let root = fixture();
        assert!(
            SmokeResourceCopyManifest::begin_with_policy(&root, None)
                .unwrap()
                .is_none()
        );
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
    }

    #[test]
    fn configured_policy_is_required_and_registration_reuses_its_capacity_floor() {
        let root = fixture();
        let activity = root.join("runs");
        let run = activity.join("run");
        let registry = root.join("registry");
        let policy = root.join("policy.json");
        fs::create_dir_all(&run).unwrap();
        assert!(SmokeResourceCopyManifest::begin_with_policy(&run, Some(&policy)).is_err());
        fs::write(&policy, b"{}").unwrap();
        assert!(SmokeResourceCopyManifest::begin_with_policy(&run, Some(&policy)).is_err());
        assert!(!registry.exists());
        let value = json!({"schemaVersion": 1, "minimumFreeSpaceBytes": 100,
            "managedRoots": [{"activityRoot": activity, "registryRoot": registry}]});
        fs::write(&policy, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(SmokeResourceCopyManifest::begin_with_policy(&root, Some(&policy)).is_err());
        assert!(!registry.exists());
        let manifest = SmokeResourceCopyManifest::begin_with_policy(&run, Some(&policy))
            .unwrap()
            .unwrap();
        assert!(manifest.path.starts_with(&registry));
        let source = root.join("source");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("asset"), b"fixture").unwrap();
        manifest
            .copy_preflight()
            .require_capacity(std::slice::from_ref(&source))
            .unwrap();
        fs::write(&policy, b"{}").unwrap();
        assert!(
            manifest
                .copy_preflight()
                .require_capacity(&[source])
                .is_err()
        );
    }

    #[test]
    fn schema_atomic_active_copy_and_sealed_completion() {
        let root = fixture();
        let activity = root.join("activity");
        let source = root.join("source");
        let target = activity.join("instance/runtime");
        fs::create_dir_all(&target).unwrap();
        fs::create_dir(&source).unwrap();
        let mut manifest = SmokeResourceCopyManifest::begin_in(
            &activity,
            &root.join("registry"),
            &root.join("policy.json"),
        )
        .unwrap();
        let first: Value = serde_json::from_slice(&fs::read(&manifest.path).unwrap()).unwrap();
        assert_eq!(first["kind"], "managed-game-resource-copy");
        assert_eq!(first["producer"], "langame-dst-native-smoke");
        assert_eq!(first["consumerContract"], "sealed-one-shot-v1");
        assert_eq!(first["schemaVersion"], 1);
        assert_eq!(first["ownerPid"], std::process::id());
        assert_eq!(
            first["ownerStartFileTime"],
            current_process_start().unwrap()
        );
        assert_eq!(first["state"], "active");
        assert!(first["completedAtUtc"].is_null());
        assert_eq!(
            first["activityRootIdentity"]["scheme"],
            "win32-file-index64"
        );
        assert_eq!(
            first["activityRootIdentity"]["volumeSerialHex8"]
                .as_str()
                .unwrap()
                .len(),
            8
        );
        assert_eq!(
            first["activityRootIdentity"]["fileIdHex16"]
                .as_str()
                .unwrap()
                .len(),
            16
        );
        manifest
            .record_copy(CopyProfile::ServerAssets, &source, &target)
            .unwrap();
        manifest
            .record_copy(CopyProfile::ServerAssets, &source, &target)
            .unwrap();
        assert_eq!(manifest.value["copies"].as_array().unwrap().len(), 1);
        assert_eq!(manifest.value["copies"][0]["profile"], "dst-server-assets");
        manifest.complete().unwrap();
        assert_eq!(manifest.value["state"], "completed");
        assert!(
            manifest.value["completedAtUtc"]
                .as_str()
                .unwrap()
                .ends_with('Z')
        );
        assert!(
            manifest
                .record_copy(CopyProfile::ServerAssets, &source, &target)
                .is_err()
        );
        assert!(manifest.complete().is_err());
        assert_eq!(fs::read(&manifest.path).unwrap(), manifest.published);
    }

    #[test]
    fn workshop_records_only_real_selected_mod_pairs_and_rejects_escape() {
        let root = fixture();
        let activity = root.join("activity");
        let source = root.join("source");
        let target = activity.join("ugc/Master");
        let relative = "content/322330/123456";
        fs::create_dir_all(source.join(relative)).unwrap();
        fs::create_dir_all(&activity).unwrap();
        let mut manifest = SmokeResourceCopyManifest::begin_in(
            &activity,
            &root.join("registry"),
            &root.join("policy.json"),
        )
        .unwrap();
        manifest
            .record_workshop_items(&source, &target, &["123456".into()])
            .unwrap();
        assert!(
            !target.exists(),
            "planning must not create deployment paths"
        );
        assert_eq!(
            manifest.value["copies"][0]["profile"],
            "dst-workshop-assets"
        );
        assert_eq!(
            Path::new(manifest.value["copies"][0]["sourceRoot"].as_str().unwrap()),
            source.join(relative)
        );
        assert_eq!(
            Path::new(manifest.value["copies"][0]["targetRoot"].as_str().unwrap()),
            target.join(relative)
        );
        let before = fs::read(&manifest.path).unwrap();
        assert!(
            manifest
                .record_copy(CopyProfile::ServerAssets, &source, &source)
                .is_err()
        );
        assert!(
            manifest
                .record_workshop_items(&source, &target, &["../save".into()])
                .is_err()
        );
        assert_eq!(fs::read(&manifest.path).unwrap(), before);
    }

    #[test]
    fn changed_manifest_and_activity_identity_fail_closed() {
        let root = fixture();
        let activity = root.join("activity");
        fs::create_dir(&activity).unwrap();
        let mut manifest = SmokeResourceCopyManifest::begin_in(
            &activity,
            &root.join("registry"),
            &root.join("policy.json"),
        )
        .unwrap();
        let source = root.join("source");
        let target = activity.join("runtime");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&target).unwrap();
        manifest
            .record_copy(CopyProfile::ServerAssets, &source, &target)
            .unwrap();
        let original = manifest.published.clone();
        fs::write(&manifest.path, b"external writer").unwrap();
        assert!(manifest.complete().is_err());
        assert_eq!(fs::read(&manifest.path).unwrap(), b"external writer");
        fs::write(&manifest.path, &original).unwrap();
        fs::rename(&activity, root.join("old-activity")).unwrap();
        fs::create_dir(&activity).unwrap();
        assert!(manifest.complete().is_err());
        assert_eq!(fs::read(&manifest.path).unwrap(), original);
    }

    #[test]
    fn dropped_active_writer_does_not_claim_completion() {
        let root = fixture();
        let activity = root.join("activity");
        fs::create_dir(&activity).unwrap();
        let manifest = SmokeResourceCopyManifest::begin_in(
            &activity,
            &root.join("registry"),
            &root.join("policy.json"),
        )
        .unwrap();
        let path = manifest.path.clone();
        drop(manifest);
        let value: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert_eq!(value["state"], "active");
        assert!(value["completedAtUtc"].is_null());
    }
}
