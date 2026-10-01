use super::*;
use sha2::{Digest, Sha256};
use std::io::Read;

const CRITICAL_PROGRAM_FILES: [&str; 2] = [
    "bin64/dontstarve_dedicated_server_nullrenderer_x64.exe",
    "data/databundles/scripts.zip",
];

pub(super) async fn validate_owned_launch_program(
    environment: &LiveEnvironment,
    run_root: &Path,
    current: &InstanceDetails,
    launch: &LaunchPlan,
) -> LiveResult<Value> {
    if !launch.ready_to_launch {
        return Err("native launch plan is not ready".into());
    }
    let run_root = run_root.to_path_buf();
    let config = PathBuf::from(&current.config_file_path);
    let instance_id = current.summary.id.clone();
    let library = environment.install_root.clone();
    let executable = PathBuf::from(&launch.executable_path);
    let uses_private_runtime = launch.uses_private_runtime;
    tokio::task::spawn_blocking(move || {
        let checked = || -> LiveResult<Value> {
            let mut components = Path::new(&instance_id).components();
            if !matches!(components.next(), Some(std::path::Component::Normal(_)))
                || components.next().is_some()
            {
                return Err("native launch instance id is not one path component".into());
            }
            for path in [&run_root, &config, &library] {
                reject_reparse_ancestors(path)?;
            }
            let instance_root = config
                .parent()
                .and_then(Path::parent)
                .ok_or("native launch configuration has no instance root")?
                .canonicalize()?;
            let expected = run_root.join("i").join(&instance_id);
            reject_reparse_ancestors(&expected)?;
            if instance_root != expected.canonicalize()? {
                return Err("native launch instance is outside its exact owned directory".into());
            }
            let runtime = app_storage::resolve_instance_runtime_root(&instance_root)?;
            let mut evidence =
                validate_program_files(&library, &instance_root, &runtime, &executable)?;
            evidence["instanceId"] = json!(instance_id);
            evidence["usesPrivateRuntime"] = json!(uses_private_runtime);
            Ok(evidence)
        };
        checked().map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("native program verification worker failed: {error}"))?
    .map_err(Into::into)
}

fn validate_program_files(
    library: &Path,
    instance_root: &Path,
    runtime: &Path,
    executable: &Path,
) -> LiveResult<Value> {
    let private = instance_root.join("runtime");
    for path in [library, &private, runtime, executable] {
        reject_reparse_ancestors(path)?;
    }
    let library = library.canonicalize()?;
    let runtime = runtime.canonicalize()?;
    let mode = if runtime == library {
        "certified_library"
    } else if runtime == private.canonicalize()? {
        "instance_private_copy"
    } else {
        return Err("native launch runtime is outside the owned program roots".into());
    };
    if executable.canonicalize()? != runtime.join(CRITICAL_PROGRAM_FILES[0]).canonicalize()? {
        return Err("native launch executable differs from its exact resolved program path".into());
    }
    let mut files = Vec::new();
    for relative in CRITICAL_PROGRAM_FILES {
        let source = library.join(relative);
        let target = runtime.join(relative);
        reject_reparse_ancestors(&source)?;
        reject_reparse_ancestors(&target)?;
        let expected = streaming_sha256(&source)?;
        let actual = streaming_sha256(&target)?;
        if expected != actual {
            return Err(format!("native program differs from certified source: {relative}").into());
        }
        files.push(json!({"file":relative,"sourceSha256":expected.0,
            "runtimeSha256":actual.0,"byteCount":actual.1,"matches":true}));
    }
    Ok(
        json!({"runtimeMode":mode,"runtimeRoot":runtime,"certifiedSourceRoot":library,
        "criticalFiles":files,"criticalFilesMatch":true}),
    )
}

fn streaming_sha256(path: &Path) -> LiveResult<(String, u64)> {
    if !fs::metadata(path)?.is_file() {
        return Err("native critical program entry is not an ordinary file".into());
    }
    let mut file = fs::File::open(path)?;
    let mut buffer = [0_u8; 16 * 1024];
    let mut digest = Sha256::new();
    let mut bytes = 0_u64;
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
        bytes += count as u64;
    }
    let hash = digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok((hash, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let root = temp_test_dir("launch-program");
            assert!(
                root.canonicalize()
                    .unwrap()
                    .starts_with(std::env::temp_dir().canonicalize().unwrap())
            );
            // Inert files exercise only the path/hash gate; no game is run.
            for directory in ["library", "instance/runtime", "outside"] {
                for relative in CRITICAL_PROGRAM_FILES {
                    let file = root.join(directory).join(relative);
                    fs::create_dir_all(file.parent().unwrap()).unwrap();
                    fs::write(file, b"inert program proof fixture").unwrap();
                }
            }
            Self(root)
        }

        fn check(&self, runtime: &Path, executable: &Path) -> LiveResult<Value> {
            validate_program_files(
                &self.0.join("library"),
                &self.0.join("instance"),
                runtime,
                executable,
            )
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).expect("remove inert program proof fixture");
        }
    }

    #[test]
    fn native_program_gate_accepts_only_matching_library_or_instance_copy() {
        let fixture = Fixture::new();
        for (directory, mode) in [
            ("library", "certified_library"),
            ("instance/runtime", "instance_private_copy"),
        ] {
            let runtime = fixture.0.join(directory);
            let evidence = fixture
                .check(&runtime, &runtime.join(CRITICAL_PROGRAM_FILES[0]))
                .unwrap();
            assert_eq!(evidence["runtimeMode"], mode);
            assert_eq!(evidence["criticalFilesMatch"], true);
            assert_eq!(evidence["criticalFiles"].as_array().unwrap().len(), 2);
        }
    }

    #[test]
    fn native_program_gate_rejects_outside_executable_even_with_identical_bytes() {
        let fixture = Fixture::new();
        let error = fixture
            .check(
                &fixture.0.join("instance/runtime"),
                &fixture.0.join("outside").join(CRITICAL_PROGRAM_FILES[0]),
            )
            .unwrap_err();
        assert!(error.to_string().contains("exact resolved program path"));
    }

    #[test]
    fn native_program_gate_rejects_outside_runtime_even_with_identical_bytes() {
        let fixture = Fixture::new();
        let runtime = fixture.0.join("outside");
        let error = fixture
            .check(&runtime, &runtime.join(CRITICAL_PROGRAM_FILES[0]))
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("outside the owned program roots")
        );
    }

    #[test]
    fn native_program_gate_rejects_either_critical_file_hash_mismatch() {
        let fixture = Fixture::new();
        let runtime = fixture.0.join("instance/runtime");
        for relative in CRITICAL_PROGRAM_FILES {
            fs::write(runtime.join(relative), b"changed program").unwrap();
            let error = fixture
                .check(&runtime, &runtime.join(CRITICAL_PROGRAM_FILES[0]))
                .unwrap_err();
            assert!(error.to_string().contains(relative));
            assert!(error.to_string().contains("differs from certified source"));
            fs::copy(
                fixture.0.join("library").join(relative),
                runtime.join(relative),
            )
            .unwrap();
        }
    }
}
