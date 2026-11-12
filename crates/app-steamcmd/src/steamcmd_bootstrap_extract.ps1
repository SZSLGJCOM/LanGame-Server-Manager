$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression

# ZipFile.OpenRead/ExtractToFile pass paths through legacy .NET normalization.
# Open native handles instead; the ZIP parser and all entry checks stay the same.
Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.ComponentModel;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
public static class LanGameBootstrapFile {
  [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
  private static extern SafeFileHandle CreateFile(string path, uint access, uint sharing, IntPtr security, uint disposition, uint flags, IntPtr template);

  public static FileStream Open(string path, bool write) {
    if (!path.StartsWith(@"\\?\", StringComparison.Ordinal)) {
      path = path.StartsWith(@"\\", StringComparison.Ordinal)
        ? @"\\?\UNC\" + path.Substring(2) : @"\\?\" + path;
    }
    // Both files already exist: the verified archive and Rust-owned staging.
    SafeFileHandle handle = CreateFile(path, write ? 0x40000000u : 0x80000000u,
      write ? 0u : 1u, IntPtr.Zero, 3, 0x80, IntPtr.Zero);
    if (handle.IsInvalid) {
      int error = Marshal.GetLastWin32Error();
      handle.Dispose();
      throw new Win32Exception(error);
    }
    try {
      return new FileStream(handle, write ? FileAccess.Write : FileAccess.Read);
    } catch {
      handle.Dispose();
      throw;
    }
  }
}
'@

function Expand-SteamCmdBootstrapper([string]$archivePath, [string]$stagingPath) {
  $archiveStream = [LanGameBootstrapFile]::Open($archivePath, $false)
  try {
    $zip = [IO.Compression.ZipArchive]::new($archiveStream, [IO.Compression.ZipArchiveMode]::Read, $true)
    try {
      $entry = $zip.GetEntry('steamcmd.exe')
      if ($null -eq $entry -or $entry.Length -lt 1 -or $entry.Length -gt 64MB) {
        throw 'Invalid SteamCMD bootstrapper ZIP'
      }
      $payloadStream = $entry.Open()
      try {
        $stageStream = [LanGameBootstrapFile]::Open($stagingPath, $true)
        try {
          $stageStream.SetLength(0)
          $payloadStream.CopyTo($stageStream)
          if ($stageStream.Length -ne $entry.Length) { throw 'Invalid SteamCMD bootstrapper size' }
          $stageStream.Flush($true)
        } finally { $stageStream.Dispose() }
      } finally { $payloadStream.Dispose() }
    } finally { $zip.Dispose() }
  } finally { $archiveStream.Dispose() }
}
