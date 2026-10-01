use super::*;

struct TestRoot(PathBuf);

impl TestRoot {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("lgsm-package-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn directory_pair(&self) -> (PathBuf, PathBuf) {
        let source = self.0.join("source");
        let target = self.0.join("target");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&target).unwrap();
        fs::write(source.join("first.pak"), b"new first package data").unwrap();
        fs::write(source.join("second.pak"), b"new second package data").unwrap();
        fs::write(target.join("first.pak"), b"original first package").unwrap();
        fs::write(target.join("operator.xml"), b"original package metadata").unwrap();
        (source, target)
    }

    fn assert_no_staging(&self) {
        assert!(fs::read_dir(&self.0).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".lgsm-package-")
        }));
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let resolved = fs::canonicalize(&self.0).unwrap();
        assert!(resolved.starts_with(fs::canonicalize(std::env::temp_dir()).unwrap()));
        fs::remove_dir_all(resolved).unwrap();
    }
}

#[test]
fn directory_copy_failure_preserves_installed_package() {
    let root = TestRoot::new();
    let (source, target) = root.directory_pair();
    let mut files = 0;
    let result = deploy(
        "fixture",
        &source,
        &target,
        PackageKind::Directory,
        |from, to| {
            files += 1;
            if files == 2 {
                fs::write(to, b"partial package").unwrap();
                return Err(io::Error::other("injected package copy failure"));
            }
            copy_bytes(from, to)
        },
        move_exclusive,
    );
    assert!(result.is_err());
    assert_eq!(files, 2);
    assert_eq!(
        fs::read(target.join("first.pak")).unwrap(),
        b"original first package"
    );
    assert_eq!(
        fs::read(target.join("operator.xml")).unwrap(),
        b"original package metadata"
    );
    assert!(!target.join("second.pak").exists());
    root.assert_no_staging();
}

#[test]
fn file_copy_failure_preserves_installed_package() {
    let root = TestRoot::new();
    let source = root.0.join("source.pak");
    let target = root.0.join("target.pak");
    fs::write(&source, b"complete new package").unwrap();
    fs::write(&target, b"original package").unwrap();
    let result = deploy(
        "fixture",
        &source,
        &target,
        PackageKind::File,
        |_, to| {
            fs::write(to, b"partial package").unwrap();
            Err(io::Error::other("injected package copy failure"))
        },
        move_exclusive,
    );
    assert!(result.is_err());
    assert_eq!(fs::read(&target).unwrap(), b"original package");
    root.assert_no_staging();
}

#[test]
fn publication_failure_restores_installed_package() {
    let root = TestRoot::new();
    let (source, target) = root.directory_pair();
    let result = deploy(
        "fixture",
        &source,
        &target,
        PackageKind::Directory,
        copy_bytes,
        |_, _| Err(io::Error::other("injected package publication failure")),
    );
    assert!(result.is_err());
    assert_eq!(
        fs::read(target.join("first.pak")).unwrap(),
        b"original first package"
    );
    assert_eq!(
        fs::read(target.join("operator.xml")).unwrap(),
        b"original package metadata"
    );
    root.assert_no_staging();
}

#[test]
fn restoration_never_replaces_a_new_target() {
    let root = TestRoot::new();
    let (source, target) = root.directory_pair();
    let error = deploy(
        "fixture",
        &source,
        &target,
        PackageKind::Directory,
        copy_bytes,
        |_, target| {
            fs::create_dir(target).unwrap();
            fs::write(target.join("operator.xml"), b"concurrent writer").unwrap();
            Err(io::Error::other("injected concurrent publication"))
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("original package retained at"));
    assert_eq!(
        fs::read(target.join("operator.xml")).unwrap(),
        b"concurrent writer"
    );
    let work = fs::read_dir(&root.0)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".lgsm-package-")
        })
        .unwrap();
    assert_eq!(
        fs::read(work.join("retained/first.pak")).unwrap(),
        b"original first package"
    );
}

#[test]
fn changes_during_copy_are_not_overwritten() {
    let root = TestRoot::new();
    let source = root.0.join("source.pak");
    let target = root.0.join("target.pak");
    fs::write(&source, b"new package").unwrap();
    fs::write(&target, b"original package").unwrap();
    let result = deploy(
        "fixture",
        &source,
        &target,
        PackageKind::File,
        |from, to| {
            let bytes = copy_bytes(from, to)?;
            fs::write(&target, b"concurrent package writer").unwrap();
            Ok(bytes)
        },
        move_exclusive,
    );
    assert!(result.is_err());
    assert_eq!(fs::read(&target).unwrap(), b"concurrent package writer");
    root.assert_no_staging();
}

#[test]
fn successful_directory_deployment_replaces_only_its_package() {
    let root = TestRoot::new();
    let (source, target) = root.directory_pair();
    fs::write(root.0.join("unrelated.pak"), b"retained unrelated package").unwrap();
    replace_package_directory("fixture", &source, &target).unwrap();
    assert_eq!(
        fs::read(target.join("first.pak")).unwrap(),
        b"new first package data"
    );
    assert_eq!(
        fs::read(target.join("second.pak")).unwrap(),
        b"new second package data"
    );
    assert!(!target.join("operator.xml").exists());
    assert_eq!(
        fs::read(root.0.join("unrelated.pak")).unwrap(),
        b"retained unrelated package"
    );
    root.assert_no_staging();
    replace_package_directory("fixture", &source, &target).unwrap();
    root.assert_no_staging();
}

#[test]
fn incorrect_target_type_is_rejected_before_copy() {
    let root = TestRoot::new();
    let source = root.0.join("source.pak");
    let target = root.0.join("target.pak");
    fs::write(&source, b"new package").unwrap();
    fs::create_dir(&target).unwrap();
    fs::write(target.join("operator.xml"), b"retained content").unwrap();
    assert!(copy_package_file("fixture", &source, &target).is_err());
    assert_eq!(
        fs::read(target.join("operator.xml")).unwrap(),
        b"retained content"
    );
    root.assert_no_staging();
}

#[cfg(windows)]
#[test]
fn exclusive_move_does_not_replace_an_existing_file() {
    let root = TestRoot::new();
    let source = root.0.join("source.pak");
    let target = root.0.join("target.pak");
    fs::write(&source, b"new package").unwrap();
    fs::write(&target, b"concurrent package").unwrap();
    assert!(move_exclusive(&source, &target).is_err());
    assert_eq!(fs::read(&source).unwrap(), b"new package");
    assert_eq!(fs::read(&target).unwrap(), b"concurrent package");
}
