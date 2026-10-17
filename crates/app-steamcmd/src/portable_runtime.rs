//! Startup runtime preparation is independent of the selected game version.
use std::collections::BTreeMap;
use std::io::Read;
use std::sync::atomic::{AtomicU64, Ordering};

use super::*;
use crate::http_download::source_unavailable;
use app_core::InstallPhase;

#[path = "portable_dotnet_runtime.rs"]
mod dotnet;
#[path = "portable_java_runtime.rs"]
mod java;

const PREPARATION_BUDGET: Duration = Duration::from_secs(10 * 60);
const MARKER: &str = ".langame-runtime.json";
static SCRATCH_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Default)]
pub struct PortableRuntime {
    pub executable: Option<PathBuf>,
    pub environment: BTreeMap<String, String>,
}

pub async fn prepare_portable_runtime<F>(
    lock_context: &InstallLockContext,
    settings: &AppSettings,
    module_id: &str,
    install_root: &Path,
    executables: &[PathBuf],
    cancellation: InstallCancellation,
    mut on_progress: F,
) -> Result<Option<PortableRuntime>, SteamCmdError>
where
    F: FnMut(InstallProgressUpdate),
{
    cancellation.scope(async {
        let deadline = InstallDeadline::new("preparing server runtime", PREPARATION_BUDGET);
        deadline.check_cancelled()?;
        if matches!(module_id, "necesse" | "projectzomboid") {
            let relative = if module_id == "necesse" { "jre/bin/java.exe" } else { "jre64/bin/java.exe" };
            on_progress(InstallProgressUpdate::stage(InstallPhase::Verifying, "Verifying the game-provided Java runtime..."));
            if java::probe_major(&install_root.join(relative), deadline).await?.is_some() {
                deadline.check_cancelled()?;
                return Ok(None);
            }
            let bundled_root = install_root.join(if module_id == "necesse" { "jre" } else { "jre64" });
            let major = java::bundled_major(&bundled_root)?;
            let tools = tools_root(settings)?;
            on_progress(InstallProgressUpdate::stage(InstallPhase::Preparing,
                format!("The game-provided Java runtime is damaged; preparing compatible Java {major} from its release metadata...")));
            let runtime = java::prepare_major(lock_context, tools, major, &bundled_root, deadline, &mut on_progress).await?;
            deadline.check_cancelled()?;
            return Ok(Some(runtime));
        }
        let runtime = if module_id == "minecraft" {
            java::prepare(lock_context, tools_root(settings)?, install_root, deadline, &mut on_progress).await?
        } else {
            let Some(requirement) = dotnet::discover(executables)? else { return Ok(None); };
            dotnet::prepare(lock_context, tools_root(settings)?, requirement, deadline, &mut on_progress).await?
        };
        deadline.check_cancelled()?;
        Ok(Some(runtime))
    }).await
}

fn tools_root(settings: &AppSettings) -> Result<&Path, SteamCmdError> {
    Path::new(&settings.steamcmd_root)
        .parent()
        .filter(|root| root.is_absolute())
        .ok_or_else(|| preparation_error("Configured SteamCMD has no absolute tools parent."))
}

fn preparation_error(detail: impl Into<String>) -> SteamCmdError {
    SteamCmdError::RuntimePreparation {
        detail: detail.into(),
    }
}

fn read_json(path: &Path) -> Result<serde_json::Value, SteamCmdError> {
    let file = fs::File::open(path)
        .map_err(|error| preparation_error(format!("Cannot read {}: {error}", path.display())))?;
    let mut bytes = Vec::new();
    file.take(65_537)
        .read_to_end(&mut bytes)
        .map_err(|error| preparation_error(format!("Cannot read {}: {error}", path.display())))?;
    if bytes.len() > 65_536 {
        return Err(preparation_error(format!(
            "{} exceeds the 64 KiB runtime metadata limit",
            path.display()
        )));
    }
    serde_json::from_slice(&bytes).map_err(|error| {
        preparation_error(format!(
            "Invalid runtime metadata {}: {error}",
            path.display()
        ))
    })
}

async fn acquire_runtime_root(
    context: &InstallLockContext,
    root: &Path,
    identity: &str,
    deadline: InstallDeadline,
) -> Result<ResourceLocks, SteamCmdError> {
    let guard = ResourceLocks::acquire(context, &[identity], &[root.to_owned()], deadline).await?;
    // Do not claim an operator's nonempty tool directory or follow a reparse
    // point into another tree. Only our created scratch directories are removed.
    for ancestor in root.ancestors().take_while(|path| path.parent().is_some()) {
        if let Ok(metadata) = fs::symlink_metadata(ancestor)
            && (metadata.file_type().is_symlink() || is_reparse(&metadata))
        {
            return Err(preparation_error(
                "Portable runtime paths must not contain reparse points.",
            ));
        }
    }
    let marker = root.join(".langame-portable-runtime-root");
    if root.exists()
        && !marker.is_file()
        && fs::read_dir(root)
            .map_err(|error| preparation_error(error.to_string()))?
            .next()
            .is_some()
    {
        return Err(preparation_error(format!(
            "Unmanaged nonempty runtime directory was preserved: {}",
            root.display()
        )));
    }
    fs::create_dir_all(root).map_err(|source| SteamCmdError::CreatePath {
        path: root.to_owned(),
        source,
    })?;
    if !marker.exists() {
        fs::write(marker, b"LanGame portable runtime root v1\n")
            .map_err(|error| preparation_error(error.to_string()))?;
    }
    Ok(guard)
}

fn is_reparse(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        let _ = metadata;
        false
    }
}

fn scratch(root: &Path, suffix: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let sequence = SCRATCH_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    root.join(format!(
        ".lg-runtime-{:x}-{stamp:x}-{sequence:x}.{suffix}",
        std::process::id()
    ))
}

fn cache_roots(root: &Path, kind: &str) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut candidates = entries
        .take(256)
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).ok()?;
            if !metadata.is_dir() || metadata.file_type().is_symlink() || is_reparse(&metadata) {
                return None;
            }
            let marker = read_json(&path.join(MARKER)).ok()?;
            if marker.get("kind")?.as_str()? != kind {
                return None;
            }
            Some((marker.get("published_at")?.as_u64()?, path))
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    candidates.into_iter().map(|(_, root)| root).collect()
}

fn matches_identity(root: &Path, kind: &str, identity: &str) -> bool {
    read_json(&root.join(MARKER)).is_ok_and(|marker| {
        marker.get("kind").and_then(serde_json::Value::as_str) == Some(kind)
            && marker.get("identity").and_then(serde_json::Value::as_str) == Some(identity)
    })
}

fn publish_runtime(
    stage: &Path,
    root: &Path,
    directory: &str,
    kind: &str,
    identity: &str,
) -> Result<PathBuf, SteamCmdError> {
    let mut destination = root.join(directory);
    if destination.exists() {
        // Published generations are immutable, even if their verification now
        // fails. A running process may still have their native libraries open.
        destination = root.join(format!(
            "{directory}-{}",
            scratch(root, "generation")
                .file_name()
                .ok_or_else(|| preparation_error("Invalid runtime generation path"))?
                .to_string_lossy()
        ));
    }
    let published_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let marker =
        serde_json::json!({"kind": kind, "identity": identity, "published_at": published_at});
    fs::write(stage.join(MARKER), marker.to_string())
        .map_err(|error| preparation_error(format!("Cannot record runtime identity: {error}")))?;
    fs::rename(stage, &destination)
        .map_err(|error| preparation_error(format!("Cannot publish runtime: {error}")))?;
    Ok(destination)
}

fn degraded(
    on_progress: &mut impl FnMut(InstallProgressUpdate),
    name: &str,
    error: &SteamCmdError,
) {
    on_progress(InstallProgressUpdate::stage(
        InstallPhase::Verifying,
        format!("Unable to check {name} updates; reusing the verified installed runtime: {error}"),
    ));
}

async fn extract_archive(
    archive: &Path,
    stage: &Path,
    deadline: InstallDeadline,
) -> Result<(), SteamCmdError> {
    let script = bounded_extraction_script(archive, stage);
    let output = run_powershell(&script, stage.parent(), deadline).await?;
    if !output.status.success() {
        return Err(preparation_error(format!(
            "Runtime archive extraction failed: {}",
            output_excerpt(&output.stdout, &output.stderr)
        )));
    }
    deadline.check_cancelled()
}

fn bounded_extraction_script(archive: &Path, stage: &Path) -> String {
    let script = r#"$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.IO.Compression.FileSystem
$archive=__ARCHIVE__
$stage=__STAGE__
if ([IO.Directory]::Exists($stage)) { throw 'Runtime staging directory already exists.' }
[IO.Directory]::CreateDirectory($stage) | Out-Null
$prefix=[IO.Path]::GetFullPath($stage).TrimEnd('\')+'\'
$zip=[IO.Compression.ZipFile]::OpenRead($archive)
try {
  if ($zip.Entries.Count -gt 50000) { throw 'Runtime archive has too many entries.' }
  $seen=New-Object 'System.Collections.Generic.HashSet[string]' ([StringComparer]::OrdinalIgnoreCase)
  [long]$total=0
  foreach ($entry in $zip.Entries) {
    $name=$entry.FullName.Replace('/','\')
    $parts=$name.TrimEnd('\').Split('\')
    if ([IO.Path]::IsPathRooted($name) -or $parts.Count -eq 0) { throw 'Absolute archive entry.' }
    foreach ($part in $parts) {
      if ($part -eq '' -or $part -eq '.' -or $part -eq '..' -or $part.EndsWith('.') -or $part.EndsWith(' ') -or $part.IndexOfAny([IO.Path]::GetInvalidFileNameChars()) -ge 0 -or $part -match '^(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(\.|$)') { throw 'Unsafe archive path.' }
    }
    $target=[IO.Path]::GetFullPath([IO.Path]::Combine($stage,$name))
    if (-not $target.StartsWith($prefix,[StringComparison]::OrdinalIgnoreCase) -or -not $seen.Add($target.TrimEnd('\'))) { throw 'Duplicate or escaping archive entry.' }
    $unixType=($entry.ExternalAttributes -shr 16) -band 61440
    if ($unixType -eq 40960 -or ($entry.ExternalAttributes -band 1024) -ne 0) { throw 'Archive links are not supported.' }
    $total+=$entry.Length
    if ($entry.Length -lt 0 -or $total -gt 2147483648) { throw 'Runtime archive exceeds the 2 GiB expanded limit.' }
    if ($name.EndsWith('\')) { [IO.Directory]::CreateDirectory($target) | Out-Null; continue }
    [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($target)) | Out-Null
    $input=$entry.Open()
    try {
      $output=[IO.File]::Open($target,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None)
      try {
        $buffer=New-Object byte[] 65536
        [long]$copied=0
        while (($count=$input.Read($buffer,0,$buffer.Length)) -gt 0) {
          $copied+=$count
          if ($copied -gt $entry.Length) { throw 'Archive entry exceeds its declared length.' }
          $output.Write($buffer,0,$count)
        }
        if ($copied -ne $entry.Length) { throw 'Archive entry is incomplete.' }
      } finally { $output.Dispose() }
    } finally { $input.Dispose() }
  }
} finally { $zip.Dispose() }
"#;
    script
        .replace("__ARCHIVE__", &ps_literal(archive))
        .replace("__STAGE__", &ps_literal(stage))
}

#[cfg(test)]
#[path = "portable_runtime_tests.rs"]
mod tests;

#[cfg(all(test, windows))]
#[path = "portable_runtime_sandbox_tests.rs"]
mod sandbox_tests;
