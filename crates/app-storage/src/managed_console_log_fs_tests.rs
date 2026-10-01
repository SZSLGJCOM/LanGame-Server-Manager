use super::*;

struct Fixture(std::path::PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("lg-console-fs-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
    fn file(&self, name: &str, bytes: &[u8]) -> (std::path::PathBuf, FileIdentity) {
        let path = self.0.join(name);
        fs::write(&path, bytes).unwrap();
        let id = identity(&File::open(&path).unwrap()).unwrap();
        (path, id)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn replaced_source_is_never_deleted_or_moved() {
    let fixture = Fixture::new();
    let (source, expected) = fixture.file("owned.log", b"owned");
    fs::rename(&source, fixture.0.join("previous.log")).unwrap();
    fs::write(&source, b"unknown replacement").unwrap();
    assert!(remove_owned(&source, &expected).is_err());
    assert!(move_owned(&source, &fixture.0.join("archive.log"), &expected).is_err());
    assert_eq!(fs::read(&source).unwrap(), b"unknown replacement");
}

#[test]
fn moving_owned_file_never_overwrites_an_existing_destination() {
    let fixture = Fixture::new();
    let (source, expected) = fixture.file("owned.log", b"owned");
    let (destination, _) = fixture.file("archive.log", b"unknown archive");
    assert!(move_owned(&source, &destination, &expected).is_err());
    assert_eq!(fs::read(source).unwrap(), b"owned");
    assert_eq!(fs::read(destination).unwrap(), b"unknown archive");
}

#[cfg(windows)]
#[test]
fn verified_mutation_handle_blocks_source_and_parent_replacement() {
    let fixture = Fixture::new();
    let (source, expected) = fixture.file("owned.log", b"owned");
    let mutation = windows::OwnedMutation::open(&source, &expected).unwrap();
    assert!(fs::rename(&source, fixture.0.join("replacement.log")).is_err());
    assert!(fs::rename(&fixture.0, fixture.0.with_extension("moved")).is_err());
    mutation.rename(&fixture.0.join("archive.log")).unwrap();
    assert_eq!(fs::read(fixture.0.join("archive.log")).unwrap(), b"owned");
}

#[cfg(windows)]
#[test]
fn unlink_removes_the_name_while_an_existing_reader_keeps_the_file() {
    use std::io::Read;
    let fixture = Fixture::new();
    let (source, expected) = fixture.file("owned.log", b"retained reader bytes");
    let mut reader = File::open(&source).unwrap();
    assert!(remove_owned(&source, &expected).unwrap());
    assert!(!source.exists());
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"retained reader bytes");
    fs::write(&source, b"new generation").unwrap();
    assert_eq!(fs::read(source).unwrap(), b"new generation");
}

#[cfg(windows)]
#[test]
fn canonical_paths_are_moved_and_removed_without_aliasing() {
    let fixture = Fixture::new();
    let (source, expected) = fixture.file("owned.log", b"canonical output");
    let source = fs::canonicalize(source).unwrap();
    let destination = fs::canonicalize(&fixture.0).unwrap().join("archive.log");
    move_owned(&source, &destination, &expected).unwrap();
    assert_eq!(fs::read(&destination).unwrap(), b"canonical output");
    assert!(remove_owned(&destination, &expected).unwrap());
    assert!(!destination.exists());
}

#[cfg(windows)]
#[test]
fn renames_preserve_exact_unicode_names_at_different_buffer_alignments() {
    use std::os::windows::ffi::OsStrExt;

    let fixture = Fixture::new();
    let short_directory = fs::canonicalize(&fixture.0).unwrap();
    let mut long_directory = short_directory.clone();
    while long_directory.as_os_str().encode_wide().count() <= 280 {
        long_directory.push("nested-console-output-directory");
    }
    fs::create_dir_all(&long_directory).unwrap();
    for directory in [&short_directory, &long_directory] {
        for suffix_length in 0..16 {
            let source = directory.join("owned.log");
            fs::write(&source, b"exact destination").unwrap();
            let expected = identity(&File::open(&source).unwrap()).unwrap();
            let destination = directory.join(format!("玩家😀{}.log", "x".repeat(suffix_length)));
            move_owned(&source, &destination, &expected)
                .unwrap_or_else(|error| panic!("rename to {destination:?}: {error}"));
            assert!(!source.exists());
            assert_eq!(fs::read(&destination).unwrap(), b"exact destination");
            assert!(remove_owned(&destination, &expected).unwrap());
        }
    }
}

#[cfg(windows)]
#[test]
fn mutation_rejects_alternate_stream_and_trailing_alias_targets() {
    let fixture = Fixture::new();
    let (source, expected) = fixture.file("owned.log", b"owned");
    for name in ["archive.log:stream", "archive.log.", "archive.log "] {
        assert!(
            move_owned(&source, &fixture.0.join(name), &expected).is_err(),
            "accepted target alias: {name}"
        );
        assert_eq!(fs::read(&source).unwrap(), b"owned");
    }
}
