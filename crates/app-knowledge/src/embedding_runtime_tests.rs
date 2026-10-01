use super::*;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("langame-native-runtime-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!("Cannot clean owned native runtime fixture: {error}");
        }
    }
}

#[test]
fn offline_bundle_is_complete_content_addressed_and_reused() {
    let fixture = Fixture::new();
    let installed = ensure_runtime(&fixture.0).unwrap();
    assert_eq!(installed.directory.file_name().unwrap(), BUNDLE_ID);
    let manifest = fs::read(installed.directory.join("runtime-manifest.json")).unwrap();
    let digest: String = Sha256::digest(&manifest)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(digest, BUNDLE_ID);
    for name in LOAD_ORDER {
        assert!(FILES.iter().any(|file| file.name == *name));
        assert!(installed.directory.join(name).is_file());
    }
    let first_time = fs::metadata(installed.directory.join("onnxruntime.dll"))
        .unwrap()
        .modified()
        .unwrap();
    let reused = ensure_runtime(&fixture.0).unwrap();
    assert_eq!(reused.directory, installed.directory);
    assert_eq!(
        fs::metadata(reused.directory.join("onnxruntime.dll"))
            .unwrap()
            .modified()
            .unwrap(),
        first_time
    );
    assert!(!fs::read_dir(&reused.directory).unwrap().any(|file| {
        file.unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".partial")
    }));
}

#[test]
fn verified_runtime_locks_prevent_byte_changes_deletion_and_directory_replacement() {
    let fixture = Fixture::new();
    let installed = ensure_runtime(&fixture.0).unwrap();
    let file = installed.directory.join("onnxruntime.dll");
    assert!(OpenOptions::new().write(true).open(&file).is_err());
    assert!(fs::remove_file(&file).is_err());
    assert!(fs::rename(&installed.directory, fixture.0.join("replacement")).is_err());
    drop(installed);
    assert!(OpenOptions::new().write(true).open(&file).is_ok());
}

#[test]
fn corrupted_cache_is_rejected_before_any_native_code_is_loaded() {
    let fixture = Fixture::new();
    let installed = ensure_runtime(&fixture.0).unwrap();
    let path = installed.directory.join("onnxruntime.dll");
    drop(installed);
    let mut file = OpenOptions::new().write(true).open(&path).unwrap();
    file.write_all(b"NO").unwrap();
    drop(file);
    let error = match ensure_runtime(&fixture.0) {
        Ok(_) => panic!("corrupted runtime must not be admitted"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("SHA256 mismatch"));
}

#[test]
fn concurrent_offline_publication_returns_the_same_verified_bundle() {
    let fixture = Fixture::new();
    let first = fixture.0.clone();
    let second = fixture.0.clone();
    let (left, right) = std::thread::scope(|scope| {
        let left = scope.spawn(|| ensure_runtime(&first));
        let right = scope.spawn(|| ensure_runtime(&second));
        (
            left.join().unwrap().unwrap(),
            right.join().unwrap().unwrap(),
        )
    });
    assert_eq!(left.directory, right.directory);
    assert_eq!(fs::read_dir(&left.directory).unwrap().count(), FILES.len());
}

#[test]
fn installation_lock_is_exclusive_bounded_and_released_without_deleting_it() {
    let fixture = Fixture::new();
    let first = installation_lock(&fixture.0, Instant::now()).unwrap();
    let error = installation_lock(&fixture.0, Instant::now()).unwrap_err();
    assert!(error.to_string().contains("timed out waiting"));
    let path = fixture.0.join(format!(".install-{BUNDLE_ID}.lock"));
    assert!(fs::remove_file(&path).is_err());
    drop(first);
    assert!(path.is_file());
    let _next = installation_lock(&fixture.0, Instant::now()).unwrap();
}

#[test]
fn a_runtime_directory_cannot_be_substituted_with_an_existing_file() {
    let fixture = Fixture::new();
    fs::write(fixture.0.join("native-runtime"), b"unrelated data").unwrap();
    assert!(ensure_runtime(&fixture.0).is_err());
    assert_eq!(
        fs::read(fixture.0.join("native-runtime")).unwrap(),
        b"unrelated data"
    );
}

#[test]
fn native_loader_uses_system_dependencies_and_rejects_foreign_api_ownership() {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    // ORT and the Windows module list are process-global. Fresh child processes
    // exercise the actual loader without relying on the parent test order.
    for mode in [
        "shadow_dependency",
        "foreign_api",
        "conflicting_module",
        "reuse",
        "contended_install",
    ] {
        let fixture = Fixture::new();
        let installer = if mode == "contended_install" {
            let root = fixture.0.join("native-runtime");
            fs::create_dir(&root).unwrap();
            Some(installation_lock(&root, Instant::now()).unwrap())
        } else {
            None
        };
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "embedding_runtime::windows::tests::native_loader_child",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("LANGAME_ORT_TEST_CHILD_MODE", mode)
            .env("LANGAME_ORT_TEST_CHILD_ROOT", &fixture.0)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(0x08000000)
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                let output = child.wait_with_output().unwrap();
                panic!(
                    "native loader child {mode} timed out: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "native loader child {mode} failed: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        drop(installer);
        if mode == "contended_install" {
            // The child could not enter publication while this process held the
            // lock; releasing it must allow the unchanged production installer.
            ensure_runtime(&fixture.0).unwrap();
        }
    }
}

#[test]
#[ignore = "Private subprocess entry, exercised by native_loader_uses_system_dependencies_and_rejects_foreign_api_ownership"]
fn native_loader_child() {
    let Ok(mode) = std::env::var("LANGAME_ORT_TEST_CHILD_MODE") else {
        return;
    };
    let root = PathBuf::from(
        std::env::var_os("LANGAME_ORT_TEST_CHILD_ROOT").expect("parent fixture path"),
    );
    assert!(root.is_absolute());
    let root = fs::canonicalize(root).unwrap();
    let repository = fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap();
    assert!(!root.starts_with(repository));
    if mode == "contended_install" {
        let installation_root = root.join("native-runtime");
        let error = installation_lock(&installation_root, Instant::now()).unwrap_err();
        assert!(error.to_string().contains("timed out waiting"));
        assert!(!installation_root.join(BUNDLE_ID).exists());
        return;
    }
    let installed = ensure_runtime(&root).unwrap();
    if mode == "shadow_dependency" {
        let name: Vec<u16> = "dbghelp.dll".encode_utf16().chain(Some(0)).collect();
        let mut module = std::ptr::null_mut();
        // SAFETY: valid NUL-terminated name and output pointer; never loads a DLL.
        let existing = unsafe { GetModuleHandleExW(0, name.as_ptr(), &mut module) };
        if existing != 0 {
            drop(NativeLibrary(module));
        }
        assert_eq!(existing, 0, "fresh child unexpectedly preloaded dbghelp");
        let system = system_directory().unwrap().join("dbghelp.dll");
        fs::copy(&system, installed.directory.join("dbghelp.dll")).unwrap();
        initialize(&root).unwrap();
        // SAFETY: the successful GetModuleHandleEx adds the reference owned below.
        assert_ne!(
            unsafe { GetModuleHandleExW(0, name.as_ptr(), &mut module) },
            0
        );
        let actual = module_path(&NativeLibrary(module)).unwrap();
        assert_eq!(actual, fs::canonicalize(system).unwrap());
        assert_ne!(actual.parent().unwrap(), installed.directory);
    } else if mode == "foreign_api" || mode == "conflicting_module" {
        let mut locks = Vec::new();
        let libraries: Vec<_> = LOAD_ORDER
            .iter()
            .take(if mode == "foreign_api" {
                LOAD_ORDER.len()
            } else {
                LOAD_ORDER.len() - 1
            })
            .map(|name| preload(&installed.directory.join(name), &mut locks).unwrap())
            .collect();
        if mode == "foreign_api" {
            assert!(ort::set_api(native_api(libraries.last().unwrap()).unwrap()));
            let error = initialize(&root).unwrap_err();
            assert!(error.to_string().contains("API was initialized outside"));
        } else {
            let other = root.join("other-library");
            fs::create_dir(&other).unwrap();
            let other_dll = other.join("onnxruntime.dll");
            fs::copy(installed.directory.join("onnxruntime.dll"), &other_dll).unwrap();
            let other_name: Vec<u16> = other_dll.as_os_str().encode_wide().chain(Some(0)).collect();
            // SAFETY: this is an unchanged copy of our already verified official
            // runtime; only its path differs to exercise conflict rejection.
            let other_handle = unsafe {
                LoadLibraryExW(
                    other_name.as_ptr(),
                    std::ptr::null_mut(),
                    LOAD_LIBRARY_SEARCH_SYSTEM32,
                )
            };
            assert!(!other_handle.is_null());
            let _other = NativeLibrary(other_handle);
            let error = initialize(&root).unwrap_err();
            assert!(error.to_string().contains("conflicting already-loaded"));
        }
    } else if mode == "reuse" {
        initialize(&root).unwrap();
        let second = root.join("no-second-extraction");
        std::thread::scope(|scope| {
            for _ in 0..4 {
                scope.spawn(|| initialize(&second).unwrap());
            }
        });
        assert!(!second.exists());
    } else {
        panic!("unknown private loader test mode");
    }
}

#[test]
#[ignore = "Native runtime load acceptance; set LANGAME_EMBEDDING_MODEL_DIR to a retained cache outside the repository"]
fn packaged_runtime_initializes_once_and_reuses_owner_across_model_directories() {
    let model_dir = PathBuf::from(
        std::env::var_os("LANGAME_EMBEDDING_MODEL_DIR").expect("Set LANGAME_EMBEDDING_MODEL_DIR"),
    );
    assert!(model_dir.is_absolute());
    let model_dir = fs::canonicalize(model_dir).unwrap();
    let repository = fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap();
    assert!(!model_dir.starts_with(repository));
    initialize(&model_dir).unwrap();
    let second = model_dir.join(format!("unused-runtime-reuse-{}", uuid::Uuid::new_v4()));
    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| initialize(&second).unwrap());
        }
    });
    assert!(!second.exists());
}
