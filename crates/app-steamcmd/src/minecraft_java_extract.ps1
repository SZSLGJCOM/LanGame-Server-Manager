Add-Type -AssemblyName System.IO.Compression

function Expand-MinecraftJavaArchive([string]$archivePath, [string]$stagingPath) {
  $stage = [LanGameInstallFiles]::FullPath($stagingPath).TrimEnd('\')
  if ([LanGameInstallFiles]::Attributes($stage) -ne -1) {
    throw 'Java staging directory already exists.'
  }
  [LanGameInstallFiles]::CreateDirectory($stage)
  $prefix = $stage + '\'
  $archiveStream = [LanGameInstallFiles]::OpenRead($archivePath)
  try {
    $zip = [IO.Compression.ZipArchive]::new($archiveStream, [IO.Compression.ZipArchiveMode]::Read, $true)
    try {
      if ($zip.Entries.Count -gt 50000) { throw 'Java archive has too many entries.' }
      $seen = New-Object 'System.Collections.Generic.HashSet[string]' ([StringComparer]::OrdinalIgnoreCase)
      [long]$total = 0
      # Validate the complete entry list before writing any package files.
      foreach ($entry in $zip.Entries) {
        $name = $entry.FullName.Replace('/', '\')
        $parts = $name.TrimEnd('\').Split('\')
        if ($name.StartsWith('\') -or $parts.Count -eq 0) { throw 'Absolute Java archive entry.' }
        foreach ($part in $parts) {
          if ($part -eq '' -or $part -eq '.' -or $part -eq '..' -or
              $part.EndsWith('.') -or $part.EndsWith(' ') -or
              $part.IndexOfAny([IO.Path]::GetInvalidFileNameChars()) -ge 0 -or
              $part -match '^(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(\.|$)') {
            throw 'Unsafe Java archive path.'
          }
        }
        $target = [LanGameInstallFiles]::Combine($stage, $name)
        if (-not $target.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase) -or
            -not $seen.Add($target.TrimEnd('\'))) {
          throw 'Duplicate or escaping Java archive entry.'
        }
        $unixType = ($entry.ExternalAttributes -shr 16) -band 61440
        if ($unixType -eq 40960 -or ($entry.ExternalAttributes -band 1024) -ne 0) {
          throw 'Java archive links are not supported.'
        }
        $total += $entry.Length
        if ($entry.Length -lt 0 -or $total -gt 2147483648) {
          throw 'Java archive exceeds the 2 GiB expanded limit.'
        }
        if ($name.EndsWith('\') -and $entry.Length -ne 0) {
          throw 'Java archive directory contains file data.'
        }
      }
      $buffer = New-Object byte[] 65536
      foreach ($entry in $zip.Entries) {
        $name = $entry.FullName.Replace('/', '\')
        $target = [LanGameInstallFiles]::Combine($stage, $name)
        if ($name.EndsWith('\')) {
          [LanGameInstallFiles]::CreateDirectory($target)
          continue
        }
        [LanGameInstallFiles]::CreateDirectory([LanGameInstallFiles]::Parent($target))
        $inputStream = $entry.Open()
        try {
          $outputStream = [LanGameInstallFiles]::CreateNew($target)
          try {
            [long]$copied = 0
            while (($count = $inputStream.Read($buffer, 0, $buffer.Length)) -gt 0) {
              $copied += $count
              if ($copied -gt $entry.Length) { throw 'Java archive entry exceeds its declared length.' }
              $outputStream.Write($buffer, 0, $count)
            }
            if ($copied -ne $entry.Length) { throw 'Java archive entry is incomplete.' }
            $outputStream.Flush($true)
          } finally { $outputStream.Dispose() }
        } finally { $inputStream.Dispose() }
      }
    } finally { $zip.Dispose() }
  } finally { $archiveStream.Dispose() }
}
