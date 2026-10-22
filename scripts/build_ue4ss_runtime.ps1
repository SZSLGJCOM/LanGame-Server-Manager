param([Parameter(Mandatory = $true)][string]$OutDir)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
foreach ($moduleName in @('Microsoft.PowerShell.Management', 'Microsoft.PowerShell.Utility')) {
    Import-Module -Name ([IO.Path]::Combine($PSHOME, 'Modules', $moduleName, $moduleName + '.psd1')) -ErrorAction Stop
}
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
. (Join-Path $PSScriptRoot 'build_control_output.ps1')
$output = Resolve-ControlBuildOutput -OutDir $OutDir -ProjectRoot $projectRoot -CargoLeaf 'ue4ss-runtime'
[IO.Directory]::CreateDirectory($output) | Out-Null
$destination = Join-Path $output 'UE4SS.zip'
$expectedBytes = 8718500L
$expectedHash = '4f9762f812329a640c8cfa14444c2bb97ecc213b8320bd4c5433383c3eef48f7'
$url = 'https://github.com/UE4SS-RE/RE-UE4SS/releases/download/experimental/UE4SS_v3.0.1-1125-g527a483b.zip'

function Assert-VerifiedArchive([string]$Path) {
    $item = Get-Item -LiteralPath $Path -ErrorAction Stop
    if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0 -or
        $item.Length -ne $expectedBytes -or
        (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash -ine $expectedHash) {
        throw 'UE4SS archive does not match the pinned official release; existing files were preserved.'
    }
}

if (Test-Path -LiteralPath $destination) {
    Assert-VerifiedArchive $destination
    Write-Output $destination
    return
}
$source = $env:LANGAME_UE4SS_ARCHIVE
if (-not [string]::IsNullOrWhiteSpace($source)) {
    if (-not [IO.Path]::IsPathRooted($source)) { throw 'LANGAME_UE4SS_ARCHIVE requires an absolute archive path.' }
    Assert-VerifiedArchive $source
}
$partial = Join-Path $output ('UE4SS.' + [Guid]::NewGuid().ToString('N') + '.partial')
try {
    if (-not [string]::IsNullOrWhiteSpace($source)) {
        [IO.File]::Copy($source, $partial, $false)
    } else {
        Add-Type -AssemblyName System.Net.Http
        $client = [Net.Http.HttpClient]::new()
        $cancel = [Threading.CancellationTokenSource]::new([TimeSpan]::FromSeconds(180))
        $response = $null; $inputStream = $null; $outputStream = $null
        try {
            $response = $client.GetAsync($url, [Net.Http.HttpCompletionOption]::ResponseHeadersRead, $cancel.Token).GetAwaiter().GetResult()
            $response.EnsureSuccessStatusCode() | Out-Null
            if ($null -ne $response.Content.Headers.ContentLength -and $response.Content.Headers.ContentLength -ne $expectedBytes) {
                throw 'The fixed UE4SS archive download length changed.'
            }
            $inputStream = $response.Content.ReadAsStreamAsync().GetAwaiter().GetResult()
            $outputStream = [IO.File]::Open($partial, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write)
            $buffer = [byte[]]::new(65536)
            $total = 0L
            while (($count = $inputStream.ReadAsync($buffer, 0, $buffer.Length, $cancel.Token).GetAwaiter().GetResult()) -gt 0) {
                $total += $count
                if ($total -gt $expectedBytes) { throw 'UE4SS archive exceeded its fixed download size.' }
                $outputStream.Write($buffer, 0, $count)
            }
        } finally {
            if ($outputStream) { $outputStream.Dispose() }
            if ($inputStream) { $inputStream.Dispose() }
            if ($response) { $response.Dispose() }
            $cancel.Dispose(); $client.Dispose()
        }
    }
    Assert-VerifiedArchive $partial
    # Publish without replacing a cache another build may have completed.
    [IO.File]::Move($partial, $destination)
    Assert-VerifiedArchive $destination
} finally {
    if (Test-Path -LiteralPath $partial -PathType Leaf) { Remove-Item -LiteralPath $partial }
}
Write-Output $destination
