use std::fs;
use std::path::{Path, PathBuf};

use super::install_publication_metadata::{
    InstallPublicationMetadata, MAX_RETAINED_LIBRARY_BYTES, RETAINED_LIBRARY_MARKER,
};
use super::{SteamCmdError, ps_literal};

#[cfg(test)]
pub(super) fn direct_download_publish_script(
    install_root: &Path,
    staging_root: &Path,
    rollback_root: &Path,
    publish_phase_path: &Path,
    staging_verification: &Path,
    archive_path: &Path,
    metadata: &InstallPublicationMetadata,
) -> String {
    install_publish_script(
        install_root,
        staging_root,
        rollback_root,
        publish_phase_path,
        staging_verification,
        PayloadPreparation::Archive(archive_path),
        metadata,
    )
}

pub(super) fn direct_download_stage_script(
    install_root: &Path,
    staging_root: &Path,
    rollback_root: &Path,
    publish_phase_path: &Path,
    staging_verification: &Path,
    archive_path: &Path,
    metadata: &InstallPublicationMetadata,
) -> String {
    install_publish_script(
        install_root,
        staging_root,
        rollback_root,
        publish_phase_path,
        staging_verification,
        PayloadPreparation::StageArchive(archive_path),
        metadata,
    )
}

pub(super) fn prepared_install_publish_script(
    install_root: &Path,
    staging_root: &Path,
    rollback_root: &Path,
    publish_phase_path: &Path,
    staging_verification: &Path,
    metadata: &InstallPublicationMetadata,
) -> String {
    install_publish_script(
        install_root,
        staging_root,
        rollback_root,
        publish_phase_path,
        staging_verification,
        PayloadPreparation::Prepared,
        metadata,
    )
}

enum PayloadPreparation<'a> {
    #[cfg(test)]
    Archive(&'a Path),
    StageArchive(&'a Path),
    Prepared,
}

fn install_publish_script(
    install_root: &Path,
    staging_root: &Path,
    rollback_root: &Path,
    publish_phase_path: &Path,
    staging_verification: &Path,
    preparation: PayloadPreparation<'_>,
    metadata: &InstallPublicationMetadata,
) -> String {
    let (archive_path, stage_only) = match preparation {
        #[cfg(test)]
        PayloadPreparation::Archive(path) => (Some(path), false),
        PayloadPreparation::StageArchive(path) => (Some(path), true),
        PayloadPreparation::Prepared => (None, false),
    };
    format!(
        r#"$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
$root={root}
$stage={stage}
$rollback={rollback}
$phase={phase}
$verify={verify}
$zip={zip}
$stageOnly={stage_only}
$staged=$false
$preserveData={preserve_data}
$marker={retained_marker}
$libraryMarker={library_marker}
$libraryHash={library_hash}
$libraryLimit={library_limit}
function Assert-PlainItem($item) {{
  if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0 -or $item.LinkType) {{
    throw 'Installation data contains a link or reparse point.'
  }}
}}
function Assert-PlainPathChain([string]$path) {{
  $item = Get-Item -LiteralPath $path -Force
  while ($null -ne $item) {{
    Assert-PlainItem $item
    $item = if ($item -is [IO.DirectoryInfo]) {{ $item.Parent }} else {{ $item.Directory }}
  }}
}}
function Assert-PlainTree([string]$path) {{
  $item = Get-Item -LiteralPath $path -Force
  Assert-PlainItem $item
  if ($item -is [IO.DirectoryInfo]) {{
    foreach ($child in Get-ChildItem -LiteralPath $path -Force) {{ Assert-PlainTree $child.FullName }}
  }}
}}
$stage = [IO.Path]::GetFullPath($stage)
$stagePrefix = $stage.TrimEnd('\') + '\'
$root = [IO.Path]::GetFullPath($root)
$verify = [IO.Path]::GetFullPath($verify)
function Get-StagedPath([string]$relative) {{
  if ([IO.Path]::IsPathRooted($relative) -or $relative.Contains(':')) {{
    throw 'Installation path escapes the staging directory.'
  }}
  $target = [IO.Path]::GetFullPath((Join-Path $stage $relative))
  if (-not $target.StartsWith($stagePrefix, [StringComparison]::OrdinalIgnoreCase)) {{
    throw 'Installation path escapes the staging directory.'
  }}
  return $target
}}
function Copy-RetainedData([string]$source, [string]$relative) {{
  foreach ($item in Get-ChildItem -LiteralPath $source -Force) {{
    Assert-PlainItem $item
    if (-not $relative -and ($item.Name -eq $marker -or $item.Name -eq $libraryMarker)) {{ continue }}
    $childRelative = if ($relative) {{ Join-Path $relative $item.Name }} else {{ $item.Name }}
    $target = Get-StagedPath $childRelative
    if (-not $relative -and
        ($item.Name -eq '.langame-initial-package.json' -or $item.Name -eq '.langame-clean-package.json') -and
        (Test-Path -LiteralPath $target)) {{
      throw 'Retained data conflicts with fresh manager-owned package inventory.'
    }}
    if ($target.Equals($verify, [StringComparison]::OrdinalIgnoreCase) -or
        $target.StartsWith($verify + '\', [StringComparison]::OrdinalIgnoreCase) -or
        ($item -isnot [IO.DirectoryInfo] -and $verify.StartsWith($target + '\', [StringComparison]::OrdinalIgnoreCase))) {{
      throw 'Retained data conflicts with the required server verification file.'
    }}
    if (Test-Path -LiteralPath $target) {{
      $existing = Get-Item -LiteralPath $target -Force
      Assert-PlainItem $existing
      if (($item -is [IO.DirectoryInfo]) -ne ($existing -is [IO.DirectoryInfo])) {{
        # Only this operation's checked staging tree may be replaced by retained data.
        Assert-PlainTree $target
        Remove-Item -LiteralPath $target -Recurse -Force
      }}
    }}
    if ($item -is [IO.DirectoryInfo]) {{
      New-Item -ItemType Directory -Force -Path $target | Out-Null
      Copy-RetainedData $item.FullName $childRelative
    }} else {{
      [IO.File]::Copy($item.FullName, $target, $true)
    }}
  }}
}}
function Copy-RetainedLibraryMarker {{
  $source = Join-Path $root $libraryMarker
  $target = Join-Path $stage $libraryMarker
  if (Test-Path -LiteralPath $target) {{ throw 'Payload contains a retained-library marker.' }}
  if ($null -eq $libraryHash) {{
    if (Test-Path -LiteralPath $source) {{ throw 'Retained library ownership changed during installation.' }}
    return
  }}
  Assert-PlainPathChain $source
  $libraryInput = [IO.File]::Open($source, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
  try {{
    if ($libraryInput.Length -gt $libraryLimit) {{ throw 'Retained library marker exceeds its size limit.' }}
    $sha = [Security.Cryptography.SHA256]::Create()
    try {{ $actual = [BitConverter]::ToString($sha.ComputeHash($libraryInput)).Replace('-', '').ToLowerInvariant() }}
    finally {{ $sha.Dispose() }}
    if ($actual -ne $libraryHash) {{ throw 'Retained library ownership changed during installation.' }}
    $libraryInput.Position = 0
    $libraryOutput = [IO.File]::Open($target, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
    try {{ $libraryInput.CopyTo($libraryOutput); $libraryOutput.Flush($true) }} finally {{ $libraryOutput.Dispose() }}
  }} finally {{ $libraryInput.Dispose() }}
}}
Assert-PlainPathChain ([IO.Path]::GetDirectoryName($root))
if (Test-Path -LiteralPath $root) {{ Assert-PlainPathChain $root }}
if ($null -ne $zip -and (Test-Path -LiteralPath $stage)) {{
  throw 'Staging path already exists.'
}}
if ($null -eq $zip -and -not (Test-Path -LiteralPath $stage -PathType Container)) {{
  throw 'Prepared staging directory is missing.'
}}
if (Test-Path -LiteralPath $rollback) {{
  throw 'Rollback path already exists.'
}}
if (Test-Path -LiteralPath $phase) {{
  throw 'Publish phase path already exists.'
}}
if ($null -ne $zip) {{ New-Item -ItemType Directory -Path $stage | Out-Null }}
try {{
  if ($null -ne $zip) {{
  Add-Type -AssemblyName System.IO.Compression,System.IO.Compression.FileSystem
  $archive = [IO.Compression.ZipFile]::OpenRead($zip)
  try {{
    foreach ($entry in $archive.Entries) {{
      $entryPath = Get-StagedPath $entry.FullName
      if ((($entry.ExternalAttributes -shr 16) -band 0xF000) -eq 0xA000 -or
          ($entry.ExternalAttributes -band 0x400) -ne 0) {{
        throw 'Archive contains a link or reparse point.'
      }}
      if ($entryPath.Equals((Join-Path $stage $marker), [StringComparison]::OrdinalIgnoreCase)) {{
        throw 'Archive contains a retained-data marker.'
      }}
      if ($entryPath.Equals((Join-Path $stage $libraryMarker), [StringComparison]::OrdinalIgnoreCase)) {{
        throw 'Archive contains a retained-library marker.'
      }}
      foreach ($inventory in @('.langame-initial-package.json', '.langame-clean-package.json')) {{
        if ($entryPath.Equals((Join-Path $stage $inventory), [StringComparison]::OrdinalIgnoreCase)) {{
          throw 'Archive contains manager-owned package inventory.'
        }}
      }}
    }}
  }} finally {{ $archive.Dispose() }}
  Expand-Archive -LiteralPath $zip -DestinationPath $stage -Force
  }}
  Assert-PlainTree $stage
  if (Test-Path -LiteralPath (Join-Path $stage $marker)) {{ throw 'Payload contains a retained-data marker.' }}
  if (Test-Path -LiteralPath (Join-Path $stage $libraryMarker)) {{ throw 'Payload contains a retained-library marker.' }}
  if (-not (Test-Path -LiteralPath $verify -PathType Leaf)) {{ throw 'Required server file is missing from staged payload.' }}
  if ($stageOnly) {{ $staged=$true; return }}
  if ($preserveData) {{
    if (-not (Test-Path -LiteralPath (Join-Path $root $marker) -PathType Leaf)) {{
      throw 'Retained installation data marker disappeared before publication.'
    }}
    Copy-RetainedData $root ''
  }}
  Copy-RetainedLibraryMarker
  if (Test-Path -LiteralPath $root) {{
    [IO.File]::WriteAllText($phase, 'rollback_pending')
    Move-Item -LiteralPath $root -Destination $rollback
  }}
  try {{
    Move-Item -LiteralPath $stage -Destination $root
  }} catch {{
    if (Test-Path -LiteralPath $rollback) {{ Move-Item -LiteralPath $rollback -Destination $root }}
    Remove-Item -LiteralPath $phase -Force -ErrorAction SilentlyContinue
    throw
  }}
}} finally {{
  if ($null -ne $zip) {{ Remove-Item -LiteralPath $zip -Force -ErrorAction SilentlyContinue }}
  if (-not $staged -and (Test-Path -LiteralPath $stage)) {{ Remove-Item -LiteralPath $stage -Recurse -Force -ErrorAction SilentlyContinue }}
}}
"#,
        root = ps_literal(install_root),
        stage = ps_literal(staging_root),
        rollback = ps_literal(rollback_root),
        phase = ps_literal(publish_phase_path),
        verify = ps_literal(staging_verification),
        zip = archive_path
            .map(ps_literal)
            .unwrap_or_else(|| String::from("$null")),
        stage_only = if stage_only { "$true" } else { "$false" },
        preserve_data = if metadata.preserve_retained_data {
            "$true"
        } else {
            "$false"
        },
        retained_marker = ps_literal(Path::new(super::RETAINED_INSTALL_DATA_MARKER)),
        library_marker = ps_literal(Path::new(RETAINED_LIBRARY_MARKER)),
        library_hash = metadata
            .retained_library_sha256
            .as_ref()
            .map(|hash| format!("'{hash}'"))
            .unwrap_or_else(|| String::from("$null")),
        library_limit = MAX_RETAINED_LIBRARY_BYTES,
    )
}

pub(super) async fn recover_direct_download_install(
    install_root: &Path,
    staging_root: &Path,
    rollback_root: &Path,
    rejected_root: &Path,
    publish_phase_path: &Path,
    archive_path: &Path,
    published_by_operation: bool,
) -> Result<(), SteamCmdError> {
    let _ = fs::remove_file(archive_path);
    recover_install_publication(
        install_root,
        staging_root,
        rollback_root,
        rejected_root,
        publish_phase_path,
        published_by_operation,
    )
    .await
}

pub(super) async fn recover_install_publication(
    install_root: &Path,
    staging_root: &Path,
    rollback_root: &Path,
    rejected_root: &Path,
    publish_phase_path: &Path,
    published_by_operation: bool,
) -> Result<(), SteamCmdError> {
    if rollback_root.exists() && !published_by_operation && !publish_phase_path.exists() {
        await_failed_directory_cleanup(vec![staging_root.to_path_buf()]).await;
        return Err(SteamCmdError::InstallRollbackFailed {
            path: install_root.to_path_buf(),
            detail: format!(
                "rollback {} exists without this operation's publish phase marker",
                rollback_root.display()
            ),
        });
    }
    let quarantined = match restore_published_directory(
        install_root,
        rollback_root,
        rejected_root,
        published_by_operation,
    ) {
        Ok(quarantined) => quarantined,
        Err(recovery_error) => {
            await_failed_directory_cleanup(vec![staging_root.to_path_buf()]).await;
            return Err(recovery_error);
        }
    };
    let _ = fs::remove_file(publish_phase_path);

    let mut cleanup_paths = vec![staging_root.to_path_buf()];
    cleanup_paths.extend(quarantined);
    await_failed_directory_cleanup(cleanup_paths).await;
    Ok(())
}

pub(super) fn restore_published_directory(
    published_root: &Path,
    rollback_root: &Path,
    rejected_root: &Path,
    published_by_operation: bool,
) -> Result<Option<PathBuf>, SteamCmdError> {
    if !rollback_root.exists() {
        if published_by_operation && published_root.exists() {
            fs::rename(published_root, rejected_root).map_err(|source| {
                SteamCmdError::InstallRollbackFailed {
                    path: published_root.to_path_buf(),
                    detail: format!("failed to quarantine unverified payload: {source}"),
                }
            })?;
            return Ok(Some(rejected_root.to_path_buf()));
        }
        return Ok(None);
    }
    if rejected_root.exists() {
        return Err(SteamCmdError::InstallRollbackFailed {
            path: published_root.to_path_buf(),
            detail: format!(
                "rollback quarantine path already exists: {}",
                rejected_root.display()
            ),
        });
    }

    let published_was_moved = if published_root.exists() {
        fs::rename(published_root, rejected_root).map_err(|source| {
            SteamCmdError::InstallRollbackFailed {
                path: published_root.to_path_buf(),
                detail: format!("failed to quarantine the newly published payload: {source}"),
            }
        })?;
        true
    } else {
        false
    };

    if let Err(source) = fs::rename(rollback_root, published_root) {
        let restore_new_result = if published_was_moved {
            fs::rename(rejected_root, published_root)
        } else {
            Ok(())
        };
        let restore_new_detail = restore_new_result
            .err()
            .map(|error| format!("; restoring the new payload also failed: {error}"))
            .unwrap_or_default();
        return Err(SteamCmdError::InstallRollbackFailed {
            path: published_root.to_path_buf(),
            detail: format!(
                "failed to restore {}: {source}{restore_new_detail}",
                rollback_root.display()
            ),
        });
    }

    Ok(published_was_moved.then(|| rejected_root.to_path_buf()))
}

pub(super) fn schedule_verified_directory_cleanup(paths: Vec<PathBuf>) {
    // Verified payloads use operation-unique rollback paths. Cleanup may safely
    // outlive the install guard because no future operation reuses these paths.
    tokio::spawn(async move {
        let _ = tokio::task::spawn_blocking(move || remove_directories(paths)).await;
    });
}

pub(super) async fn await_failed_directory_cleanup(paths: Vec<PathBuf>) {
    // The operating system cannot cancel a recursive deletion halfway through.
    // Keep ownership until it finishes, even after cancellation, so returning a
    // stopped operation never leaves a detached file worker behind.
    let worker = tokio::task::spawn_blocking(move || remove_directories(paths));
    let _ = worker.await;
}

fn remove_directories(paths: Vec<PathBuf>) {
    for path in paths {
        let _ = fs::remove_dir_all(path);
    }
}

pub(super) async fn finish_published_jre(
    validation: Result<Option<u16>, SteamCmdError>,
    required_major: u16,
    jre_root: &Path,
    rollback_root: &Path,
    rejected_root: &Path,
    staging_root: &Path,
) -> Result<(), SteamCmdError> {
    if let Err(error @ SteamCmdError::InstallProcessCleanupFailed { .. }) = validation {
        return Err(error);
    }
    let validation_error = match validation {
        Ok(Some(published_major)) if published_major == required_major => {
            schedule_verified_directory_cleanup(vec![
                rollback_root.to_path_buf(),
                staging_root.to_path_buf(),
            ]);
            return Ok(());
        }
        Ok(published_major) => SteamCmdError::MinecraftJrePreparation {
            detail: format!(
                "published Java runtime reports {:?}, expected {required_major}",
                published_major
            ),
        },
        Err(error) => error,
    };

    let quarantined =
        match restore_published_directory(jre_root, rollback_root, rejected_root, true) {
            Ok(quarantined) => quarantined,
            Err(recovery_error) => {
                await_failed_directory_cleanup(vec![staging_root.to_path_buf()]).await;
                return Err(recovery_error);
            }
        };
    let mut cleanup_paths = vec![staging_root.to_path_buf()];
    cleanup_paths.extend(quarantined);
    await_failed_directory_cleanup(cleanup_paths).await;
    Err(validation_error)
}
