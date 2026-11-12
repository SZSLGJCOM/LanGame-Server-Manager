use super::*;

const ICON: &str =
    "https://83374.media.forgecdn.net/avatars/thumbnails/1949/338/256/256/639208701457408935.png";

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "langame-mod-presentations-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }

    fn path(&self) -> PathBuf {
        cache_path(&self.0).unwrap()
    }

    fn write(&self, bytes: &[u8]) {
        fs::create_dir_all(self.path().parent().unwrap()).unwrap();
        fs::write(self.path(), bytes).unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        assert!(self.0.is_absolute() && self.0.starts_with(std::env::temp_dir()));
        assert!(
            self.0
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("langame-mod-presentations-")
        );
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn entry(id: &str, title: Option<&str>, icon: Option<&str>) -> ManualModPresentation {
    ManualModPresentation {
        id: id.into(),
        title: title.map(str::to_owned),
        icon_url: icon.map(str::to_owned),
    }
}

#[test]
fn presentation_cache_reopens_and_merges_without_erasing_optional_metadata() {
    let fixture = Fixture::new();
    assert!(
        read_curseforge_presentations(&fixture.0)
            .unwrap()
            .is_empty()
    );
    remember_curseforge_presentations(
        &fixture.0,
        &[entry("1346144", Some("  DevKit\n Live  "), Some(ICON))],
    )
    .unwrap();
    let stored = read_curseforge_presentations(&fixture.0).unwrap();
    assert_eq!(
        stored["1346144"],
        entry("1346144", Some("DevKit Live"), Some(ICON))
    );
    let original = fs::read(fixture.path()).unwrap();
    remember_curseforge_presentations(&fixture.0, &[entry("1346144", None, None)]).unwrap();
    assert_eq!(fs::read(fixture.path()).unwrap(), original);
    remember_curseforge_presentations(
        &fixture.0,
        &[
            entry(
                "1346144",
                Some("Updated name"),
                Some("https://untrusted.example/icon.png"),
            ),
            entry("20", Some("Another Mod"), None),
        ],
    )
    .unwrap();
    let reopened = read_curseforge_presentations(&fixture.0).unwrap();
    assert_eq!(reopened.len(), 2);
    assert_eq!(
        reopened["1346144"],
        entry("1346144", Some("Updated name"), Some(ICON))
    );
    assert_eq!(reopened["20"].icon_url, None);
}

#[test]
fn id_only_batches_do_not_create_an_empty_cache_and_old_optional_fields_load() {
    let fixture = Fixture::new();
    remember_curseforge_presentations(&fixture.0, &[entry("42", None, None)]).unwrap();
    assert!(!fixture.path().exists());
    fixture.write(br#"{"entries":[{"id":"42","title":"Existing title"}]}"#);
    assert_eq!(
        read_curseforge_presentations(&fixture.0).unwrap()["42"],
        entry("42", Some("Existing title"), None)
    );
    remember_curseforge_presentations(&fixture.0, &[entry("42", None, Some(ICON))]).unwrap();
    assert_eq!(
        read_curseforge_presentations(&fixture.0).unwrap()["42"],
        entry("42", Some("Existing title"), Some(ICON))
    );
}

#[test]
fn optional_metadata_is_bounded_and_uses_only_verified_public_images() {
    assert_eq!(normalize_display_name(Some(" \n\t  ")), None);
    assert_eq!(
        normalize_display_name(Some("A\0\n\t B")),
        Some("A B".into())
    );
    assert_eq!(
        normalize_display_name(Some(&"图".repeat(300)))
            .unwrap()
            .chars()
            .count(),
        240
    );
    assert_eq!(verified_icon_url(Some(ICON)), Some(ICON.into()));
    for icon in [
        "http://images.steamusercontent.com/ugc/1/icon.png",
        "https://user:fixture@images.steamusercontent.com/ugc/1/icon.png",
        "https://images.steamusercontent.com:444/ugc/1/icon.png",
        "https://images.steamusercontent.com/ugc/1/icon.png#secret",
        "https://images.steamusercontent.com.evil.example/ugc/1/icon.png",
        "https://127.0.0.1/ugc/1/icon.png",
        "https://images.steamusercontent.com/arbitrary/icon.png",
        "data:image/png;base64,AAAA",
        "file:///C:/private/image.png",
    ] {
        assert_eq!(verified_icon_url(Some(icon)), None, "{icon}");
    }
    let too_long = format!(
        "https://images.steamusercontent.com/ugc/1/{}.png",
        "x".repeat(2048)
    );
    assert_eq!(verified_icon_url(Some(&too_long)), None);
}

#[test]
fn malformed_cache_and_invalid_ids_are_reported_without_overwriting_data() {
    let fixture = Fixture::new();
    for original in [
        b"broken json".as_slice(),
        br#"{"schema_version":2,"entries":[]}"#,
        br#"{"entries":[{"id":"../42"}]}"#,
        br#"{"entries":[{"id":"042"}]}"#,
        br#"{"entries":[{"id":"0"}]}"#,
        br#"{"entries":[{"id":"4294967296"}]}"#,
        br#"{"entries":[{"id":"42"},{"id":"42"}]}"#,
    ] {
        fixture.write(original);
        assert!(read_curseforge_presentations(&fixture.0).is_err());
        assert!(
            remember_curseforge_presentations(&fixture.0, &[entry("42", Some("Name"), None)])
                .is_err()
        );
        assert_eq!(fs::read(fixture.path()).unwrap(), original);
    }
    for id in ["", "0", "01", "-1", "4294967296", "../42", "42/other"] {
        assert!(
            remember_curseforge_presentations(&fixture.0, &[entry(id, Some("Name"), None)])
                .is_err()
        );
    }
}

#[test]
fn oversized_files_and_entry_counts_are_rejected_without_replacement() {
    let fixture = Fixture::new();
    let original = vec![b' '; MAX_BYTES + 1];
    fixture.write(&original);
    assert!(
        read_curseforge_presentations(&fixture.0)
            .unwrap_err()
            .contains("size limit")
    );
    assert!(
        remember_curseforge_presentations(&fixture.0, &[entry("42", Some("Name"), None)]).is_err()
    );
    assert_eq!(fs::read(fixture.path()).unwrap(), original);
    let entries: Vec<_> = (1..=MAX_ENTRIES + 1)
        .map(|id| entry(&id.to_string(), Some("Name"), None))
        .collect();
    fixture.write(
        &serde_json::to_vec(&Cache {
            schema_version: 1,
            entries,
        })
        .unwrap(),
    );
    assert!(
        read_curseforge_presentations(&fixture.0)
            .unwrap_err()
            .contains("oversized")
    );
}

#[test]
fn cache_evicts_oldest_presentations_under_both_limits() {
    let fixture = Fixture::new();
    let initial: Vec<_> = (1..=MAX_ENTRIES)
        .map(|id| entry(&id.to_string(), Some("Name"), None))
        .collect();
    remember_curseforge_presentations(&fixture.0, &initial).unwrap();
    remember_curseforge_presentations(&fixture.0, &[entry("2049", Some("Newest"), None)]).unwrap();
    let reopened = read_curseforge_presentations(&fixture.0).unwrap();
    assert_eq!(reopened.len(), MAX_ENTRIES);
    assert!(!reopened.contains_key("1"));
    assert_eq!(reopened["2049"].title.as_deref(), Some("Newest"));
    let long_icon = format!(
        "https://images.steamusercontent.com/ugc/1/{}.png",
        "x".repeat(1800)
    );
    let long_name = "图".repeat(240);
    let large: Vec<_> = (3000..3600)
        .map(|id| entry(&id.to_string(), Some(&long_name), Some(&long_icon)))
        .collect();
    remember_curseforge_presentations(&fixture.0, &large).unwrap();
    assert!(fs::metadata(fixture.path()).unwrap().len() <= MAX_BYTES as u64);
    let reopened = read_curseforge_presentations(&fixture.0).unwrap();
    assert!(!reopened.contains_key("3000"));
    assert!(reopened.contains_key("3599"));
    assert_eq!(
        reopened["3599"].icon_url.as_deref(),
        Some(long_icon.as_str())
    );
}

#[test]
fn cache_rejects_non_regular_paths_and_relative_roots() {
    assert!(read_curseforge_presentations(Path::new("relative/data")).is_err());
    let fixture = Fixture::new();
    assert!(read_curseforge_presentations(&fixture.0.join("../other")).is_err());
    fs::create_dir_all(fixture.path()).unwrap();
    assert!(read_curseforge_presentations(&fixture.0).is_err());
    assert!(
        remember_curseforge_presentations(&fixture.0, &[entry("42", Some("Name"), None)]).is_err()
    );
}

#[cfg(unix)]
#[test]
fn cache_rejects_symbolic_link_ancestors_without_writing_the_target() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let outside = Fixture::new();
    symlink(&outside.0, fixture.0.join("cache")).unwrap();
    assert!(read_curseforge_presentations(&fixture.0).is_err());
    assert!(
        remember_curseforge_presentations(&fixture.0, &[entry("42", Some("Name"), None)]).is_err()
    );
    assert!(fs::read_dir(&outside.0).unwrap().next().is_none());
}

#[test]
fn atomic_publication_detects_concurrent_changes_and_preserves_them() {
    let fixture = Fixture::new();
    fixture.write(b"newer value");
    assert!(
        publish(&fixture.path(), Some(b"previous value"), b"replacement")
            .unwrap_err()
            .contains("changed")
    );
    assert_eq!(fs::read(fixture.path()).unwrap(), b"newer value");
    assert!(publish(&fixture.path(), None, b"replacement").is_err());
    assert_eq!(fs::read(fixture.path()).unwrap(), b"newer value");
}
