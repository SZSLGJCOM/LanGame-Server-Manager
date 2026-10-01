use super::commands_local_paths::{explorer_directory_argument, local_directory_to_open};
use std::fs;
use std::path::{Path, PathBuf};

struct DirectoryFixture(PathBuf);

impl DirectoryFixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("lgsm-open-path-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir_all(&root).expect("create isolated directory fixture");
        Self(root)
    }
}

impl Drop for DirectoryFixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("remove isolated directory fixture");
    }
}

#[test]
fn open_local_path_forward_slash_directory_uses_native_explorer_argument() {
    let fixture = DirectoryFixture::new();
    let directory = fixture.0.join("配置 & saves, files");
    fs::create_dir(&directory).expect("create directory with shell-significant characters");
    let frontend_path = directory
        .to_str()
        .expect("UTF-8 test path")
        .replace('\\', "/");

    let argument = local_directory_to_open(&frontend_path).expect("resolve existing directory");

    assert_eq!(argument, explorer_directory_argument(&directory).unwrap());
    assert!(argument.is_absolute());
    assert!(!argument.to_string_lossy().contains('/'));
    assert!(argument.ends_with("配置 & saves, files"));
}

#[test]
fn open_local_path_file_opens_its_containing_directory() {
    let fixture = DirectoryFixture::new();
    let file = fixture.0.join("server config.xml");
    fs::write(&file, "fixture").expect("create test file");
    let frontend_path = file.to_str().expect("UTF-8 test path").replace('\\', "/");

    let argument = local_directory_to_open(&frontend_path).expect("resolve file directory");

    assert_eq!(argument, explorer_directory_argument(&fixture.0).unwrap());
    assert!(argument.is_dir());
}

#[test]
fn open_local_path_relative_directory_resolves_before_explorer_launch() {
    let argument = local_directory_to_open(".").expect("resolve current directory");
    assert_eq!(
        argument,
        dunce::simplified(&std::env::current_dir().unwrap())
    );
    assert!(argument.is_absolute());
}

#[test]
fn open_local_path_relative_file_opens_current_directory() {
    // Cargo runs this test in the package directory; inspect its manifest only.
    let argument = local_directory_to_open("Cargo.toml").expect("resolve relative file");
    assert_eq!(
        argument,
        dunce::simplified(&std::env::current_dir().unwrap())
    );
}

#[test]
fn open_local_path_keeps_unc_share_and_safely_simplifies_extended_disk_paths() {
    assert_eq!(
        explorer_directory_argument(Path::new(r"\\server\share\config")).unwrap(),
        PathBuf::from(r"\\server\share\config")
    );
    assert_eq!(
        explorer_directory_argument(Path::new(r"\\?\C:\LanGame\config")).unwrap(),
        PathBuf::from(r"C:\LanGame\config")
    );
    assert_eq!(
        explorer_directory_argument(Path::new(r"\\?\UNC\server\share\config")).unwrap(),
        PathBuf::from(r"\\?\UNC\server\share\config")
    );
    assert_eq!(
        explorer_directory_argument(Path::new(r"\\?\C:\LanGame\config.")).unwrap(),
        PathBuf::from(r"\\?\C:\LanGame\config.")
    );
    assert_eq!(
        explorer_directory_argument(Path::new(r"\\?\C:\LanGame\CON")).unwrap(),
        PathBuf::from(r"\\?\C:\LanGame\CON")
    );
    let long_path = PathBuf::from(format!(r"\\?\C:\LanGame\{}\config", "a".repeat(245)));
    assert_eq!(explorer_directory_argument(&long_path).unwrap(), long_path);
}

#[test]
fn open_local_path_rejects_empty_and_missing_targets() {
    assert_eq!(
        local_directory_to_open(" \t ").unwrap_err(),
        "path is required"
    );
    let fixture = DirectoryFixture::new();
    let missing = fixture.0.join("missing");
    let error = local_directory_to_open(missing.to_str().unwrap()).unwrap_err();
    assert!(error.contains("failed to inspect path"));
    assert!(error.contains("missing"));
}
