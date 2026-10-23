//! Opt-in acceptance on owned copies of already downloaded official packages.
use super::*;
use app_modules::ModuleDescriptor;
use app_storage::{ProgramBaselineInventory, VerifiedProgramFiles};
use std::fs::{File, FileTimes, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::windows::io::AsRawHandle;
use std::time::SystemTime;
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, FILE_BASIC_INFO, FileBasicInfo, GetFileInformationByHandle,
    SetFileInformationByHandle,
};

fn input_directory(name: &str) -> TestResult<PathBuf> {
    let path = PathBuf::from(std::env::var_os(name).ok_or_else(|| format!("set {name}"))?);
    Ok(app_steamcmd::steam_depot::checked_root(&path)?)
}

/// Copy with create_new and hash the same bytes; scans use the identical bounded
/// reader. Neither mode follows links or includes manager-owned metadata.
fn payload_tree(source: &Path, destination: Option<&Path>) -> TestResult<BTreeMap<String, String>> {
    app_steamcmd::steam_depot::check_plain(source, true)?;
    if let Some(destination) = destination {
        fs::create_dir_all(destination)?;
    }
    let mut files = BTreeMap::new();
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with(".langame-") {
            continue;
        }
        let from = entry.path();
        let to = destination.map(|root| root.join(entry.file_name()));
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "non-UTF-8 fixture path")?;
        if entry.file_type()?.is_dir() {
            for (relative, digest) in payload_tree(&from, to.as_deref())? {
                files.insert(format!("{name}/{relative}"), digest);
            }
        } else {
            let mut input = app_steamcmd::steam_depot::open_read(&from)?;
            let mut output = to
                .map(|path| OpenOptions::new().write(true).create_new(true).open(path))
                .transpose()?;
            let mut hash = Sha256::new();
            let mut buffer = vec![0_u8; 256 * 1024];
            loop {
                let count = input.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                hash.update(&buffer[..count]);
                if let Some(output) = &mut output {
                    output.write_all(&buffer[..count])?;
                }
            }
            files.insert(
                name,
                hash.finalize()
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect(),
            );
        }
    }
    Ok(files)
}

fn add_operator_files(root: &Path) -> TestResult {
    for name in [
        "operator.cfg",
        "mods/verification-private.bin",
        "BepInEx/plugins/verification-private.dll",
    ] {
        let path = root.join(name);
        fs::create_dir_all(path.parent().ok_or("operator file has no parent")?)?;
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?
            .write_all(b"private operator data")?;
    }
    Ok(())
}

fn installed_version(
    storage: &StorageBootstrap,
    descriptor: &ModuleDescriptor,
    root: &Path,
) -> TestResult<String> {
    let probe = probe_module_install_state_with_override(
        &storage.settings,
        &descriptor.summary.id,
        descriptor.summary.steam_app_id,
        descriptor.install.as_ref(),
        descriptor.process.as_ref(),
        Some(root.to_string_lossy().as_ref()),
    );
    check(
        probe.install_state == InstallState::Installed && probe.executable_exists,
        "fixture is not a complete installed package",
    )?;
    probe
        .current_version
        .ok_or_else(|| "fixture has no installed build ID".into())
}

fn official_inventory(
    root: &Path,
    steamcmd: &Path,
    descriptor: &ModuleDescriptor,
    version: &str,
) -> TestResult<ProgramBaselineInventory> {
    let mut evidence = VerifiedProgramFiles::default();
    let include_path = app_storage::program_package_path_filter(descriptor)?;
    let verified = app_steamcmd::verify_installed_steam_package_with_progress(
        root,
        steamcmd,
        descriptor
            .summary
            .steam_app_id
            .ok_or("missing Steam app ID")?,
        version,
        app_steamcmd::SteamPackageVerificationOptions {
            include_path: Some(&include_path),
            ..Default::default()
        },
        |_| {},
        |relative, file, digest| {
            evidence
                .capture_large_verified_file(relative, file, digest)
                .map_err(std::io::Error::other)
        },
    )?;
    Ok(ProgramBaselineInventory {
        files: verified.files,
        directories: verified.directories,
        verified_files: evidence,
    })
}

fn independent_file_identity(path: &Path) -> TestResult<(u32, u64)> {
    let file = app_steamcmd::steam_depot::open_read(path)?;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // The handle and correctly sized output remain live for the native call.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok((
        info.dwVolumeSerialNumber,
        (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
    ))
}

struct AcceptancePackage<'a> {
    descriptor: &'a ModuleDescriptor,
    steamcmd: &'a Path,
    version: &'a str,
    expected: &'a BTreeMap<String, String>,
    source: &'a Path,
}

async fn create_and_verify(
    app: &tauri::App<tauri::test::MockRuntime>,
    storage: &StorageBootstrap,
    package: &AcceptancePackage<'_>,
    sequence: usize,
) -> TestResult<PathBuf> {
    let created = CREATION_INSTALL_FORBIDDEN
        .scope(
            true,
            create_instance_record(
                app.state::<DesktopState>(),
                CreateInstanceInput {
                    name: format!("Verification acceptance {sequence}"),
                    module_id: MODULE.into(),
                },
                Some(app_core::InstanceProgramMode::Independent),
            ),
        )
        .await?;
    let binding = app_storage::read_instance_program_install(&storage.paths, &created.summary.id)
        .await?
        .ok_or("created instance has no binding")?;
    check(
        binding.runtime_mode == "independent"
            && binding.install.install_state == InstallState::Installed,
        "created instance does not use an installed independent program",
    )?;
    let root = fs::canonicalize(binding.install.install_root)?;
    if sequence == 1 {
        check(
            binding.install.scope == app_storage::ProgramInstallScope::Library
                && binding.install.owner_instance_id.is_none()
                && root == fs::canonicalize(package.source)?
                && app_storage::instance_uses_exclusive_program(
                    &storage.paths.instances_root.join(&created.summary.id),
                )?,
            "first independent instance did not exclusively bind the original library source",
        )?;
    } else {
        check(
            binding.install.scope == app_storage::ProgramInstallScope::Instance
                && binding.install.owner_instance_id.as_deref()
                    == Some(created.summary.id.as_str())
                && root.starts_with(fs::canonicalize(
                    storage.paths.instances_root.join(&created.summary.id),
                )?),
            "later instance did not own a separate program under the isolated instances root",
        )?;
    }
    let verified =
        official_inventory(&root, package.steamcmd, package.descriptor, package.version)?;
    check(
        verified.files == *package.expected,
        "instance full official SHA-1/SHA-256 verification differs from the source",
    )?;
    let manifest: serde_json::Value = serde_json::from_slice(&fs::read(root.join(CLEAN_PACKAGE))?)?;
    let copied: BTreeMap<String, String> = serde_json::from_value(manifest["files"].clone())?;
    check(
        copied == *package.expected,
        "instance copy inventory differs from the verified official package",
    )?;
    for name in [
        "operator.cfg",
        "mods/verification-private.bin",
        "BepInEx/plugins/verification-private.dll",
        "first-instance-private.bin",
    ] {
        check(
            !root.join(name).exists(),
            "a later instance inherited private operator or instance data",
        )?;
    }
    Ok(root)
}

fn rewrite_first_byte(path: &Path, value: u8, modified: SystemTime) -> TestResult {
    let size = fs::metadata(path)?.len();
    let mut file = OpenOptions::new().read(true).write(true).open(path)?;
    file.seek(SeekFrom::Start(0))?;
    file.write_all(&[value])?;
    file.sync_all()?;
    file.set_times(FileTimes::new().set_modified(modified))?;
    drop(file);
    check(
        fs::metadata(path)?.len() == size && fs::metadata(path)?.modified()? == modified,
        "tamper probe did not preserve size and LastWriteTime",
    )
}

fn reject_tampering(
    owned: &Path,
    fixture: &Path,
    steamcmd: &Path,
    descriptor: &ModuleDescriptor,
    version: &str,
) -> TestResult<String> {
    app_storage::begin_empty_library_program_acquisition(owned, descriptor)?;
    payload_tree(fixture, Some(owned))?;
    let pending = fs::read(owned.join(".langame-program-acquisition.json"))?;
    let official = app_steamcmd::steam_depot::inventory_for_inspection(
        owned,
        steamcmd,
        descriptor
            .summary
            .steam_app_id
            .ok_or("missing Steam app ID")?,
        true,
    )?;
    let large_file = official
        .files
        .iter()
        .find(|(_, entry)| entry.size >= VerifiedProgramFiles::MIN_LARGE_FILE_BYTES)
        .map(|(relative, _)| relative)
        .ok_or("real package has no cache-admitted file")?;
    let payload = owned.join(large_file);
    check(
        fs::metadata(&payload)?.len() >= VerifiedProgramFiles::MIN_LARGE_FILE_BYTES,
        "real tamper file is below the evidence admission threshold",
    )?;
    let modified = fs::metadata(&payload)?.modified()?;
    let mut byte = [0_u8; 1];
    File::open(&payload)?.read_exact(&mut byte)?;
    rewrite_first_byte(&payload, byte[0] ^ 0x80, modified)?;
    let error = official_inventory(owned, steamcmd, descriptor, version).unwrap_err();
    check(
        error.to_string().contains("SHA1 mismatch"),
        "official verification did not reject same-size restored-mtime tampering",
    )?;
    rewrite_first_byte(&payload, byte[0], modified)?;
    {
        let file = OpenOptions::new().write(true).open(&payload)?;
        // Establish an old ChangeTime before evidence capture, avoiding timing
        // assumptions when the second rewrite must invalidate that evidence.
        let old = FILE_BASIC_INFO {
            ChangeTime: 132_000_000_000_000_000,
            ..Default::default()
        };
        if unsafe {
            SetFileInformationByHandle(
                file.as_raw_handle(),
                FileBasicInfo,
                (&old as *const FILE_BASIC_INFO).cast(),
                std::mem::size_of_val(&old) as u32,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    let verified = official_inventory(owned, steamcmd, descriptor, version)?;
    rewrite_first_byte(&payload, byte[0] ^ 0x80, modified)?;
    check(
        matches!(
            app_storage::record_verified_library_program_baseline(
                owned, descriptor, verified, None
            ),
            Err(app_storage::StorageError::CleanLibraryProgramRequired { .. })
        ),
        "storage accepted stale evidence after same-size restored-mtime tampering",
    )?;
    check(
        !owned.join(CLEAN_PACKAGE).exists()
            && !owned.join(".langame-initial-package.json").exists(),
        "failed verification published a baseline",
    )?;
    check(
        fs::read(owned.join(".langame-program-acquisition.json"))? == pending,
        "failed verification cleared or altered pending ownership",
    )?;
    Ok(large_file.clone())
}

fn optional_second_timing(
    storage: &StorageBootstrap,
    descriptors: &[ModuleDescriptor],
    steamcmd: &Path,
    reportdir: &Path,
) -> TestResult<Option<serde_json::Value>> {
    if std::env::var_os("LANGAME_VERIFICATION_SECOND_FIXTURE_ROOT").is_none() {
        return Ok(None);
    }
    let fixture = input_directory("LANGAME_VERIFICATION_SECOND_FIXTURE_ROOT")?;
    let module = std::env::var("LANGAME_VERIFICATION_SECOND_MODULE")?;
    let descriptor = find_descriptor(descriptors, &module)?;
    check(
        module != MODULE,
        "second timing package must use a different module",
    )?;
    let source = storage.paths.games_root.join(
        &descriptor
            .install
            .as_ref()
            .ok_or("second module has no install contract")?
            .shared_game_dir,
    );
    app_storage::begin_empty_library_program_acquisition(&source, descriptor)?;
    payload_tree(&fixture, Some(&source))?;
    let version = installed_version(storage, descriptor, &source)?;
    let report = reportdir.join(module);
    fs::create_dir_all(&report)?;
    Ok(Some(super::verification_timing::run_timing(
        &source, steamcmd, descriptor, &version, &report,
    )?))
}

async fn real_package_acceptance(measure: bool) -> TestResult {
    let _serial = command_smoke_lock().lock().await;
    let fixture = input_directory("LANGAME_VERIFICATION_FIXTURE_ROOT")?;
    let steamcmd = input_directory("LANGAME_VERIFICATION_STEAMCMD_ROOT")?;
    let reportdir = input_directory("LANGAME_VERIFICATION_REPORT")?;
    input_directory("LANGAME_SMOKE_RUNTIME_ROOT")?;
    let mut root = ProbeRoot::new_for("verify-real", "REAL_VERIFICATION_ACCEPTANCE")?;
    check(
        !root.path.starts_with(&fixture)
            && !fixture.starts_with(&root.path)
            && !reportdir.starts_with(&fixture)
            && !reportdir.starts_with(&steamcmd),
        "owned output overlaps a read-only fixture",
    )?;
    let _environment = ProgramDataEnvGuard::set(&root.path.join("programdata"));
    // Release resolves resources beside its executable. This test harness lives
    // in deps/, so supply the captured repository resources explicitly without
    // changing the production bundled-resource policy.
    let mut paths = app_storage::StoragePaths::resolve_default()?;
    paths.modules_root = workspace_root().join("modules");
    paths.migrations_root = workspace_root().join("migrations");
    app_storage::save_app_settings_with_paths(fixture_settings(&root.path), &paths)?;
    let storage = app_storage::bootstrap_storage_with_paths(paths)?;
    initialize_database(&storage.paths).await?;
    let descriptors = discover_modules(&storage.paths.modules_root)?;
    sync_modules(&storage.paths, &descriptors).await?;
    let descriptor = find_descriptor(&descriptors, MODULE)?;
    let source = storage.paths.games_root.join(MODULE);
    let lock_context = app_steamcmd::InstallLockContext::new(storage.paths.app_data_root.clone());
    let guard = app_steamcmd::acquire_game_install_lifecycle(
        &lock_context,
        MODULE,
        std::slice::from_ref(&source),
    )
    .await?;
    app_storage::begin_empty_library_program_acquisition(&source, descriptor)?;
    let original = payload_tree(&fixture, Some(&source))?;
    let version = installed_version(&storage, descriptor, &source)?;
    let inventory = official_inventory(&source, &steamcmd, descriptor, &version)?;
    let expected = inventory.files.clone();
    serde_json::to_writer_pretty(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(reportdir.join("official-inventory.json"))?,
        &expected,
    )?;
    let unknown = original
        .keys()
        .filter(|key| !expected.contains_key(*key))
        .take(20)
        .collect::<Vec<_>>();
    check(
        unknown.is_empty(),
        &format!("first-reuse fixture includes non-official files: {unknown:?}"),
    )?;
    app_storage::record_verified_library_program_baseline(&source, descriptor, inventory, None)?;
    sync_game_installs(
        &storage.paths,
        &[GameInstallSyncRecord {
            module_id: MODULE.into(),
            install_root: source.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: Some(version.clone()),
            mark_verified: true,
        }],
    )
    .await?;
    let (timing, second_timing) = if measure {
        (
            Some(super::verification_timing::run_timing(
                &source, &steamcmd, descriptor, &version, &reportdir,
            )?),
            optional_second_timing(&storage, &descriptors, &steamcmd, &reportdir)?,
        )
    } else {
        (None, None)
    };
    let tampered_file = reject_tampering(
        &root.path.join("tamper-copy"),
        &fixture,
        &steamcmd,
        descriptor,
        &version,
    )?;
    drop(guard);
    let app = mock_app(&storage)?;
    let mut instances = Vec::new();
    let source_identity = independent_file_identity(&source.join("valheim_server.exe"))?;
    let mut identities = Vec::new();
    let mut source_before = None;
    let package = AcceptancePackage {
        descriptor,
        steamcmd: &steamcmd,
        version: &version,
        expected: &expected,
        source: &source,
    };
    for sequence in 1..=3 {
        let instance = create_and_verify(&app, &storage, &package, sequence).await?;
        let identity = independent_file_identity(&instance.join("valheim_server.exe"))?;
        check(
            !identities.contains(&identity),
            "independent programs share an executable hard link",
        )?;
        identities.push(identity);
        if sequence == 1 {
            check(
                identity == source_identity,
                "first instance did not reuse the actual source executable",
            )?;
            add_operator_files(&instance)?;
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(instance.join("first-instance-private.bin"))?
                .write_all(b"first instance private state")?;
            source_before = Some(payload_tree(&source, None)?);
        } else {
            check(
                identity != source_identity,
                "later independent instance is hard linked to the first source",
            )?;
        }
        instances.push(instance);
    }
    for relative in expected.keys() {
        let mut file_ids = std::collections::BTreeSet::new();
        for instance in &instances {
            check(
                file_ids.insert(independent_file_identity(&instance.join(relative))?),
                &format!("independent instances share an official file hard link: {relative}"),
            )?;
        }
    }
    check(
        fs::read(instances[0].join("first-instance-private.bin"))?
            == b"first instance private state",
        "later creation changed the first instance's private data",
    )?;
    check(
        Some(payload_tree(&source, None)?) == source_before,
        "later instance creation modified the first instance/source payload",
    )?;
    check(
        official_inventory(&source, &steamcmd, descriptor, &version)?.files == expected,
        "creation changed official program bytes in the first instance/source",
    )?;
    check(
        payload_tree(&fixture, None)? == original,
        "acceptance modified the read-only original fixture",
    )?;
    let library = app_storage::read_library_program_install(&storage.paths, MODULE)
        .await?
        .ok_or("library registration was lost")?;
    check(
        fs::canonicalize(library.install_root)? == fs::canonicalize(&source)?,
        "creation replaced the verified library source",
    )?;
    let report = json!({
        "module": MODULE, "build": version, "official_files": expected.len(), "instances": instances,
        "each_instance_full_official_sha1_and_sha256": true, "instance_executable_file_ids": identities,
        "library_executable_file_id": source_identity, "first_instance_exclusively_reuses_library": true,
        "later_instances_distinct_executables_and_no_executable_hardlinks": true,
        "all_official_files_distinct_across_instances": true,
        "source_official_payload_unchanged": true, "later_creation_preserves_first_instance_payload": true, "read_only_fixture_unchanged": true,
        "operator_and_mod_files_excluded": true, "instance_private_data_isolated": true,
        "same_size_restored_mtime_official_rejected": true, "stale_same_handle_evidence_rejected": true,
        "tamper_cache_admitted_file": tampered_file,
        "failed_verification_keeps_pending_and_publishes_no_baseline": true,
        "installer_calls": 0, "game_starts": 0, "timing": timing, "second_package_timing": second_timing,
    });
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(reportdir.join("native-verification-acceptance.json"))?
        .write_all(&serde_json::to_vec_pretty(&report)?)?;
    println!(
        "REAL_VERIFICATION_ACCEPTANCE {}",
        serde_json::to_string(&report)?
    );
    drop(app);
    root.cleanup()?;
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "opt-in: copied real packages and depot cache required; benchmarks verification and verifies three isolated instances and tamper rejection; no download, SteamCMD or game starts"]
async fn native_valheim_verification_benchmark_and_three_instance_acceptance() -> TestResult {
    real_package_acceptance(true).await
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "opt-in: copied real Valheim package and depot cache required; verifies three isolated instances and tamper rejection without rerunning timings; no download, SteamCMD or game starts"]
async fn native_valheim_three_instance_integrity_acceptance() -> TestResult {
    real_package_acceptance(false).await
}
