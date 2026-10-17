# Shared by the control builders; dot-sourcing this file performs no writes.
function ConvertTo-ControlBuildPath([string]$Path, [bool]$LocalOnly = $false) {
    if ($Path -notmatch '^(?:[A-Za-z]:[\\/]|[\\/]{2}[^\\/\r\n]+[\\/][^\\/\r\n]+(?:[\\/]|$))' -or
        ($LocalOnly -and $Path -notmatch '^[A-Za-z]:[\\/]')) {
        throw 'Control build output requires an absolute supported Windows path.'
    }
    if ($Path.IndexOfAny([char[]]'%!&|<>^"*?') -ge 0 -or $Path -match '[\x00-\x1f]' -or
        $Path.IndexOf(':', 2) -ge 0) {
        throw 'Control build output contains unsupported command or path characters.'
    }
    # Win32 can resolve a component ending in a space or dot to another spelling
    # even when GetFullPath preserves it. Reject aliases before containment
    # checks; standalone . and .. retain their normal canonicalization semantics.
    foreach ($component in $Path -split '[\\/]') {
        if ($component -ne '.' -and $component -ne '..' -and $component -match '[ .]$') {
            throw 'Control build paths cannot contain components ending in a space or dot.'
        }
    }
    [IO.Path]::GetFullPath($Path).TrimEnd([char[]]'\/')
}

function Assert-PlainControlBuildOutput([string]$Path) {
    for ($current = [IO.DirectoryInfo]::new($Path); $null -ne $current; $current = $current.Parent) {
        try { $attributes = [IO.File]::GetAttributes($current.FullName) }
        catch {
            if ($_.Exception.InnerException -is [IO.FileNotFoundException] -or
                $_.Exception.InnerException -is [IO.DirectoryNotFoundException]) { continue }
            throw
        }
        if (($attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0 -or
            ($attributes -band [IO.FileAttributes]::Directory) -eq 0) {
            throw 'Control build output and its existing ancestors must be ordinary directories.'
        }
    }
}

function Resolve-ControlBuildOutput {
    param(
        [Parameter(Mandatory = $true)][string]$OutDir,
        [Parameter(Mandatory = $true)][string]$ProjectRoot,
        [Parameter(Mandatory = $true)]
        [ValidateSet('enshrouded-control', 'valheim-control', 'dragonwilds-player-query')][string]$CargoLeaf,
        [switch]$LocalOnly
    )
    $output = ConvertTo-ControlBuildPath $OutDir $LocalOnly.IsPresent
    $repository = ConvertTo-ControlBuildPath $ProjectRoot
    $separator = [IO.Path]::DirectorySeparatorChar
    $root = [IO.Path]::GetPathRoot($output).TrimEnd([char[]]'\/')
    if ($output.Equals($root, [StringComparison]::OrdinalIgnoreCase) -or
        $output.Equals($repository, [StringComparison]::OrdinalIgnoreCase) -or
        $repository.StartsWith($output + $separator, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Control build output cannot be a filesystem root, repository root or repository ancestor.'
    }
    if ($output.StartsWith($repository + $separator, [StringComparison]::OrdinalIgnoreCase)) {
        # Standard Cargo builds use repository/target. Admit only the exact,
        # inherited OUT_DIR child used by build.rs, never a source-directory
        # OUT_DIR supplied by a caller. Standalone builders still use external
        # output directories, and no global target-directory setting is needed.
        if ([string]::IsNullOrWhiteSpace($env:OUT_DIR)) {
            throw 'Repository output requires the exact Cargo OUT_DIR child.'
        }
        $cargoOutput = ConvertTo-ControlBuildPath $env:OUT_DIR $LocalOnly.IsPresent
        $targetPrefix = [IO.Path]::Combine($repository, 'target') + $separator
        $expected = [IO.Path]::Combine($cargoOutput, $CargoLeaf)
        if (-not $cargoOutput.StartsWith($targetPrefix, [StringComparison]::OrdinalIgnoreCase) -or
            -not $output.Equals($expected, [StringComparison]::OrdinalIgnoreCase)) {
            throw 'Repository output requires the exact Cargo OUT_DIR child below target.'
        }
    }
    Assert-PlainControlBuildOutput $output
    $output
}
