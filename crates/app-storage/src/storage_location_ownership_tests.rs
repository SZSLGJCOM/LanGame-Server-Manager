use super::*;
use std::collections::BTreeMap;
use std::time::SystemTime;

const MARKER: &str = ".langame-server-manager.json";
const BUSINESS_DIRECTORIES: [&str; 5] = [
    "cmd/steamcmd",
    "server-files",
    "instances",
    "instances/.trash",
    "app-data/ServerManager",
];

#[derive(Debug, PartialEq, Eq)]
struct EntrySnapshot {
    directory: bool,
    readonly: bool,
    modified: SystemTime,
    bytes: Vec<u8>,
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, EntrySnapshot> {
    fn visit(root: &Path, path: &Path, entries: &mut BTreeMap<PathBuf, EntrySnapshot>) {
        let metadata = fs::symlink_metadata(path).unwrap();
        assert!(!metadata.file_type().is_symlink());
        entries.insert(
            path.strip_prefix(root).unwrap().to_owned(),
            EntrySnapshot {
                directory: metadata.is_dir(),
                readonly: metadata.permissions().readonly(),
                modified: metadata.modified().unwrap(),
                bytes: if metadata.is_file() {
                    fs::read(path).unwrap()
                } else {
                    Vec::new()
                },
            },
        );
        if metadata.is_dir() {
            for entry in fs::read_dir(path).unwrap() {
                visit(root, &entry.unwrap().path(), entries);
            }
        }
    }
    let mut entries = BTreeMap::new();
    visit(root, root, &mut entries);
    entries
}

fn make_layout(root: &Path) {
    for relative in BUSINESS_DIRECTORIES {
        fs::create_dir_all(root.join(relative)).unwrap();
    }
}

fn directory_identity(root: &Path) -> Value {
    let node = crate::instance_archive_files::native::open(root, true, false).unwrap();
    serde_json::to_value(node.identity().unwrap()).unwrap()
}

fn assert_marker(root: &Path) {
    let bytes = fs::read(root.join(MARKER)).unwrap();
    assert!(!bytes.is_empty() && bytes.len() <= 2048);
    let marker: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        marker,
        json!({
            "product": "cn.langame.servermanager",
            "schema_version": 1,
            "directory": directory_identity(root),
        })
    );
}

fn assert_untouched_fallback(fixture: &Fixture, user: &Path, rejected: &Path) {
    let before = snapshot(rejected);
    let fallback = fixture.candidate("fallback");
    let paths = resolve_paths(user, || Ok(vec![rejected.to_owned(), fallback.clone()]))
        .expect("an unknown or invalid root must not prevent another usable candidate");
    assert!(
        snapshot(rejected) == before,
        "rejected candidate was modified: {}",
        rejected.display()
    );
    assert_eq!(paths.instances_root, fallback.join("instances"));
    assert!(paths.database_path.is_file());
    assert_marker(&fallback);
}

#[test]
fn unrelated_nonempty_directory_is_skipped_without_writes() {
    let fixture = Fixture::new();
    let unknown = fixture.candidate("unknown");
    fs::create_dir_all(unknown.join("personal/nested")).unwrap();
    fs::write(
        unknown.join("personal/nested/keep.bin"),
        b"unrelated content",
    )
    .unwrap();
    assert_untouched_fallback(&fixture, &fixture.user, &unknown);
}

#[test]
fn a_complete_looking_layout_without_a_marker_is_not_adopted() {
    let fixture = Fixture::new();
    let unknown = fixture.candidate("unknown");
    make_layout(&unknown);
    fs::write(
        unknown.join("server-files/keep.bin"),
        b"existing game files",
    )
    .unwrap();
    assert_untouched_fallback(&fixture, &fixture.user, &unknown);
}

#[test]
fn only_unknown_nonempty_candidates_fail_without_modifying_any_candidate() {
    let fixture = Fixture::new();
    let roots = [fixture.candidate("first"), fixture.candidate("second")];
    fs::create_dir_all(&roots[0]).unwrap();
    fs::write(roots[0].join("keep.txt"), b"unrelated root").unwrap();
    make_layout(&roots[1]);
    let before: Vec<_> = roots.iter().map(|root| snapshot(root)).collect();
    let result = resolve_paths(&fixture.user, || Ok(roots.to_vec()));
    for (root, expected) in roots.iter().zip(before) {
        assert!(snapshot(root) == expected, "{} changed", root.display());
    }
    assert!(
        result.is_err(),
        "no unknown directory may become the selected root"
    );
    assert!(!fixture.pointer().exists());
}

#[test]
fn empty_and_missing_roots_receive_a_marker_bound_to_the_real_directory() {
    for already_empty in [false, true] {
        let fixture = Fixture::new();
        let root = fixture.candidate("selected");
        if already_empty {
            fs::create_dir_all(&root).unwrap();
        }
        let paths = fixture.initialize();
        assert_marker(&root);
        for relative in BUSINESS_DIRECTORIES {
            assert!(root.join(relative).is_dir(), "missing {relative}");
        }
        assert!(paths.database_path.is_file());
        assert_eq!(fixture.read_pointer()["runtime_root"], json!(root));
        let settings: Value =
            serde_json::from_slice(&fs::read(paths.settings_path).unwrap()).unwrap();
        assert_eq!(settings["servers_root"], json!(root.join("instances")));
    }
}

#[test]
fn another_user_reuses_recognized_business_roots_without_adopting_the_first_database() {
    let fixture = Fixture::new();
    let first = fixture.initialize();
    let root = fixture.candidate("selected");
    assert_marker(&root);
    let marker = fs::read(root.join(MARKER)).unwrap();
    fs::write(
        &first.database_path,
        b"first user's database must remain unchanged",
    )
    .unwrap();
    fs::write(
        first.instances_root.join("keep.bin"),
        b"existing instance data",
    )
    .unwrap();
    let private_before = snapshot(&first.app_data_root);
    let user = fixture.root.join("another-user");
    let second = resolve_paths(&user, || Ok(vec![root.clone()])).unwrap();
    assert_ne!(second.app_data_root, first.app_data_root);
    assert_ne!(second.database_path, first.database_path);
    assert!(second.database_path.is_file());
    assert!(
        fs::read(&second.database_path)
            .unwrap()
            .starts_with(b"SQLite format 3\0")
    );
    assert_eq!(second.instances_root, first.instances_root);
    assert_eq!(second.archives_root, first.archives_root);
    assert_eq!(second.games_root, first.games_root);
    assert_eq!(second.steamcmd_root, first.steamcmd_root);
    assert!(
        snapshot(&first.app_data_root) == private_before,
        "the first user's private data changed"
    );
    assert_eq!(fs::read(root.join(MARKER)).unwrap(), marker);
    assert_eq!(
        fs::read(first.instances_root.join("keep.bin")).unwrap(),
        b"existing instance data"
    );
    let pointer: Value =
        serde_json::from_slice(&fs::read(user.join(LOCATION_FILE)).unwrap()).unwrap();
    assert_eq!(pointer["app_data_root"], json!(second.app_data_root));
    assert_eq!(
        fixture.read_pointer()["app_data_root"],
        json!(first.app_data_root)
    );
}

#[test]
fn invalid_marker_content_is_not_repaired_or_adopted() {
    for case in [
        "product",
        "version",
        "unknown-field",
        "directory-field",
        "directory-type",
        "directory-volume-type",
        "malformed",
        "oversized",
    ] {
        let fixture = Fixture::new();
        fixture.initialize();
        let root = fixture.candidate("selected");
        let marker_path = root.join(MARKER);
        let mut marker: Value = serde_json::from_slice(&fs::read(&marker_path).unwrap()).unwrap();
        match case {
            "product" => marker["product"] = json!("another.application"),
            "version" => marker["schema_version"] = json!(2),
            "unknown-field" => marker["unexpected"] = json!(true),
            "directory-field" => marker["directory"]["unexpected"] = json!(true),
            "directory-type" => marker["directory"] = json!("not an identity"),
            "directory-volume-type" => marker["directory"]["volume"] = json!("0"),
            "malformed" | "oversized" => (),
            _ => unreachable!(),
        }
        let mut bytes = serde_json::to_vec(&marker).unwrap();
        if case == "malformed" {
            bytes = b"{".to_vec();
        } else if case == "oversized" {
            assert!(bytes.len() < 2049);
            bytes.resize(2049, b' ');
        }
        fs::write(marker_path, bytes).unwrap();
        assert_untouched_fallback(&fixture, &fixture.root.join("another-user"), &root);
    }
}

#[test]
fn a_marked_root_with_an_incomplete_business_layout_is_not_repaired() {
    for relative in BUSINESS_DIRECTORIES {
        let fixture = Fixture::new();
        fixture.initialize();
        let root = fixture.candidate("selected");
        assert_marker(&root);
        fixture.remove_directory(&root.join(relative));
        assert_untouched_fallback(&fixture, &fixture.root.join("another-user"), &root);
        assert!(
            !root.join(relative).exists(),
            "missing {relative} was recreated"
        );
    }
}

#[test]
fn a_marker_copied_into_another_complete_layout_does_not_establish_ownership() {
    let fixture = Fixture::new();
    fixture.initialize();
    let original = fixture.candidate("selected");
    let unrelated = fixture.candidate("unrelated");
    make_layout(&unrelated);
    assert_ne!(
        directory_identity(&original),
        directory_identity(&unrelated)
    );
    fs::copy(original.join(MARKER), unrelated.join(MARKER)).unwrap();
    assert_untouched_fallback(&fixture, &fixture.root.join("another-user"), &unrelated);
}

#[test]
fn a_saved_location_without_a_marker_keeps_its_database_and_does_not_add_a_marker() {
    let fixture = Fixture::new();
    let first = fixture.initialize();
    let root = fixture.candidate("selected");
    let marker = root.join(MARKER);
    if marker.exists() {
        fs::remove_file(&marker).unwrap();
    }
    fs::write(&first.database_path, b"existing 0.0.2 database").unwrap();
    let before = snapshot(&root);
    let pointer = fs::read(fixture.pointer()).unwrap();
    let next = resolve_paths(&fixture.user, || panic!("saved location must not reselect")).unwrap();
    assert_eq!(next.app_data_root, first.app_data_root);
    assert_eq!(next.database_path, first.database_path);
    assert!(snapshot(&root) == before, "saved data was modified");
    assert_eq!(fs::read(fixture.pointer()).unwrap(), pointer);
    assert!(!marker.exists());
}

#[test]
fn a_directory_at_the_marker_path_is_not_removed_or_adopted() {
    let fixture = Fixture::new();
    fixture.initialize();
    let root = fixture.candidate("selected");
    let marker = root.join(MARKER);
    fs::remove_file(&marker).unwrap();
    fs::create_dir(&marker).unwrap();
    fs::write(marker.join("keep.bin"), b"not a marker file").unwrap();
    assert_untouched_fallback(&fixture, &fixture.root.join("another-user"), &root);
}

#[test]
fn manually_selected_parent_creates_one_langame_child_and_publishes_its_location() {
    let fixture = Fixture::new();
    let parent = fixture.root.join("manual");
    assert!(!parent.exists());
    let paths = super::super::resolve_selected_directory(&fixture.user, &parent).unwrap();
    let root = parent.join("LanGame");
    assert_eq!(paths.instances_root, root.join("instances"));
    assert_eq!(fixture.read_pointer()["runtime_root"], json!(root));
    assert!(paths.database_path.is_file());
    assert_marker(&root);
    assert!(!root.join("LanGame").exists());
}

#[test]
fn selecting_langame_directly_does_not_nest_another_langame_directory() {
    let fixture = Fixture::new();
    let root = fixture.candidate("manual");
    fs::create_dir_all(&root).unwrap();
    let paths = super::super::resolve_selected_directory(&fixture.user, &root).unwrap();
    assert_eq!(paths.instances_root, root.join("instances"));
    assert_eq!(fixture.read_pointer()["runtime_root"], json!(root));
    assert_marker(&root);
    assert!(!root.join("LanGame").exists());
}

#[test]
fn manually_selected_parent_preserves_unrelated_siblings() {
    let fixture = Fixture::new();
    let parent = fixture.root.join("manual");
    fs::create_dir_all(parent.join("personal/nested")).unwrap();
    fs::write(parent.join("personal/nested/keep.bin"), b"personal files").unwrap();
    fs::write(parent.join("keep.txt"), b"personal document").unwrap();
    let personal = snapshot(&parent.join("personal"));
    let document = snapshot(&parent.join("keep.txt"));
    let paths = super::super::resolve_selected_directory(&fixture.user, &parent).unwrap();
    assert!(snapshot(&parent.join("personal")) == personal);
    assert!(snapshot(&parent.join("keep.txt")) == document);
    let mut children: Vec<_> = fs::read_dir(&parent)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    children.sort();
    assert_eq!(
        children,
        ["LanGame", "keep.txt", "personal"].map(std::ffi::OsString::from)
    );
    assert_eq!(paths.instances_root, parent.join("LanGame/instances"));
    assert_marker(&parent.join("LanGame"));
}

#[test]
fn manually_selected_unknown_langame_returns_no_usable_location_without_writes() {
    for select_child in [false, true] {
        let fixture = Fixture::new();
        let parent = fixture.root.join("manual");
        let root = parent.join("LanGame");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("keep.bin"), b"unrelated files").unwrap();
        let before = snapshot(&parent);
        let selected = if select_child { &root } else { &parent };
        let result = super::super::resolve_selected_directory(&fixture.user, selected);
        assert!(
            matches!(result, Err(StorageError::NoUsableStorageLocation { .. })),
            "an occupied unknown directory must require a different selection"
        );
        assert!(
            snapshot(&parent) == before,
            "selected directory was modified"
        );
        assert!(!fixture.pointer().exists());
    }
}

#[test]
fn an_existing_saved_location_ignores_a_new_manual_selection() {
    let fixture = Fixture::new();
    let first = fixture.initialize();
    fs::write(&first.database_path, b"existing database to preserve").unwrap();
    let before = snapshot(&fixture.candidate("selected"));
    let pointer = fs::read(fixture.pointer()).unwrap();
    let parent = fixture.root.join("manual");
    let next = super::super::resolve_selected_directory(&fixture.user, &parent).unwrap();
    assert_eq!(next.app_data_root, first.app_data_root);
    assert_eq!(next.database_path, first.database_path);
    assert!(snapshot(&fixture.candidate("selected")) == before);
    assert_eq!(fs::read(fixture.pointer()).unwrap(), pointer);
    assert!(!parent.exists());
}

#[test]
fn a_missing_saved_database_rejects_manual_reselection_without_recreating_it() {
    let fixture = Fixture::new();
    let paths = fixture.initialize();
    fs::remove_file(&paths.database_path).unwrap();
    let before = snapshot(&fixture.candidate("selected"));
    let pointer = fs::read(fixture.pointer()).unwrap();
    let parent = fixture.root.join("manual");
    let result = super::super::resolve_selected_directory(&fixture.user, &parent);
    assert!(result.is_err());
    assert!(!matches!(
        result,
        Err(StorageError::NoUsableStorageLocation { .. })
    ));
    assert!(!paths.database_path.exists());
    assert!(!parent.exists());
    assert!(snapshot(&fixture.candidate("selected")) == before);
    assert_eq!(fs::read(fixture.pointer()).unwrap(), pointer);
}

#[test]
fn manual_selection_rejects_relative_and_parent_traversal_paths() {
    for relative in [true, false] {
        let fixture = Fixture::new();
        let parent = if relative {
            PathBuf::from("relative-parent")
        } else {
            let mut value = fixture.root.as_os_str().to_owned();
            value.push(std::path::MAIN_SEPARATOR_STR);
            value.push("..");
            value.push(std::path::MAIN_SEPARATOR_STR);
            value.push(fixture.root.file_name().unwrap());
            value.push(std::path::MAIN_SEPARATOR_STR);
            value.push("manual");
            PathBuf::from(value)
        };
        let result = super::super::resolve_selected_directory(&fixture.user, &parent);
        assert!(
            matches!(result, Err(StorageError::NoUsableStorageLocation { details })
                if details.contains("请选择")),
            "unsafe selection was not rejected before access: {}",
            parent.display()
        );
        assert!(!fixture.pointer().exists());
        assert!(!fixture.root.join("manual").exists());
    }
}

#[cfg(windows)]
#[test]
fn manual_selection_rejects_unc_paths_before_accessing_a_share() {
    let share = format!("lgsm-unavailable-test-share-{}", Uuid::new_v4());
    for parent in [
        format!(r"\\localhost\{share}\LanGame"),
        format!(r"\\?\UNC\localhost\{share}\LanGame"),
    ] {
        let fixture = Fixture::new();
        let result = super::super::resolve_selected_directory(&fixture.user, Path::new(&parent));
        assert!(
            matches!(result, Err(StorageError::NoUsableStorageLocation { details })
                if details.contains("请选择")),
            "UNC selection was not rejected before access: {parent}"
        );
        assert!(!fixture.pointer().exists());
    }
}
