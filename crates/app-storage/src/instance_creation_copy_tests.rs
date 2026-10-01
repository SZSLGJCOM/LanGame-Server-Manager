use super::*;
use crate::private_runtime::{PrivateRuntimeProjection, prepare_private_runtime_projection};
use crate::private_runtime_refresh::{
    record_copied_package_baseline, record_package_baseline_with_rules,
};
use std::io::{Read, Write};
use std::time::{Duration, Instant};

const BASELINE: &str = ".langame-package-baseline.json";

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("langame-creation-copy-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        Self(root)
    }

    fn shared(&self) -> PathBuf {
        let path = self.0.join("shared");
        fs::create_dir_all(&path).unwrap();
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("remove owned creation-copy fixture");
    }
}

fn assert_scanned_baseline_matches(
    shared: &Path,
    runtime: &Path,
    exclusions: &[PathBuf],
    exclude_dst_workshop_mods: bool,
) -> serde_json::Value {
    let copied = fs::read(runtime.join(BASELINE)).unwrap();
    record_package_baseline_with_rules(
        shared,
        runtime,
        exclusions,
        Some("fixture-generation"),
        exclude_dst_workshop_mods,
        None,
    )
    .unwrap();
    assert_eq!(
        fs::read(runtime.join(BASELINE)).unwrap(),
        copied,
        "the streamed baseline must equal an independent scan of the finished runtime"
    );
    serde_json::from_slice(&copied).unwrap()
}

#[test]
fn instance_creation_projection_preserves_exclusions_empty_directories_and_refresh_conflicts() {
    let fixture = Fixture::new();
    let shared = fixture.shared();
    for directory in [
        "nested/empty",
        "saves/world",
        "steamapps/workshop/content",
        "mods/workshop-123",
        "mods/workshop-custom",
        "nested/.langame-refresh-staging",
        ".langame-refresh-staging/nested",
    ] {
        fs::create_dir_all(shared.join(directory)).unwrap();
    }
    for (path, content) in [
        ("server.bin", "package one"),
        ("empty.file", ""),
        ("saves/world/save.dat", "existing world"),
        ("steamapps/workshop/content/mod.bin", "existing steam cache"),
        ("mods/workshop-123/modmain.lua", "existing DST mod"),
        ("mods/workshop-custom/packaged.lua", "packaged script"),
        (
            "nested/.langame-refresh-staging/asset",
            "nested ordinary package path",
        ),
        (
            ".langame-refresh-staging/nested/asset",
            "reserved root marker tree",
        ),
        (BASELINE, "source baseline is never package-owned"),
        (PRIVATE_RUNTIME_MARKER, "source marker"),
    ] {
        fs::write(shared.join(path), content).unwrap();
    }
    let projection = PrivateRuntimeProjection {
        private_directories: vec![
            PathBuf::from("saves"),
            PathBuf::from("steamapps/workshop"),
            PathBuf::from("mods/workshop-123"),
        ],
    };
    let exclusions = projection
        .private_directories
        .iter()
        .map(|relative| shared.join(relative))
        .collect::<Vec<_>>();
    let instance = fixture.0.join("instance");
    let runtime = prepare_private_runtime_projection(
        &shared,
        &instance,
        &projection,
        Some("fixture-generation"),
        None,
    )
    .unwrap();
    let baseline = assert_scanned_baseline_matches(&shared, &runtime, &exclusions, false);
    assert_eq!(baseline["version"], 2);
    assert_eq!(baseline["source_generation"], "fixture-generation");
    assert_eq!(baseline["exclude_dst_workshop_mods"], false);
    assert!(
        baseline["directories"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("nested/empty"))
    );
    assert!(
        baseline["files"]
            .get("nested/.langame-refresh-staging/asset")
            .is_some()
    );
    assert!(
        baseline["files"]
            .get(".langame-refresh-staging/nested/asset")
            .is_none()
    );
    assert!(
        baseline["files"]
            .get("mods/workshop-custom/packaged.lua")
            .is_some()
    );
    for private in &projection.private_directories {
        assert!(runtime.join(private).is_dir());
        assert_eq!(fs::read_dir(runtime.join(private)).unwrap().count(), 0);
    }

    fs::write(runtime.join("server.bin"), b"operator choice").unwrap();
    assert_eq!(fs::read(shared.join("server.bin")).unwrap(), b"package one");
    fs::write(shared.join("server.bin"), b"package two").unwrap();
    let error =
        crate::refresh_private_runtime(&shared, &instance, Some("next-generation")).unwrap_err();
    assert!(error.to_string().contains("changed in both"));
    assert_eq!(
        fs::read(runtime.join("server.bin")).unwrap(),
        b"operator choice"
    );
}

#[test]
fn instance_creation_projection_baseline_tracks_created_parent_directories() {
    let fixture = Fixture::new();
    let shared = fixture.shared();
    fs::create_dir_all(shared.join("existing/private")).unwrap();
    fs::create_dir_all(shared.join("assets/empty")).unwrap();
    fs::write(shared.join("existing/private/save.dat"), b"existing save").unwrap();
    fs::write(shared.join("server.bin"), b"package").unwrap();
    let projection = PrivateRuntimeProjection {
        private_directories: vec![
            PathBuf::from("existing/private"),
            PathBuf::from("missing/parent/private"),
        ],
    };
    let runtime = prepare_private_runtime_projection(
        &shared,
        &fixture.0.join("instance"),
        &projection,
        Some("fixture-generation"),
        None,
    )
    .unwrap();
    let exclusions = projection
        .private_directories
        .iter()
        .map(|path| shared.join(path))
        .collect::<Vec<_>>();
    let baseline = assert_scanned_baseline_matches(&shared, &runtime, &exclusions, false);
    assert!(
        baseline["directories"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("missing/parent"))
    );
    assert!(runtime.join("missing/parent/private").is_dir());
    assert!(!runtime.join("existing/private/save.dat").exists());
}

#[cfg(windows)]
#[test]
fn instance_creation_baseline_does_not_reopen_copied_payload() {
    use std::os::windows::fs::OpenOptionsExt;
    let fixture = Fixture::new();
    let shared = fixture.shared();
    let runtime = fixture.0.join("runtime");
    fs::create_dir(&runtime).unwrap();
    fs::write(shared.join("server.bin"), b"abc").unwrap();
    let digest = copy_creation_file(
        &shared.join("server.bin"),
        &runtime.join("server.bin"),
        None,
    )
    .unwrap();
    let mut package = PackageTree::default();
    package
        .copied_file(Path::new("server.bin"), digest)
        .unwrap();
    let exclusive = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(runtime.join("server.bin"))
        .unwrap();
    record_copied_package_baseline(
        &shared,
        &runtime,
        &[],
        Some("fixture-generation"),
        false,
        package,
        None,
    )
    .unwrap();
    drop(exclusive);
    assert_scanned_baseline_matches(&shared, &runtime, &[], false);
}

// Reference for the previous desktop path: cancellable 256 KiB copy, then
// an independent full baseline scan. Fixture input is a plain, trusted tree.
fn copy_without_hash(source: &Path, destination: &Path, cancellation: &AtomicBool) {
    check_creation_cancelled(Some(cancellation)).unwrap();
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        check_creation_cancelled(Some(cancellation)).unwrap();
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_without_hash(&entry.path(), &target, cancellation);
        } else {
            let mut input = fs::File::open(entry.path()).unwrap();
            let mut output = fs::File::create_new(&target).unwrap();
            let mut buffer = vec![0_u8; 256 * 1024];
            loop {
                check_creation_cancelled(Some(cancellation)).unwrap();
                let count = input.read(&mut buffer).unwrap();
                if count == 0 {
                    break;
                }
                output.write_all(&buffer[..count]).unwrap();
                crate::instance_creation_io::test_gate::pause_if_registered(
                    cancellation,
                    crate::instance_creation_io::test_gate::PausePoint::Copy,
                );
            }
            fs::set_permissions(&target, input.metadata().unwrap().permissions()).unwrap();
        }
    }
}

#[test]
#[ignore = "explicit fixed 65 MiB local I/O comparison; no timing assertion"]
fn instance_creation_projection_baseline_benchmark() {
    let fixture = Fixture::new();
    let shared = fixture.shared();
    let payload = shared.join("payload.bin");
    let mut file = fs::File::create(&payload).unwrap();
    let block = (0..1024 * 1024)
        .map(|index| (index % 251) as u8)
        .collect::<Vec<_>>();
    for _ in 0..64 {
        file.write_all(&block).unwrap();
    }
    drop(file);
    fs::create_dir(shared.join("small")).unwrap();
    for index in 0..256 {
        fs::write(shared.join(format!("small/{index:03}.bin")), &block[..4096]).unwrap();
    }
    let mut old_times = Vec::<Duration>::new();
    let mut new_times = Vec::<Duration>::new();
    let cancellation = AtomicBool::new(false);
    let projection = PrivateRuntimeProjection {
        private_directories: vec![PathBuf::from("private-data")],
    };
    let exclusions = [shared.join("private-data")];
    let mut expected_baseline = None;
    for round in 0..4 {
        // Alternate order to reduce systematic warm-cache/order effects.
        for streamed in if round % 2 == 0 {
            [false, true]
        } else {
            [true, false]
        } {
            let instance = fixture.0.join(format!("round-{round}-{streamed}"));
            let start = Instant::now();
            let runtime = if streamed {
                prepare_private_runtime_projection(
                    &shared,
                    &instance,
                    &projection,
                    Some("fixture-generation"),
                    Some(&cancellation),
                )
                .unwrap()
            } else {
                let staging = instance.join("runtime.staging");
                copy_without_hash(&shared, &staging, &cancellation);
                record_package_baseline_with_rules(
                    &shared,
                    &staging,
                    &exclusions,
                    Some("fixture-generation"),
                    false,
                    Some(&cancellation),
                )
                .unwrap();
                fs::write(staging.join(PRIVATE_RUNTIME_MARKER), b"managed\n").unwrap();
                let runtime = instance.join("runtime");
                publish_creation_directory(&staging, &runtime, Some(&cancellation)).unwrap();
                runtime
            };
            let elapsed = start.elapsed();
            if streamed {
                new_times.push(elapsed);
            } else {
                old_times.push(elapsed);
            }
            let baseline = fs::read(runtime.join(BASELINE)).unwrap();
            if let Some(expected) = &expected_baseline {
                assert_eq!(&baseline, expected);
            } else {
                expected_baseline = Some(baseline);
            }
            assert_scanned_baseline_matches(&shared, &runtime, &exclusions, false);
            println!(
                "round={} method={} duration_ms={:.3}",
                round + 1,
                if streamed {
                    "streamed"
                } else {
                    "copy_then_scan"
                },
                elapsed.as_secs_f64() * 1000.0
            );
            fs::remove_dir_all(&instance).unwrap();
        }
    }
    let mean_ms = |times: &[Duration]| {
        times.iter().map(Duration::as_secs_f64).sum::<f64>() * 1000.0 / times.len() as f64
    };
    println!(
        "fixed fixture: 257 files, {} payload bytes; debug_assertions={}; warm filesystem cache, alternating order; copy_then_scan_mean_ms={:.3}; streamed_mean_ms={:.3}",
        65 * 1024 * 1024,
        cfg!(debug_assertions),
        mean_ms(&old_times),
        mean_ms(&new_times)
    );
}

#[cfg(windows)]
#[test]
fn instance_creation_projection_preserves_source_casing_for_configured_parents() {
    let fixture = Fixture::new();
    let shared = fixture.shared();
    fs::create_dir_all(shared.join("Engine")).unwrap();
    fs::write(shared.join("Engine/server.bin"), b"package").unwrap();
    let projection = PrivateRuntimeProjection {
        private_directories: vec![
            PathBuf::from("engine/Saved"),
            PathBuf::from("engine/missing/private"),
        ],
    };
    let runtime = prepare_private_runtime_projection(
        &shared,
        &fixture.0.join("instance"),
        &projection,
        Some("fixture-generation"),
        None,
    )
    .unwrap();
    let exclusions = projection
        .private_directories
        .iter()
        .map(|path| shared.join(path))
        .collect::<Vec<_>>();
    let baseline = assert_scanned_baseline_matches(&shared, &runtime, &exclusions, false);
    let directories = baseline["directories"].as_array().unwrap();
    assert!(directories.contains(&serde_json::json!("Engine")));
    assert!(directories.contains(&serde_json::json!("Engine/missing")));
    assert!(!directories.contains(&serde_json::json!("engine")));
}

#[cfg(windows)]
#[test]
fn instance_creation_manifest_still_rejects_real_source_case_collisions() {
    let fixture = Fixture::new();
    let shared = fixture.shared();
    let runtime = fixture.0.join("runtime");
    fs::create_dir(&runtime).unwrap();
    let mut package = PackageTree::default();
    package.copied_directory(Path::new("Engine")).unwrap();
    package.copied_directory(Path::new("engine")).unwrap();
    package
        .created_directory(Path::new("engine/missing"))
        .unwrap();
    let error = record_copied_package_baseline(&shared, &runtime, &[], None, false, package, None)
        .unwrap_err();
    assert!(error.to_string().contains("differ only by ASCII case"));
    assert!(!runtime.join(BASELINE).exists());
}
