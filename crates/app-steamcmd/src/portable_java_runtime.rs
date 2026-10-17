use super::*;
use crate::http_download::download_file_with_progress;

pub(super) async fn prepare(
    context: &InstallLockContext,
    tools: &Path,
    install: &Path,
    deadline: InstallDeadline,
    progress: &mut impl FnMut(InstallProgressUpdate),
) -> Result<PortableRuntime, SteamCmdError> {
    let major = required_major(install)?;
    prepare_major(
        context,
        tools,
        major,
        &install.join("jre"),
        deadline,
        progress,
    )
    .await
}

pub(super) async fn prepare_major(
    context: &InstallLockContext,
    tools: &Path,
    major: u16,
    bundled: &Path,
    deadline: InstallDeadline,
    progress: &mut impl FnMut(InstallProgressUpdate),
) -> Result<PortableRuntime, SteamCmdError> {
    let root = tools.join("runtimes/java").join(major.to_string());
    let _guard =
        acquire_runtime_root(context, &root, &format!("portable-java-{major}"), deadline).await?;
    progress(InstallProgressUpdate::stage(
        InstallPhase::Preparing,
        format!("Checking Temurin Java {major} runtime updates..."),
    ));
    let client = http_client()?;
    let package = crate::minecraft_java::latest_runtime_package(&client, major, deadline).await;
    let (url, checksum, size) = match package {
        Ok(package) => package,
        Err(error) => {
            return reuse_existing_runtime(error, &root, bundled, major, deadline, progress).await;
        }
    };
    for candidate in cache_roots(&root, "java") {
        if matches_identity(&candidate, "java", &checksum)
            && valid_java(&candidate, major, deadline).await?
        {
            progress(InstallProgressUpdate::stage(
                InstallPhase::Ready,
                format!("Verified the current Temurin Java {major} runtime."),
            ));
            deadline.check_cancelled()?;
            return Ok(runtime(candidate));
        }
    }
    let stage = scratch(&root, "stage");
    let archive_path = scratch(&root, "zip");
    let result = async {
        let detail = format!("Downloading the current Temurin Java {major} runtime...");
        let archive = download_file_with_progress(
            &client,
            &url,
            &archive_path,
            DownloadIntegrity {
                sha256: Some(&checksum),
                size: Some(size),
                ..Default::default()
            },
            deadline,
            |bytes, total| {
                progress(InstallProgressUpdate::download(
                    &detail,
                    bytes,
                    total.or(Some(size)),
                ))
            },
        )
        .await?;
        progress(InstallProgressUpdate::stage(
            InstallPhase::Extracting,
            format!("Preparing an isolated Temurin Java {major} runtime..."),
        ));
        extract_archive(archive.path(), &stage, deadline).await?;
        let staged = extracted_java_root(&stage)?;
        progress(InstallProgressUpdate::stage(
            InstallPhase::Verifying,
            format!("Verifying Temurin Java {major} runtime..."),
        ));
        if !valid_java(&staged, major, deadline).await? {
            return Err(preparation_error(format!(
                "Downloaded Java runtime does not report Java {major}."
            )));
        }
        deadline.check_cancelled()?;
        let published = publish_runtime(&staged, &root, &checksum, "java", &checksum)?;
        Ok(runtime(published))
    }
    .await;
    // A cleanup failure means a child may still hold files. Preserve evidence.
    if !matches!(
        &result,
        Err(SteamCmdError::InstallProcessCleanupFailed { .. })
    ) {
        await_failed_directory_cleanup(vec![stage]).await;
    }
    // The downloader has joined pending file writes and closed its staging
    // handle before returning. Finish directory cleanup before probing a prior
    // runtime; a network outage does not invalidate that healthy installation.
    let result = match result {
        Err(error) => {
            reuse_existing_runtime(error, &root, bundled, major, deadline, progress).await
        }
        ready => ready,
    };
    if result.is_ok() {
        progress(InstallProgressUpdate::stage(
            InstallPhase::Ready,
            format!("Temurin Java {major} runtime is ready."),
        ));
        deadline.check_cancelled()?;
    }
    result
}

async fn reuse_existing_runtime(
    error: SteamCmdError,
    root: &Path,
    bundled: &Path,
    major: u16,
    deadline: InstallDeadline,
    progress: &mut impl FnMut(InstallProgressUpdate),
) -> Result<PortableRuntime, SteamCmdError> {
    if !source_unavailable(&error) {
        return Err(error);
    }
    let mut candidates = cache_roots(root, "java");
    candidates.push(bundled.to_owned());
    reuse_checked_runtime(
        error,
        candidates,
        major,
        deadline,
        progress,
        |candidate, deadline| async move { valid_java(&candidate, major, deadline).await },
    )
    .await
}

async fn reuse_checked_runtime<P, Fut>(
    error: SteamCmdError,
    candidates: Vec<PathBuf>,
    major: u16,
    deadline: InstallDeadline,
    progress: &mut impl FnMut(InstallProgressUpdate),
    mut probe: P,
) -> Result<PortableRuntime, SteamCmdError>
where
    P: FnMut(PathBuf, InstallDeadline) -> Fut,
    Fut: std::future::Future<Output = Result<bool, SteamCmdError>>,
{
    if !source_unavailable(&error) {
        return Err(error);
    }
    deadline.check_cancelled()?;
    for candidate in candidates {
        let healthy = probe(candidate.clone(), deadline).await?;
        deadline.check_cancelled()?;
        if healthy {
            degraded(progress, &format!("Java {major}"), &error);
            deadline.check_cancelled()?;
            return Ok(runtime(candidate));
        }
    }
    Err(error)
}

fn required_major(install: &Path) -> Result<u16, SteamCmdError> {
    let metadata = read_json(&install.join(".langame/minecraft-server.json"))?;
    metadata
        .get("required_java_major")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u16::try_from(value).ok())
        .filter(|major| (8..=99).contains(major))
        .ok_or_else(|| {
            preparation_error(
                "Installed Minecraft metadata has no valid required Java major version.",
            )
        })
}

pub(super) fn bundled_major(root: &Path) -> Result<u16, SteamCmdError> {
    let file = fs::File::open(root.join("release"))
        .map_err(|error| preparation_error(format!("The package Java runtime is damaged and its release metadata cannot be read: {error}. Repair the server program.")))?;
    let mut bytes = Vec::new();
    file.take(65_537)
        .read_to_end(&mut bytes)
        .map_err(|error| preparation_error(error.to_string()))?;
    if bytes.len() > 65_536 {
        return Err(preparation_error(
            "Package Java release metadata exceeds 64 KiB.",
        ));
    }
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| preparation_error("Package Java release metadata is not UTF-8."))?;
    parse_bundled_major(text)
}

fn parse_bundled_major(text: &str) -> Result<u16, SteamCmdError> {
    let field = |key: &str| -> Option<&str> {
        let prefix = format!("{key}=");
        let mut values = text.lines().filter_map(|line| line.strip_prefix(&prefix));
        let value = values.next()?.strip_prefix('"')?.strip_suffix('"')?;
        if values.next().is_some() {
            None
        } else {
            Some(value)
        }
    };
    let version = field("JAVA_VERSION").ok_or_else(|| {
        preparation_error(
            "Package Java release metadata has no unique JAVA_VERSION; repair the server program.",
        )
    })?;
    if !matches!(field("OS_ARCH"), Some("amd64" | "x86_64" | "x64"))
        || (text.lines().any(|line| line.starts_with("OS_NAME="))
            && field("OS_NAME") != Some("Windows"))
        || version.is_empty()
        || !version
            .bytes()
            .all(|byte| byte.is_ascii_digit() || b"._+".contains(&byte))
    {
        return Err(preparation_error(
            "Package Java release metadata does not identify a stable Windows x64 runtime; repair the server program.",
        ));
    }
    parse_major(&format!("\"{version}\"" )).filter(|major| (8..=99).contains(major))
        .ok_or_else(|| preparation_error("Package Java release metadata has an unsupported JAVA_VERSION; repair the server program."))
}

fn extracted_java_root(stage: &Path) -> Result<PathBuf, SteamCmdError> {
    let entries = fs::read_dir(stage).map_err(|error| preparation_error(error.to_string()))?;
    let mut candidates = entries
        .take(16)
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.join("bin/java.exe").is_file());
    let candidate = candidates
        .next()
        .ok_or_else(|| preparation_error("Java archive has no bin/java.exe."))?;
    if candidates.next().is_some() {
        return Err(preparation_error(
            "Java archive has multiple runtime roots.",
        ));
    }
    Ok(candidate)
}

async fn valid_java(
    root: &Path,
    major: u16,
    deadline: InstallDeadline,
) -> Result<bool, SteamCmdError> {
    if !root.join("bin/java.exe").is_file() || !root.join("bin/server/jvm.dll").is_file() {
        return Ok(false);
    }
    probe_major(&root.join("bin/java.exe"), deadline)
        .await
        .map(|actual| actual == Some(major))
}

pub(super) async fn probe_major(
    executable: &Path,
    deadline: InstallDeadline,
) -> Result<Option<u16>, SteamCmdError> {
    if !executable.is_file() {
        return Ok(None);
    }
    let mut command = Command::new(executable);
    command.arg("-version");
    let output =
        match run_command_capture(command, deadline.limited_to(Duration::from_secs(15))).await {
            Ok(output) => output,
            Err(SteamCmdError::SpawnCommand { .. }) => return Ok(None),
            Err(error) => return Err(error),
        };
    if !output.status.success() {
        return Ok(None);
    }
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(parse_major(&text))
}

fn parse_major(text: &str) -> Option<u16> {
    let version = text.split('"').nth(1)?;
    let mut numbers = version.split('.');
    let first = numbers.next()?.parse::<u16>().ok()?;
    if first == 1 {
        numbers.next()?.parse().ok()
    } else {
        Some(first)
    }
}

fn runtime(root: PathBuf) -> PortableRuntime {
    PortableRuntime {
        executable: Some(root.join("bin/java.exe")),
        environment: BTreeMap::new(),
    }
}

#[cfg(test)]
#[path = "portable_java_runtime_tests.rs"]
mod tests;
