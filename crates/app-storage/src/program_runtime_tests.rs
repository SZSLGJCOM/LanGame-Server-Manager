use super::*;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("langame-program-runtime-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        Self(root)
    }

    fn program(&self) -> PathBuf {
        let program = self.0.join("program");
        fs::create_dir(&program).unwrap();
        fs::write(program.join("server.jar"), b"unchanged game program").unwrap();
        program
    }

    fn instance(&self, name: &str) -> PathBuf {
        let instance = self.0.join(name);
        fs::create_dir(&instance).unwrap();
        instance
    }

    fn shared(&self) -> (PathBuf, PathBuf) {
        let program = self.program();
        let instance = self.instance("instance");
        prepare_shared_program_reference(&program, &instance, "minecraft").unwrap();
        (program, instance)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("remove owned program-runtime fixture");
    }
}

fn binding_path(instance: &Path) -> PathBuf {
    instance.join("runtime").join(SHARED_RUNTIME_BINDING)
}

#[test]
fn exclusive_library_reference_is_independent_and_rejects_identity_or_mode_conflicts() {
    let fixture = Fixture::new();
    let program = fixture.program();
    let instance = fixture.instance("exclusive");
    prepare_exclusive_program_reference(&program, &instance, "minecraft").unwrap();
    assert_eq!(
        instance_program_mode(&instance).unwrap(),
        InstanceProgramMode::Independent
    );
    assert!(instance_uses_exclusive_program(&instance).unwrap());
    assert!(instance_uses_library_program(&instance).unwrap());
    assert_eq!(
        resolve_instance_runtime_root(&instance).unwrap(),
        fs::canonicalize(&program).unwrap()
    );
    assert!(!instance.join("runtime/server.jar").exists());
    let identity_path = program.join(PROGRAM_IDENTITY);
    let identity = read_json(&identity_path);
    let mut changed = identity.clone();
    changed["id"] = "another-installation".into();
    write_json(&identity_path, &changed);
    assert_runtime_rejected(&instance);
    write_json(&identity_path, &identity);
    fs::copy(
        instance.join("runtime").join(EXCLUSIVE_RUNTIME_BINDING),
        binding_path(&instance),
    )
    .unwrap();
    assert_runtime_rejected(&instance);
}

fn read_json(path: &Path) -> serde_json::Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn write_json(path: &Path, value: &serde_json::Value) {
    fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}

fn assert_runtime_rejected(instance: &Path) {
    assert!(resolve_instance_runtime_root(instance).is_err());
    assert!(instance_program_mode(instance).is_err());
}

#[test]
fn shared_instances_reference_one_program_without_copying_payload() {
    let fixture = Fixture::new();
    let program = fixture.program();
    fs::create_dir(program.join("assets")).unwrap();
    fs::write(program.join("assets/payload.bin"), vec![0x5a; 65_536]).unwrap();
    let first = fixture.instance("first");
    let second = fixture.instance("second");
    let expected = fs::canonicalize(&program).unwrap();
    assert_eq!(
        prepare_shared_program_reference(&program, &first, "minecraft").unwrap(),
        expected
    );
    let identity = fs::read(program.join(PROGRAM_IDENTITY)).unwrap();
    assert_eq!(
        prepare_shared_program_reference(&program, &second, "minecraft").unwrap(),
        expected
    );
    assert_eq!(fs::read(program.join(PROGRAM_IDENTITY)).unwrap(), identity);
    for instance in [&first, &second] {
        assert_eq!(
            instance_program_mode(instance).unwrap(),
            InstanceProgramMode::Shared
        );
        assert_eq!(resolve_instance_runtime_root(instance).unwrap(), expected);
        let runtime = instance.join("runtime");
        assert!(
            !fs::symlink_metadata(&runtime)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(!crate::private_runtime::is_reparse_point(&runtime).unwrap());
        let files: Vec<_> = fs::read_dir(&runtime)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(files, [std::ffi::OsString::from(SHARED_RUNTIME_BINDING)]);
        assert!(!runtime.join("server.jar").exists());
        assert!(!runtime.join("assets").exists());
    }
    assert_ne!(
        fs::canonicalize(first.join("runtime")).unwrap(),
        fs::canonicalize(second.join("runtime")).unwrap()
    );
    fs::write(first.join("server.properties"), b"first configuration").unwrap();
    fs::write(second.join("server.properties"), b"second configuration").unwrap();
    assert_eq!(
        fs::read(first.join("server.properties")).unwrap(),
        b"first configuration"
    );
    assert_eq!(
        fs::read(second.join("server.properties")).unwrap(),
        b"second configuration"
    );
    assert!(!program.join("server.properties").exists());
    assert_eq!(
        fs::read(program.join("server.jar")).unwrap(),
        b"unchanged game program"
    );
}

#[test]
fn independent_runtime_damage_never_falls_back_to_an_available_program() {
    let fixture = Fixture::new();
    let program = fixture.program();
    let instance = fixture.instance("independent");
    assert_runtime_rejected(&instance);
    let runtime = instance.join("runtime");
    fs::create_dir(&runtime).unwrap();
    assert_runtime_rejected(&instance);
    fs::write(runtime.join(PRIVATE_RUNTIME_MARKER), b"invalid marker").unwrap();
    assert_runtime_rejected(&instance);
    fs::write(runtime.join(PRIVATE_RUNTIME_MARKER), b"managed\n").unwrap();
    assert_eq!(
        instance_program_mode(&instance).unwrap(),
        InstanceProgramMode::Independent
    );
    assert_eq!(resolve_instance_runtime_root(&instance).unwrap(), runtime);
    fs::remove_file(runtime.join(PRIVATE_RUNTIME_MARKER)).unwrap();
    assert_runtime_rejected(&instance);
    assert!(program.join("server.jar").is_file());
}

#[test]
fn shared_program_identity_changes_or_removal_prevent_resolution() {
    let fixture = Fixture::new();
    let (program, instance) = fixture.shared();
    let identity_path = program.join(PROGRAM_IDENTITY);
    let original = read_json(&identity_path);
    for (field, replacement) in [
        ("id", serde_json::json!("replacement-program")),
        ("module_id", serde_json::json!("different-module")),
        ("version", serde_json::json!(2)),
    ] {
        let mut changed = original.clone();
        changed[field] = replacement;
        write_json(&identity_path, &changed);
        assert_runtime_rejected(&instance);
    }
    write_json(&identity_path, &original);
    assert!(resolve_instance_runtime_root(&instance).is_ok());
    fs::remove_file(&identity_path).unwrap();
    assert_runtime_rejected(&instance);
    fs::remove_dir_all(&program).unwrap();
    assert_runtime_rejected(&instance);
}

#[test]
fn shared_and_independent_markers_cannot_claim_the_same_runtime() {
    let fixture = Fixture::new();
    let (_, instance) = fixture.shared();
    fs::write(
        instance.join("runtime").join(PRIVATE_RUNTIME_MARKER),
        b"managed\n",
    )
    .unwrap();
    let error = resolve_instance_runtime_root(&instance).unwrap_err();
    assert!(matches!(
        error,
        StorageError::PrivateRuntimeRefresh { message, .. }
            if message.contains("conflicting")
    ));
    assert!(instance_program_mode(&instance).is_err());
}

#[test]
fn oversized_shared_binding_is_rejected_before_deserialization() {
    let fixture = Fixture::new();
    let (_, instance) = fixture.shared();
    let binding = binding_path(&instance);
    let mut oversized = read_json(&binding);
    oversized["padding"] = "x".repeat(16_384).into();
    write_json(&binding, &oversized);
    let error = resolve_instance_runtime_root(&instance).unwrap_err();
    assert!(matches!(
        error,
        StorageError::PrivateRuntimeRefresh { message, .. }
            if message.contains("too large")
    ));
}

#[test]
fn shared_binding_rejects_traversal_relative_paths_and_instance_overlap() {
    let fixture = Fixture::new();
    let (_, instance) = fixture.shared();
    let path = binding_path(&instance);
    let original = read_json(&path);
    for program_root in [
        fixture.0.join("uncreated/../program"),
        PathBuf::from("program"),
        instance.clone(),
        fixture.0.clone(),
        instance.join("nested-program"),
    ] {
        let mut changed = original.clone();
        changed["program_root"] = serde_json::to_value(program_root).unwrap();
        write_json(&path, &changed);
        assert_runtime_rejected(&instance);
    }
    write_json(&path, &original);
    assert!(resolve_instance_runtime_root(&instance).is_ok());
}

#[test]
fn private_data_paths_allow_children_and_reject_traversal_or_external_paths() {
    let fixture = Fixture::new();
    let instance = fixture.instance("instance");
    let data = instance.join("data");
    fs::create_dir(&data).unwrap();
    fs::write(data.join("world.sav"), b"world").unwrap();
    for path in [data.join("world.sav"), data.join("uncreated/world.sav")] {
        ensure_private_data_path(&instance, &path).unwrap();
    }
    for path in [
        instance.join("../outside/world.sav"),
        data.join("../world.sav"),
        fixture.0.join("outside/world.sav"),
        fixture.0.join("instance-sibling/world.sav"),
        PathBuf::from("data/world.sav"),
    ] {
        assert!(ensure_private_data_path(&instance, &path).is_err());
    }
}

#[cfg(any(windows, unix))]
struct DirectoryLink(PathBuf);

#[cfg(any(windows, unix))]
impl DirectoryLink {
    fn new(link: &Path, target: &Path) -> Self {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let result = std::process::Command::new("cmd.exe")
                .args(["/d", "/c", "mklink", "/J"])
                .arg(link)
                .arg(target)
                .creation_flags(0x0800_0000)
                .output()
                .expect("create owned junction fixture");
            assert!(
                result.status.success(),
                "junction fixture creation failed: stdout={} stderr={}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, link).expect("create owned symlink fixture");
        Self(link.to_owned())
    }
}

#[cfg(any(windows, unix))]
impl Drop for DirectoryLink {
    fn drop(&mut self) {
        #[cfg(windows)]
        fs::remove_dir(&self.0).expect("remove owned junction without traversing it");
        #[cfg(unix)]
        fs::remove_file(&self.0).expect("remove owned directory symlink");
    }
}

#[cfg(any(windows, unix))]
#[test]
fn dangling_independent_marker_conflicts_with_shared_binding() {
    let fixture = Fixture::new();
    let (_, instance) = fixture.shared();
    let target = fixture.0.join("marker-target");
    fs::create_dir(&target).unwrap();
    let marker = instance.join("runtime").join(PRIVATE_RUNTIME_MARKER);
    let _link = DirectoryLink::new(&marker, &target);
    fs::remove_dir(&target).unwrap();
    assert!(!marker.exists());
    assert!(fs::symlink_metadata(&marker).is_ok());
    let error = resolve_instance_runtime_root(&instance).unwrap_err();
    assert!(matches!(
        error,
        StorageError::PrivateRuntimeRefresh { message, .. }
            if message.contains("conflicting")
    ));
}

#[cfg(any(windows, unix))]
#[test]
fn linked_runtime_binding_program_and_data_paths_are_rejected() {
    let fixture = Fixture::new();
    let (program, instance) = fixture.shared();
    let runtime = instance.join("runtime");
    let owned_runtime = instance.join("owned-runtime");
    fs::rename(&runtime, &owned_runtime).unwrap();
    {
        let _link = DirectoryLink::new(&runtime, &owned_runtime);
        assert_runtime_rejected(&instance);
    }
    fs::rename(&owned_runtime, &runtime).unwrap();
    assert!(resolve_instance_runtime_root(&instance).is_ok());

    let program_alias = fixture.0.join("program-alias");
    {
        let _link = DirectoryLink::new(&program_alias, &program);
        let mut binding = read_json(&binding_path(&instance));
        binding["program_root"] = serde_json::to_value(&program_alias).unwrap();
        write_json(&binding_path(&instance), &binding);
        assert_runtime_rejected(&instance);
        let other_instance = fixture.instance("other");
        assert!(
            prepare_shared_program_reference(&program_alias, &other_instance, "minecraft").is_err()
        );
        assert!(!other_instance.join("runtime").exists());
    }

    let external_data = fixture.0.join("external-data");
    fs::create_dir(&external_data).unwrap();
    fs::write(external_data.join("sentinel"), b"preserve external data").unwrap();
    {
        let link = instance.join("linked-data");
        let _link = DirectoryLink::new(&link, &external_data);
        assert!(ensure_private_data_path(&instance, &link.join("world.sav")).is_err());
    }
    assert_eq!(
        fs::read(external_data.join("sentinel")).unwrap(),
        b"preserve external data"
    );
}

#[cfg(unix)]
#[test]
fn shared_binding_file_symlinks_are_rejected() {
    let fixture = Fixture::new();
    let (_, instance) = fixture.shared();
    let binding = binding_path(&instance);
    let target = instance.join("owned-binding.json");
    fs::rename(&binding, &target).unwrap();
    std::os::unix::fs::symlink(&target, &binding).unwrap();
    assert_runtime_rejected(&instance);
    assert!(target.is_file());
}
