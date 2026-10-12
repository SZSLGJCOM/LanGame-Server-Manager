use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const MANIFEST: &str = "id = \"fixture\"\nname = \"First\"\nversion = \"1\"\n";
static ROOT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    module: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let sequence = ROOT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "langame-catalog-content-{}-{now}-{sequence}",
            std::process::id()
        ));
        let module = root.join("module");
        fs::create_dir_all(&module).unwrap();
        fs::write(module.join("module.toml"), MANIFEST).unwrap();
        Self { root, module }
    }

    fn discover(&self) -> Result<ModuleDescriptor, ModuleDiscoveryError> {
        Ok(discover_modules(&self.root)?.remove(0))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn changed_manifest_with_the_same_timestamp_is_reparsed_and_rejected_when_invalid() {
    let fixture = Fixture::new();
    let path = fixture.module.join("module.toml");
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    assert_eq!(fixture.discover().unwrap().summary.name, "First");
    let replace = |text: &str| {
        fs::write(&path, text).unwrap();
        fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(modified))
            .unwrap();
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
    };
    replace(&MANIFEST.replace("First", "Other"));
    assert_eq!(fixture.discover().unwrap().summary.name, "Other");
    replace("id = [");
    assert!(matches!(
        fixture.discover(),
        Err(ModuleDiscoveryError::ParseManifest { path: actual, .. }) if actual == path
    ));
    replace(MANIFEST);
    assert_eq!(fixture.discover().unwrap().summary.name, "First");
}

#[test]
fn schema_changes_absence_and_read_errors_remain_visible_after_manifest_reuse() {
    let fixture = Fixture::new();
    let schema = fixture.module.join("schema.json");
    fs::write(&schema, "{}").unwrap();
    assert_eq!(
        fixture.discover().unwrap().schema_json.as_deref(),
        Some("{}")
    );
    fs::write(&schema, "\u{feff}{\"title\":\"current\"}").unwrap();
    assert_eq!(
        fixture.discover().unwrap().schema_json.as_deref(),
        Some("{\"title\":\"current\"}")
    );
    fs::remove_file(&schema).unwrap();
    assert!(fixture.discover().unwrap().schema_json.is_none());
    fs::create_dir(&schema).unwrap();
    assert!(matches!(
        fixture.discover(),
        Err(ModuleDiscoveryError::ReadSchema { path, .. }) if path == schema
    ));

    let manifest = fixture.module.join("module.toml");
    fs::write(&manifest, "id = [").unwrap();
    assert!(matches!(
        fixture.discover(),
        Err(ModuleDiscoveryError::ParseManifest { .. })
    ));
    fs::write(
        &manifest,
        format!("{MANIFEST}\n[storage]\nretained_paths = [\"../escape\"]\n"),
    )
    .unwrap();
    assert!(matches!(
        fixture.discover(),
        Err(ModuleDiscoveryError::ReadSchema { .. })
    ));
    fs::remove_dir(&schema).unwrap();
    assert!(matches!(
        fixture.discover(),
        Err(ModuleDiscoveryError::InvalidStorage { .. })
    ));
}

#[test]
fn identical_manifests_keep_their_own_roots_and_return_independent_descriptors() {
    let first = Fixture::new();
    let second = Fixture::new();
    let mut changed = first.discover().unwrap();
    changed.root = second.module.clone();
    changed.summary.name = "caller mutation".into();
    changed.manifest_toml.clear();
    changed.storage.retained_paths.push("caller-file".into());
    for fixture in [&first, &second, &first] {
        let actual = fixture.discover().unwrap();
        assert_eq!(actual.root, fixture.module);
        assert_eq!(actual.summary.name, "First");
        assert_eq!(actual.manifest_toml, MANIFEST);
        assert!(actual.storage.retained_paths.is_empty());
    }
    let alternate = first.root.join(".");
    let expected = fs::read_dir(&alternate)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let actual = discover_modules(&alternate).unwrap().remove(0);
    assert_eq!(actual.root.as_os_str(), expected.as_os_str());
}

#[test]
fn warmed_catalog_still_enumerates_added_invalid_and_removed_manifests() {
    let fixture = Fixture::new();
    fixture.discover().unwrap();
    let other = fixture.root.join("other");
    fs::create_dir(&other).unwrap();
    let invalid = other.join("module.toml");
    fs::write(&invalid, "id = [").unwrap();
    assert!(matches!(
        discover_modules(&fixture.root),
        Err(ModuleDiscoveryError::ParseManifest { path, .. }) if path == invalid
    ));
    fs::remove_file(&invalid).unwrap();
    fs::remove_file(fixture.module.join("module.toml")).unwrap();
    assert!(matches!(
        discover_modules(&fixture.root),
        Err(ModuleDiscoveryError::EmptyCatalog { .. })
    ));
}

#[test]
fn manifest_reuse_is_exact_and_bounded_without_retaining_source_versions() {
    let fixture = Fixture::new();
    let descriptor = fixture.discover().unwrap();
    let mut cache = ManifestCache::default();
    cache.insert(MANIFEST.into(), descriptor.clone());
    assert!(cache.get(&fixture.module, MANIFEST).is_some());
    assert!(
        cache
            .get(&fixture.module, &format!("\u{feff}{MANIFEST}"))
            .is_none()
    );
    let changed = MANIFEST.replace("First", "Other");
    let mut updated = descriptor.clone();
    updated.summary.name = "Other".into();
    updated.manifest_toml = changed.clone();
    cache.insert(changed.clone(), updated);
    assert!(cache.get(&fixture.module, MANIFEST).is_none());
    assert_eq!(cache.source_bytes, changed.len());

    for index in 0..MAX_CACHED_MANIFESTS {
        let mut entry = descriptor.clone();
        entry.root = fixture.root.join(format!("entry-{index}"));
        cache.insert(MANIFEST.into(), entry);
    }
    assert_eq!(cache.entries.len(), MAX_CACHED_MANIFESTS);
    assert!(cache.get(&fixture.module, &changed).is_none());

    let bounded = format!(
        "{MANIFEST}#{}",
        "x".repeat(MAX_CACHED_SOURCE_BYTES - MANIFEST.len() - 1)
    );
    cache.insert(bounded.clone(), descriptor.clone());
    assert_eq!(cache.entries.len(), 1);
    assert_eq!(cache.source_bytes, MAX_CACHED_SOURCE_BYTES);
    cache.insert(format!("{bounded}x"), descriptor);
    assert_eq!(cache.source_bytes, MAX_CACHED_SOURCE_BYTES);
    assert!(cache.get(&fixture.module, &bounded).is_some());
}

#[test]
fn manifest_over_the_reuse_budget_is_still_discovered() {
    let fixture = Fixture::new();
    let source = format!("{MANIFEST}#{}", "x".repeat(MAX_CACHED_SOURCE_BYTES));
    fs::write(fixture.module.join("module.toml"), &source).unwrap();
    for _ in 0..2 {
        let descriptor = fixture.discover().unwrap();
        assert_eq!(descriptor.manifest_toml, source);
        assert_eq!(descriptor.summary.name, "First");
    }
}
