use super::*;
use crate::private_runtime::{PrivateRuntimeProjection, prepare_private_runtime_projection};
use crate::private_runtime_refresh::{
    PrivateRuntimeRefresh, record_package_baseline, refresh_private_runtime,
};
use crate::test_file_snapshot::tree_snapshot;
use std::path::PathBuf;

struct Fixture {
    root: PathBuf,
    source: PathBuf,
    instance: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "langame-projection-identity-{}",
            uuid::Uuid::new_v4()
        ));
        let source = root.join("source");
        let instance = root.join("instance");
        fs::create_dir_all(source.join("Saved")).unwrap();
        fs::create_dir_all(&instance).unwrap();
        fs::write(source.join("server.bin"), b"package one").unwrap();
        fs::write(source.join("Saved/world.sav"), b"source world").unwrap();
        Self {
            root,
            source,
            instance,
        }
    }

    fn projection(&self) -> PathBuf {
        prepare_private_runtime_projection(
            &self.source,
            &self.instance,
            &PrivateRuntimeProjection {
                private_directories: vec![PathBuf::from("Saved")],
            },
            None,
            None,
        )
        .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).expect("remove only this projection fixture");
    }
}

#[test]
fn independent_runtime_is_not_refreshable_even_with_an_old_baseline() {
    for old_baseline in [false, true] {
        let f = Fixture::new();
        let runtime = crate::instances::prepare_private_runtime_root(
            &f.source,
            &f.instance,
            &[f.source.join("Saved")],
            None,
            false,
            crate::program_runtime::ProgramFileSelection::Local,
            None,
        )
        .unwrap();
        if old_baseline {
            record_package_baseline(&f.source, &runtime, &[f.source.join("Saved")], None, None)
                .unwrap();
        }
        fs::write(runtime.join("operator.cfg"), b"local setting").unwrap();
        fs::write(f.source.join("server.bin"), b"package two").unwrap();
        let before = tree_snapshot(&f.root).unwrap();
        let error = refresh_private_runtime(&f.source, &f.instance, Some("2")).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("independent or unrecognized runtime"),
            "{error}"
        );
        assert_eq!(tree_snapshot(&f.root).unwrap(), before);
    }
}

#[test]
fn local_copy_cannot_inherit_a_projection_identity_or_baseline() {
    let f = Fixture::new();
    fs::write(
        f.source.join(PROJECTION_RUNTIME_MARKER),
        b"foreign projection identity",
    )
    .unwrap();
    fs::write(
        f.source.join(".langame-package-baseline.json"),
        b"foreign baseline",
    )
    .unwrap();
    let source_before = tree_snapshot(&f.source).unwrap();
    let runtime = crate::instances::prepare_private_runtime_root(
        &f.source,
        &f.instance,
        &[f.source.join("Saved")],
        None,
        false,
        crate::program_runtime::ProgramFileSelection::Local,
        None,
    )
    .unwrap();
    assert!(!runtime.join(PROJECTION_RUNTIME_MARKER).exists());
    assert!(!runtime.join(".langame-package-baseline.json").exists());
    assert_eq!(
        fs::read(runtime.join("server.bin")).unwrap(),
        b"package one"
    );
    assert_eq!(tree_snapshot(&f.source).unwrap(), source_before);
    let before = tree_snapshot(&f.instance).unwrap();
    assert!(refresh_private_runtime(&f.source, &f.instance, None).is_err());
    assert_eq!(tree_snapshot(&f.instance).unwrap(), before);
}

#[test]
fn local_adoption_rejects_a_projection_identity_without_moving_any_bytes() {
    let f = Fixture::new();
    fs::write(
        f.source.join(PROJECTION_RUNTIME_MARKER),
        b"foreign projection identity",
    )
    .unwrap();
    let before = tree_snapshot(&f.root).unwrap();
    let error = crate::program_adoption::ProgramAdoption::begin(
        &f.source,
        &f.instance,
        &[f.source.join("Saved")],
        false,
        crate::program_runtime::ProgramFileSelection::Local,
        None,
    )
    .unwrap_err();
    assert!(error.to_string().contains("already managed"), "{error}");
    assert_eq!(tree_snapshot(&f.root).unwrap(), before);
}

#[test]
fn projection_refresh_validates_every_participant_before_recovery_changes_any_directory() {
    for name in [
        "runtime",
        "runtime.refresh-rollback",
        "runtime.refresh-staging",
    ] {
        for wrong_source in [false, true] {
            let f = Fixture::new();
            f.projection();
            let participant = f.instance.join(name);
            fs::create_dir_all(&participant).unwrap();
            fs::write(
                participant.join("preserve.sentinel"),
                b"owned or foreign bytes",
            )
            .unwrap();
            let foreign = f.root.join("foreign");
            fs::create_dir(&foreign).unwrap();
            let (source, instance) = if wrong_source {
                (&foreign, &f.instance)
            } else {
                (&f.source, &foreign)
            };
            record_projection_identity(source, instance, &participant).unwrap();
            let before = tree_snapshot(&f.root).unwrap();
            let error = refresh_private_runtime(&f.source, &f.instance, None).unwrap_err();
            assert!(
                error.to_string().contains("does not match"),
                "{name}: {error}"
            );
            assert_eq!(tree_snapshot(&f.root).unwrap(), before, "{name}");
        }
    }
}

#[test]
fn projection_with_missing_runtime_recovers_its_own_rollback_and_partial_staging() {
    let f = Fixture::new();
    let runtime = f.projection();
    fs::write(runtime.join("Saved/world.sav"), b"private world").unwrap();
    fs::write(runtime.join("local.dll"), b"local mod").unwrap();
    let before = tree_snapshot(&runtime).unwrap();
    let rollback = f.instance.join("runtime.refresh-rollback");
    fs::rename(&runtime, &rollback).unwrap();
    let staging = f.instance.join("runtime.refresh-staging");
    fs::create_dir(&staging).unwrap();
    record_projection_identity(&f.source, &f.instance, &staging).unwrap();
    fs::write(staging.join(".langame-refresh-staging"), b"managed\n").unwrap();
    fs::write(staging.join("partial.bin"), b"incomplete new copy").unwrap();
    assert_eq!(
        refresh_private_runtime(&f.source, &f.instance, None).unwrap(),
        PrivateRuntimeRefresh::Current
    );
    assert_eq!(tree_snapshot(&runtime).unwrap(), before);
    assert!(!rollback.exists());
    assert!(!staging.exists());
    assert_eq!(
        fs::read(f.source.join("Saved/world.sav")).unwrap(),
        b"source world"
    );
}

#[test]
fn unrecognized_partial_staging_is_preserved_before_rollback_is_moved() {
    let f = Fixture::new();
    let runtime = f.projection();
    fs::rename(&runtime, f.instance.join("runtime.refresh-rollback")).unwrap();
    let staging = f.instance.join("runtime.refresh-staging");
    fs::create_dir(&staging).unwrap();
    fs::write(staging.join(".langame-refresh-staging"), b"managed\n").unwrap();
    fs::write(staging.join("unknown.bin"), b"do not remove").unwrap();
    let before = tree_snapshot(&f.root).unwrap();
    let error = refresh_private_runtime(&f.source, &f.instance, None).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("projection identity unavailable"),
        "{error}"
    );
    assert_eq!(tree_snapshot(&f.root).unwrap(), before);
}
