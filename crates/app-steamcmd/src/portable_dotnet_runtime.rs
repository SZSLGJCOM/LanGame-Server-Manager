use super::*;
use crate::http_download::{download_file_with_progress, fetch_text_from_candidates};
use std::io::{Seek, SeekFrom};

#[path = "portable_dotnet_probe.rs"]
mod probe;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Requirement {
    version: [u32; 3],
    exact: bool,
    prefer_lowest: bool,
}

impl Requirement {
    fn accepts(self, version: [u32; 3]) -> bool {
        version[..2] == self.version[..2]
            && if self.exact {
                version == self.version
            } else {
                version[2] >= self.version[2]
            }
    }

    fn channel(self) -> String {
        format!("{}.{}", self.version[0], self.version[1])
    }

    fn merge(self, other: Self) -> Result<Self, SteamCmdError> {
        if self.version[..2] != other.version[..2] {
            return Err(preparation_error(
                "This instance requires incompatible .NET runtime channels; a single runtime environment cannot satisfy every process.",
            ));
        }
        match (self.exact, other.exact) {
            (true, _) if other.accepts(self.version) => Ok(self),
            (_, true) if self.accepts(other.version) => Ok(other),
            (false, false) => Ok(Self {
                version: self.version.max(other.version),
                exact: false,
                prefer_lowest: self.prefer_lowest || other.prefer_lowest,
            }),
            _ => Err(preparation_error(
                "This instance requires incompatible exact .NET runtime patches.",
            )),
        }
    }
}

#[derive(Debug)]
struct Package {
    version: [u32; 3],
    url: String,
    hash: String,
}

pub(super) async fn prepare(
    context: &InstallLockContext,
    tools: &Path,
    requirement: Requirement,
    deadline: InstallDeadline,
    progress: &mut impl FnMut(InstallProgressUpdate),
) -> Result<PortableRuntime, SteamCmdError> {
    let channel = requirement.channel();
    let root = tools.join("runtimes/dotnet").join(&channel);
    let _guard = acquire_runtime_root(
        context,
        &root,
        &format!("portable-dotnet-{channel}"),
        deadline,
    )
    .await?;
    progress(InstallProgressUpdate::stage(
        InstallPhase::Preparing,
        format!("Checking Microsoft .NET {channel} runtime updates..."),
    ));
    let client = http_client()?;
    let sources = [
        format!(
            "https://builds.dotnet.microsoft.com/dotnet/release-metadata/{channel}/releases.json"
        ),
        format!(
            "https://dotnetcli.blob.core.windows.net/dotnet/release-metadata/{channel}/releases.json"
        ),
    ];
    let fetched =
        fetch_text_from_candidates(&client, &sources, deadline, 4 * 1024 * 1024, |text| {
            select_package(text, requirement)
        })
        .await;
    let package = match fetched {
        Ok((_, package)) => package,
        Err(error) => {
            return reuse_existing_runtime(error, &root, tools, requirement, deadline, progress)
                .await;
        }
    };
    // Keep native Windows runtime paths short; the full SHA-512 still verifies
    // the archive. The 128-bit prefix separates immutable package generations.
    let identity = format!(
        "{}-{}",
        display_version(package.version),
        &package.hash[..32]
    );
    let exact = Requirement {
        version: package.version,
        exact: true,
        prefer_lowest: false,
    };
    for candidate in cache_roots(&root, "dotnet") {
        if matches_identity(&candidate, "dotnet", &package.hash)
            && valid_runtime(&candidate, exact, &root, deadline).await?
        {
            progress(InstallProgressUpdate::stage(
                InstallPhase::Ready,
                format!("Verified the current Microsoft .NET {channel} runtime."),
            ));
            deadline.check_cancelled()?;
            return Ok(runtime(candidate));
        }
    }
    let stage = scratch(&root, "stage");
    let archive_path = scratch(&root, "zip");
    let result = async {
        let detail = format!(
            "Downloading Microsoft .NET {} runtime...",
            display_version(package.version)
        );
        let archive = download_file_with_progress(
            &client,
            &package.url,
            &archive_path,
            DownloadIntegrity {
                sha512: Some(&package.hash),
                ..Default::default()
            },
            deadline,
            |bytes, total| progress(InstallProgressUpdate::download(&detail, bytes, total)),
        )
        .await?;
        progress(InstallProgressUpdate::stage(
            InstallPhase::Extracting,
            "Preparing an isolated Microsoft .NET runtime...",
        ));
        extract_archive(archive.path(), &stage, deadline).await?;
        progress(InstallProgressUpdate::stage(
            InstallPhase::Verifying,
            "Verifying Microsoft .NET runtime...",
        ));
        if !valid_runtime(&stage, exact, &root, deadline).await? {
            return Err(preparation_error(
                "Downloaded .NET runtime did not execute the managed runtime probe successfully.",
            ));
        }
        deadline.check_cancelled()?;
        let published = publish_runtime(&stage, &root, &identity, "dotnet", &package.hash)?;
        Ok(runtime(published))
    }
    .await;
    if !matches!(
        &result,
        Err(SteamCmdError::InstallProcessCleanupFailed { .. })
    ) {
        await_failed_directory_cleanup(vec![stage]).await;
    }
    // File handles and the extraction tree have settled before a network-only
    // failure may reuse a separately verified prior runtime.
    let result = match result {
        Err(error) => {
            reuse_existing_runtime(error, &root, tools, requirement, deadline, progress).await
        }
        ready => ready,
    };
    if result.is_ok() {
        progress(InstallProgressUpdate::stage(
            InstallPhase::Ready,
            format!("Microsoft .NET {channel} runtime is ready."),
        ));
        deadline.check_cancelled()?;
    }
    result
}

async fn reuse_existing_runtime(
    error: SteamCmdError,
    owned_root: &Path,
    tools: &Path,
    required: Requirement,
    deadline: InstallDeadline,
    progress: &mut impl FnMut(InstallProgressUpdate),
) -> Result<PortableRuntime, SteamCmdError> {
    if !source_unavailable(&error) {
        return Err(error);
    }
    reuse_checked_runtime(
        error,
        existing_roots(owned_root, tools),
        &required.channel(),
        deadline,
        progress,
        |candidate, deadline| async move {
            valid_runtime(&candidate, required, owned_root, deadline).await
        },
    )
    .await
}

async fn reuse_checked_runtime<P, Fut>(
    error: SteamCmdError,
    candidates: Vec<PathBuf>,
    channel: &str,
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
            degraded(progress, &format!(".NET {channel}"), &error);
            deadline.check_cancelled()?;
            return Ok(runtime(candidate));
        }
    }
    Err(error)
}

pub(super) fn discover(executables: &[PathBuf]) -> Result<Option<Requirement>, SteamCmdError> {
    let mut required: Option<Requirement> = None;
    let mut seen = std::collections::BTreeSet::new();
    for executable in executables {
        let path = executable.with_extension("runtimeconfig.json");
        if !seen.insert(path.clone()) || !path.is_file() {
            continue;
        }
        validate_apphost_architecture(executable)?;
        let config = read_json(&path)?;
        let options = config.get("runtimeOptions").ok_or_else(|| {
            preparation_error(format!("runtimeOptions are missing in {}", path.display()))
        })?;
        if let Some(included) = options.get("includedFrameworks") {
            if options.get("framework").is_some() || options.get("frameworks").is_some() {
                return Err(preparation_error(
                    "The server runtime configuration mixes self-contained and framework-dependent declarations.",
                ));
            }
            validate_self_contained(&path, included)?;
            continue;
        }
        let next = requirement(&config)?;
        required = Some(match required {
            Some(current) => current.merge(next)?,
            None => next,
        });
    }
    Ok(required)
}

fn validate_apphost_architecture(executable: &Path) -> Result<(), SteamCmdError> {
    if !executable.is_file()
        || !executable
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("exe"))
    {
        return Ok(());
    }
    let mut file = fs::File::open(executable).map_err(|error| {
        preparation_error(format!(
            "Cannot inspect the .NET apphost architecture: {error}"
        ))
    })?;
    let mut dos = [0u8; 64];
    file.read_exact(&mut dos)
        .map_err(|error| preparation_error(format!("Invalid .NET apphost DOS header: {error}")))?;
    if &dos[..2] != b"MZ" {
        return Err(preparation_error(
            "The .NET apphost is not a Windows PE executable.",
        ));
    }
    let offset = u32::from_le_bytes([dos[60], dos[61], dos[62], dos[63]]);
    if offset > 1024 * 1024 {
        return Err(preparation_error(
            "The .NET apphost PE header exceeds its inspection bound.",
        ));
    }
    file.seek(SeekFrom::Start(u64::from(offset)))
        .map_err(|error| preparation_error(error.to_string()))?;
    let mut pe = [0u8; 6];
    file.read_exact(&mut pe)
        .map_err(|error| preparation_error(format!("Invalid .NET apphost PE header: {error}")))?;
    if &pe[..4] != b"PE\0\0" {
        return Err(preparation_error(
            "The .NET apphost PE signature is invalid.",
        ));
    }
    match u16::from_le_bytes([pe[4], pe[5]]) {
        0x8664 => Ok(()),
        0x014c => Err(preparation_error(
            "This .NET server apphost is x86; automatic runtime preparation currently supports Windows x64 and will not attach an incompatible x64 runtime.",
        )),
        _ => Err(preparation_error(
            "The .NET server apphost architecture is unsupported; automatic runtime preparation currently supports Windows x64.",
        )),
    }
}

fn validate_self_contained(
    config_path: &Path,
    included: &serde_json::Value,
) -> Result<(), SteamCmdError> {
    let frameworks = included
        .as_array()
        .filter(|frameworks| !frameworks.is_empty() && frameworks.len() <= 8)
        .ok_or_else(|| {
            preparation_error("Invalid self-contained .NET includedFrameworks declaration.")
        })?;
    for framework in frameworks {
        if framework
            .get("name")
            .and_then(serde_json::Value::as_str)
            .is_none_or(|name| name.trim().is_empty())
            || framework
                .get("version")
                .and_then(serde_json::Value::as_str)
                .is_none_or(|version| version.trim().is_empty())
        {
            return Err(preparation_error(
                "Invalid self-contained .NET framework identity.",
            ));
        }
    }
    // Additional self-contained frameworks come from the game package. Their
    // presence does not request installation of a global framework and must not
    // reject a healthy package simply because our download catalog omits it.
    if !frameworks.iter().any(|framework| {
        framework.get("name").and_then(serde_json::Value::as_str) == Some("Microsoft.NETCore.App")
    }) {
        return Ok(());
    }
    let root = config_path.parent().ok_or_else(|| {
        preparation_error("The .NET runtime configuration has no package directory.")
    })?;
    if [
        "coreclr.dll",
        "hostpolicy.dll",
        "hostfxr.dll",
        "System.Private.CoreLib.dll",
    ]
    .iter()
    .any(|file| !root.join(file).is_file())
    {
        return Err(preparation_error(format!(
            "The self-contained .NET server runtime is incomplete beside {}. Repair the server program before starting.",
            config_path.display()
        )));
    }
    Ok(())
}

fn requirement(config: &serde_json::Value) -> Result<Requirement, SteamCmdError> {
    let options = config
        .get("runtimeOptions")
        .ok_or_else(|| preparation_error("Server .NET runtimeOptions are missing."))?;
    if options.get("framework").is_some() && options.get("frameworks").is_some() {
        return Err(preparation_error(
            "The server runtime configuration declares both framework and frameworks.",
        ));
    }
    let frameworks = match options.get("frameworks") {
        Some(value) => value
            .as_array()
            .filter(|frameworks| !frameworks.is_empty() && frameworks.len() <= 8)
            .ok_or_else(|| preparation_error("The server has an invalid .NET framework list."))?
            .iter()
            .collect::<Vec<_>>(),
        None => vec![
            options
                .get("framework")
                .ok_or_else(|| preparation_error("Server .NET framework is missing."))?,
        ],
    };
    let mut required: Option<Requirement> = None;
    for framework in frameworks {
        let next = framework_requirement(options, framework)?;
        required = Some(match required {
            Some(current) => current.merge(next)?,
            None => next,
        });
    }
    required.ok_or_else(|| preparation_error("Server .NET framework is missing."))
}

fn framework_requirement(
    options: &serde_json::Value,
    framework: &serde_json::Value,
) -> Result<Requirement, SteamCmdError> {
    if framework.get("name").and_then(serde_json::Value::as_str) != Some("Microsoft.NETCore.App") {
        return Err(preparation_error(
            "The server requires an unsupported .NET framework. Automatic runtime preparation currently supports Microsoft.NETCore.App.",
        ));
    }
    let version = framework.get("version").and_then(serde_json::Value::as_str).and_then(stable_version)
        .filter(|version| (3..=99).contains(&version[0]) && version[1] <= 99)
        .ok_or_else(|| preparation_error("The server requires an unsupported .NET version; a stable runtime version is required."))?;
    let roll_value = framework
        .get("rollForward")
        .or_else(|| options.get("rollForward"));
    let patches_value = framework
        .get("applyPatches")
        .or_else(|| options.get("applyPatches"));
    if roll_value.is_some() && patches_value.is_some() {
        return Err(preparation_error(
            "The .NET runtime configuration cannot combine rollForward and applyPatches.",
        ));
    }
    let roll = match roll_value {
        Some(value) => value
            .as_str()
            .ok_or_else(|| preparation_error("The .NET rollForward policy must be a string."))?,
        None => "Minor",
    };
    let roll = roll.to_ascii_lowercase();
    if !matches!(
        roll.as_str(),
        "disable" | "latestpatch" | "minor" | "latestminor" | "major" | "latestmajor"
    ) {
        return Err(preparation_error(
            "The server has an unsupported .NET rollForward policy.",
        ));
    }
    let apply_patches = match patches_value {
        Some(value) => value
            .as_bool()
            .ok_or_else(|| preparation_error("The .NET applyPatches policy must be a boolean."))?,
        None => true,
    };
    Ok(Requirement {
        version,
        exact: roll == "disable",
        prefer_lowest: !apply_patches,
    })
}

fn stable_version(value: &str) -> Option<[u32; 3]> {
    let mut parts = value.split('.');
    let version = [
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    ];
    parts.next().is_none().then_some(version)
}

fn display_version(version: [u32; 3]) -> String {
    format!("{}.{}.{}", version[0], version[1], version[2])
}

fn select_package(text: &str, required: Requirement) -> Result<Package, String> {
    let metadata: serde_json::Value = serde_json::from_str(text)
        .map_err(|error| format!("Invalid .NET release metadata: {error}"))?;
    if metadata
        .get("channel-version")
        .and_then(serde_json::Value::as_str)
        != Some(required.channel().as_str())
    {
        return Err("Invalid .NET release channel.".into());
    }
    let releases = metadata
        .get("releases")
        .and_then(serde_json::Value::as_array)
        .filter(|releases| releases.len() <= 256)
        .ok_or("Invalid .NET release list.")?;
    let mut packages = Vec::new();
    for release in releases {
        let Some(runtime) = release.get("runtime") else {
            continue;
        };
        let Some(version) = runtime
            .get("version")
            .and_then(serde_json::Value::as_str)
            .and_then(stable_version)
        else {
            continue;
        };
        if !required.accepts(version) {
            continue;
        }
        let files = runtime
            .get("files")
            .and_then(serde_json::Value::as_array)
            .filter(|files| files.len() <= 128)
            .ok_or("Invalid .NET runtime file list.")?;
        let mut matching = files.iter().filter(|file| {
            file.get("rid").and_then(serde_json::Value::as_str) == Some("win-x64")
                && file.get("name").and_then(serde_json::Value::as_str)
                    == Some("dotnet-runtime-win-x64.zip")
        });
        let Some(file) = matching.next() else {
            continue;
        };
        if matching.next().is_some() {
            return Err("Duplicate .NET Windows x64 runtime package.".into());
        }
        let url = file
            .get("url")
            .and_then(serde_json::Value::as_str)
            .ok_or("Missing .NET package URL.")?;
        let hash = file
            .get("hash")
            .and_then(serde_json::Value::as_str)
            .ok_or("Missing .NET package SHA-512.")?;
        if hash.len() != 128
            || !hash.bytes().all(|byte| byte.is_ascii_hexdigit())
            || !official_package_url(url, version)
        {
            return Err("Invalid .NET runtime package URL or SHA-512.".into());
        }
        packages.push(Package {
            version,
            url: url.to_owned(),
            hash: hash.to_ascii_lowercase(),
        });
    }
    packages.sort_by_key(|package| package.version);
    if required.prefer_lowest {
        packages.reverse();
    }
    let latest = packages
        .pop()
        .ok_or("No Microsoft .NET package matches the installed game's runtime requirement.")?;
    if packages
        .last()
        .is_some_and(|package| package.version == latest.version)
    {
        return Err("Duplicate .NET runtime version.".into());
    }
    Ok(latest)
}

fn official_package_url(value: &str, version: [u32; 3]) -> bool {
    let Ok(url) = reqwest::Url::parse(value) else {
        return false;
    };
    url.scheme() == "https"
        && matches!(
            url.host_str(),
            Some(
                "builds.dotnet.microsoft.com"
                    | "dotnetcli.blob.core.windows.net"
                    | "download.visualstudio.microsoft.com"
            )
        )
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && url.path().ends_with(&format!(
            "/dotnet-runtime-{}-win-x64.zip",
            display_version(version)
        ))
}

async fn valid_runtime(
    root: &Path,
    required: Requirement,
    owned_root: &Path,
    deadline: InstallDeadline,
) -> Result<bool, SteamCmdError> {
    if !root.join("dotnet.exe").is_file() {
        return Ok(false);
    }
    let mut command = Command::new(root.join("dotnet.exe"));
    command
        .arg("--list-runtimes")
        .env("DOTNET_MULTILEVEL_LOOKUP", "0");
    let output =
        match run_command_capture(command, deadline.limited_to(Duration::from_secs(15))).await {
            Ok(output) => output,
            Err(SteamCmdError::SpawnCommand { .. }) => return Ok(false),
            Err(error) => return Err(error),
        };
    if !output.status.success() {
        return Ok(false);
    }
    let mut versions = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            if words.next() != Some("Microsoft.NETCore.App") {
                return None;
            }
            words
                .next()
                .and_then(stable_version)
                .filter(|version| required.accepts(*version))
        })
        .collect::<Vec<_>>();
    versions.sort();
    if !required.prefer_lowest {
        versions.reverse();
    }
    let Some(version) = versions.first().copied() else {
        return Ok(false);
    };
    if [
        "coreclr.dll",
        "hostpolicy.dll",
        "System.Private.CoreLib.dll",
    ]
    .iter()
    .any(|file| {
        !root
            .join("shared/Microsoft.NETCore.App")
            .join(display_version(version))
            .join(file)
            .is_file()
    }) {
        return Ok(false);
    }
    probe::execute(owned_root, root, version, deadline).await
}

fn runtime(root: PathBuf) -> PortableRuntime {
    let value = root.to_string_lossy().into_owned();
    PortableRuntime {
        executable: None,
        environment: BTreeMap::from([
            ("DOTNET_ROOT".into(), value.clone()),
            ("DOTNET_ROOT_X64".into(), value),
        ]),
    }
}

fn existing_roots(cache: &Path, tools: &Path) -> Vec<PathBuf> {
    let mut roots = cache_roots(cache, "dotnet");
    roots.push(tools.join("dotnet"));
    for variable in ["DOTNET_ROOT_X64", "DOTNET_ROOT"] {
        if let Some(value) = env::var_os(variable) {
            let path = PathBuf::from(value);
            if path.is_absolute() {
                roots.push(path);
            }
        }
    }
    if let Some(program_files) = env::var_os("ProgramFiles") {
        let path = PathBuf::from(program_files);
        if path.is_absolute() {
            roots.push(path.join("dotnet"));
        }
    }
    roots.dedup();
    roots
}

#[cfg(test)]
#[path = "portable_dotnet_runtime_tests.rs"]
mod tests;
