//! Controlled real-package comparison; neither mode trusts a historical cache.
use super::*;
use app_core::InstallVerificationProgress;
use app_modules::ModuleDescriptor;
use app_storage::{ProgramBaselineInventory, VerifiedProgramFiles};
use std::collections::BTreeSet;
use std::time::Instant;

type OfficialInventory = (BTreeMap<String, String>, BTreeSet<String>);

#[derive(Clone, Copy)]
struct ProcessCounters {
    read_bytes: u64,
    read_operations: u64,
    user_100ns: u64,
    kernel_100ns: u64,
}

impl ProcessCounters {
    fn capture() -> TestResult<Self> {
        use windows_sys::Win32::Foundation::FILETIME;
        use windows_sys::Win32::System::Threading::{
            GetCurrentProcess, GetProcessIoCounters, GetProcessTimes, IO_COUNTERS,
        };
        let mut io = IO_COUNTERS::default();
        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        // These output buffers live through each call; the pseudo handle belongs
        // to this process and does not need closing.
        unsafe {
            let process = GetCurrentProcess();
            if GetProcessIoCounters(process, &mut io) == 0
                || GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user) == 0
            {
                return Err(std::io::Error::last_os_error().into());
            }
        }
        let ticks =
            |time: FILETIME| (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime);
        Ok(Self {
            read_bytes: io.ReadTransferCount,
            read_operations: io.ReadOperationCount,
            user_100ns: ticks(user),
            kernel_100ns: ticks(kernel),
        })
    }

    fn delta(self, before: Self) -> serde_json::Value {
        json!({
            "logical_read_transfer_bytes": self.read_bytes - before.read_bytes,
            "read_operations": self.read_operations - before.read_operations,
            "cpu_user_ms": (self.user_100ns - before.user_100ns) as f64 / 10_000.0,
            "cpu_kernel_ms": (self.kernel_100ns - before.kernel_100ns) as f64 / 10_000.0,
        })
    }
}

fn sample(
    source: &Path,
    steamcmd: &Path,
    descriptor: &ModuleDescriptor,
    version: &str,
    reuse_current_evidence: bool,
    expected: &mut Option<OfficialInventory>,
) -> TestResult<serde_json::Value> {
    let counters_before = ProcessCounters::capture()?;
    let total_started = Instant::now();
    let mut evidence = VerifiedProgramFiles::default();
    let mut official_progress: Option<InstallVerificationProgress> = None;
    let official_started = Instant::now();
    let verified = app_steamcmd::verify_installed_steam_package_with_progress(
        source,
        steamcmd,
        descriptor
            .summary
            .steam_app_id
            .ok_or("Steam app ID missing")?,
        version,
        None,
        |progress| official_progress = Some(progress),
        |relative, file, digest| {
            if reuse_current_evidence {
                evidence
                    .capture_large_verified_file(relative, file, digest)
                    .map_err(std::io::Error::other)?;
            }
            Ok(())
        },
    )?;
    let official_ms = official_started.elapsed().as_secs_f64() * 1_000.0;
    let counters_official = ProcessCounters::capture()?;
    if let Some(expected) = expected.as_ref() {
        check(
            expected.0 == verified.files && expected.1 == verified.directories,
            "timing mode produced a different official inventory",
        )?;
    } else {
        *expected = Some((verified.files.clone(), verified.directories.clone()));
    }
    let mut baseline_progress: Option<InstallVerificationProgress> = None;
    let baseline_started = Instant::now();
    app_storage::record_verified_library_program_baseline_with_progress(
        source,
        descriptor,
        ProgramBaselineInventory {
            files: verified.files,
            directories: verified.directories,
            verified_files: evidence,
        },
        None,
        &mut |progress| baseline_progress = Some(progress),
    )?;
    let baseline_ms = baseline_started.elapsed().as_secs_f64() * 1_000.0;
    let total_ms = total_started.elapsed().as_secs_f64() * 1_000.0;
    let counters_after = ProcessCounters::capture()?;
    // Read back the durable result outside the measured interval. Cached file
    // stamps differ by mode, but the original allowlist must be identical.
    let persisted: serde_json::Value =
        serde_json::from_slice(&fs::read(source.join(".langame-initial-package.json"))?)?;
    let expected = expected.as_ref().ok_or("official inventory missing")?;
    check(
        serde_json::from_value::<BTreeMap<String, String>>(persisted["files"].clone())?
            == expected.0
            && serde_json::from_value::<BTreeSet<String>>(persisted["directories"].clone())?
                == expected.1,
        "persisted original inventory differs from official verification",
    )?;
    let official = official_progress.ok_or("no official progress")?;
    check(
        official.completed_bytes == official.total_bytes
            && official.completed_files == official.total_files,
        "official progress did not reach its measured totals",
    )?;
    let last = baseline_progress.ok_or("no baseline progress")?;
    check(
        last.stage == app_core::InstallVerificationStage::RecordingBaseline
            && last.completed_files == last.total_files,
        "baseline publication did not finish",
    )?;
    Ok(json!({
        "mode": if reuse_current_evidence { "reuse_current_evidence" } else { "second_full_hash_control" },
        "official_ms": official_ms,
        "baseline_ms": baseline_ms,
        "total_ms": total_ms,
        "official_files": official.total_files,
        "official_bytes": official.total_bytes,
        "official_process_counters": counters_official.delta(counters_before),
        "baseline_process_counters": counters_after.delta(counters_official),
        "total_process_counters": counters_after.delta(counters_before),
    }))
}

fn statistics(values: &[f64]) -> serde_json::Value {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let middle = sorted.len() / 2;
    let median = if sorted.len().is_multiple_of(2) {
        (sorted[middle - 1] + sorted[middle]) / 2.0
    } else {
        sorted[middle]
    };
    json!({ "median_ms": median, "min_ms": sorted[0], "max_ms": sorted[sorted.len() - 1] })
}

pub(super) fn run_timing(
    source: &Path,
    steamcmd: &Path,
    descriptor: &ModuleDescriptor,
    version: &str,
    reportdir: &Path,
) -> TestResult<serde_json::Value> {
    let mut expected = None;
    // Download completion is normally a warm-cache scenario. Preheat both paths
    // without clearing the machine-wide cache or disturbing other applications.
    let mut warmups = Vec::new();
    for reuse in [false, true] {
        println!("REAL_PACKAGE_TIMING warmup reuse={reuse}");
        warmups.push(sample(
            source,
            steamcmd,
            descriptor,
            version,
            reuse,
            &mut expected,
        )?);
    }
    let mut samples = Vec::new();
    // ABBA BAAB balances monotonic cache/thermal drift across four samples each.
    for (index, reuse) in [false, true, true, false, true, false, false, true]
        .into_iter()
        .enumerate()
    {
        let mut result = sample(source, steamcmd, descriptor, version, reuse, &mut expected)?;
        result["index"] = json!(index + 1);
        println!("REAL_PACKAGE_TIMING {result}");
        samples.push(result);
    }
    let totals = |mode: &str, key: &str| -> TestResult<Vec<f64>> {
        samples
            .iter()
            .filter(|sample| sample["mode"] == mode)
            .map(|sample| {
                sample[key]
                    .as_f64()
                    .ok_or_else(|| "missing measurement".into())
            })
            .collect()
    };
    let control = statistics(&totals("second_full_hash_control", "total_ms")?);
    let optimized = statistics(&totals("reuse_current_evidence", "total_ms")?);
    let before = control["median_ms"]
        .as_f64()
        .ok_or("control median missing")?;
    let after = optimized["median_ms"]
        .as_f64()
        .ok_or("optimized median missing")?;
    // Embed build inputs instead of reading a working tree that may have been
    // edited after this long-running executable was compiled.
    let workspace_manifest = include_str!("../../../../Cargo.toml");
    let workspace: toml::Value = toml::from_str(workspace_manifest)?;
    let package_overrides = workspace
        .get("profile")
        .and_then(|profiles| profiles.get("dev"))
        .and_then(|dev| dev.get("package"));
    let paired_saved_ms = samples
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| -> TestResult<f64> {
            let (control, optimized) = if pair[0]["mode"] == "second_full_hash_control" {
                (&pair[0], &pair[1])
            } else {
                (&pair[1], &pair[0])
            };
            Ok(control["total_ms"]
                .as_f64()
                .ok_or("missing paired control")?
                - optimized["total_ms"]
                    .as_f64()
                    .ok_or("missing paired optimized")?)
        })
        .collect::<TestResult<Vec<_>>>()?;
    let report = json!({
        "schema_version": 1,
        "module_id": descriptor.summary.id,
        "steam_app_id": descriptor.summary.steam_app_id,
        "build_id": version,
        "package_root": source,
        "build_profile": if cfg!(debug_assertions) {
            "managed Cargo test (debug assertions enabled)"
        } else {
            "managed Cargo bench (release defaults; debug assertions disabled)"
        },
        "debug_assertions_enabled": cfg!(debug_assertions),
        "development_dependency_overrides": package_overrides,
        "current_evidence_minimum_file_bytes": VerifiedProgramFiles::MIN_LARGE_FILE_BYTES,
        "workspace_manifest_sha256": Sha256::digest(workspace_manifest.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
        "cache_condition": "warm OS cache; one warmup per mode; no machine-wide cache flush",
        "comparison": "same current implementation; control discards only this operation's verified-handle evidence and therefore repeats full SHA-256 before publishing baseline; optimized reuses that evidence after checking the current file identity",
        "limitations": [
            "not a complete binary comparison against cde25c3",
            "does not include SteamCMD download/network or removal of first-install native validate",
            "Windows logical process I/O counters include cached reads; these are not physical disk reads",
            "does not predict cold-disk or end-to-end installation wall time"
        ],
        "samples": samples,
        "warmups_excluded_from_statistics": warmups,
        "paired_saved_ms": paired_saved_ms,
        "paired_savings": statistics(&paired_saved_ms),
        "control": { "total": control,
            "official": statistics(&totals("second_full_hash_control", "official_ms")?),
            "baseline": statistics(&totals("second_full_hash_control", "baseline_ms")?) },
        "optimized": { "total": optimized,
            "official": statistics(&totals("reuse_current_evidence", "official_ms")?),
            "baseline": statistics(&totals("reuse_current_evidence", "baseline_ms")?) },
        "saved_median_ms": before - after,
        "saved_fraction": (before - after) / before,
        "all_rounds_official_inventory_equal": true,
        "all_rounds_persisted_original_inventory_equal": true,
    });
    let output = reportdir.join("verification-timing.json");
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    serde_json::to_writer_pretty(file, &report)?;
    println!(
        "REAL_PACKAGE_TIMING module={} control_median_ms={before:.3} optimized_median_ms={after:.3} saved_ms={:.3} saved_fraction={:.4}",
        descriptor.summary.id,
        before - after,
        (before - after) / before
    );
    Ok(report)
}
