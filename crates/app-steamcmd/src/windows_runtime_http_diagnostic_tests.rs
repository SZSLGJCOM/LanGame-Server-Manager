//! Opt-in production download verification; no host installation.
use super::*;

#[tokio::test]
#[ignore = "real official HTTP downloads and signature verification only; set LANGAME_NATIVE_HTTP_DIAGNOSTIC_ROOT to a fresh task work directory"]
async fn verify_native_http_recovery() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::var_os("LANGAME_NATIVE_HTTP_DIAGNOSTIC_ROOT")
        .map(PathBuf::from)
        .ok_or("Select a fresh task work directory in LANGAME_NATIVE_HTTP_DIAGNOSTIC_ROOT")?;
    validate_cache_path(&root)?;
    if root.exists() {
        return Err("Diagnostic output exists; prior evidence preserved".into());
    }
    fs::create_dir_all(&root)?;
    let mut cases = Vec::new();
    for package in packages() {
        let path = root.join(format!("{}.exe", package.id));
        let deadline = InstallDeadline::new(
            "production native HTTP recovery verification",
            Duration::from_secs(180),
        );
        let mut observed_bytes = 0;
        let mut routes = Vec::new();
        let result=async {
            let downloaded=http_download::download_native_package_with_progress(package.url,&path,
                DownloadIntegrity {sha256:package.sha256,..Default::default()},deadline,|event|match event {
                    http_download::NativeDownloadEvent::Route {ipv4,origin}=>routes.push(serde_json::json!({"ipv4":ipv4,"origin":origin})),
                    http_download::NativeDownloadEvent::Progress {bytes,..}=>observed_bytes=observed_bytes.max(bytes),
                }).await?;
            process::verify_package(downloaded.path(),package,deadline).await?;
            let bytes=fs::metadata(downloaded.path()).map_err(|error|runtime_error(error.to_string()))?.len();
            let sha256=file_digest(downloaded.path()).map_err(|error|runtime_error(error.to_string()))?;
            if bytes==0 || observed_bytes==0 {return Err(runtime_error("No fresh download bytes recorded"))}
            let version=process::package_version(downloaded.path(),deadline).await?;
            downloaded.record_success();
            downloaded.persist(&path)?;
            Ok::<_,SteamCmdError>(serde_json::json!({"bytes":bytes,"sha256":sha256,"version":version,"signature":"production Microsoft verification succeeded"}))
        }.await;
        let (status, evidence, error) = match result {
            Ok(evidence) => ("passed", Some(evidence), None),
            Err(error) => ("failed", None, Some(error.to_string())),
        };
        println!(
            "NATIVE_HTTP_RECOVERY {} {status} {}",
            package.id,
            error.as_deref().unwrap_or_default()
        );
        cases.push(serde_json::json!({"id":package.id,"official_url":package.url,"routes":routes,"status":status,
            "observed_downloaded_bytes":observed_bytes,"download":evidence,"error":error}));
        let report = serde_json::json!({"schema_version":1,"scope":"host production HTTP recovery verification; no installation",
            "test_binary_sha256":file_digest(&std::env::current_exe()?)?,"cases":cases});
        fs::write(
            root.join("results.json"),
            serde_json::to_vec_pretty(&report)?,
        )?;
    }
    println!(
        "NATIVE_HTTP_RECOVERY_REPORT {}",
        root.join("results.json").display()
    );
    if cases.iter().any(|case| case["status"] != "passed") {
        return Err("One or more native production downloads failed".into());
    }
    Ok(())
}
