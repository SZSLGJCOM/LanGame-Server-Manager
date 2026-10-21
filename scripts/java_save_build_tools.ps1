# Small build-only JDK acquisition shared by first-party Java save extensions.
# The caller resolves and owns OutputDirectory before calling this helper.
function ConvertTo-JavaSaveBuildIoPath {
    param([string]$Path)
    if ($Path.StartsWith('\\?\', [StringComparison]::Ordinal)) { return $Path }
    $fullPath = [IO.Path]::GetFullPath($Path)
    if ($fullPath.StartsWith('\\', [StringComparison]::Ordinal)) { return '\\?\UNC\' + $fullPath.Substring(2) }
    return '\\?\' + $fullPath
}

function Test-JavaSaveBuildArchive {
    param([string]$Path, $Metadata)
    $ioPath = ConvertTo-JavaSaveBuildIoPath $Path
    if (!(Test-Path -LiteralPath $ioPath -PathType Leaf)) { return $false }
    if (([IO.File]::GetAttributes($ioPath) -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw 'Pinned build JDK archive cannot be a redirected file.'
    }
    (Get-Item -LiteralPath $ioPath).Length -eq $Metadata.size -and
        (Get-FileHash -LiteralPath $ioPath -Algorithm SHA256).Hash.ToLowerInvariant() -eq $Metadata.sha256
}

function Resolve-JavaSaveBuildArchive {
    param([string]$OutputDirectory, $Metadata)
    $archivePath = Join-Path $OutputDirectory 'official-jdk.zip'
    $ioArchivePath = ConvertTo-JavaSaveBuildIoPath $archivePath
    if (Test-JavaSaveBuildArchive $archivePath $Metadata) { return $archivePath }
    if (Test-Path -LiteralPath $ioArchivePath) {
        # An interrupted earlier build may leave a truncated download. Preserve
        # that exact cache artifact; it must not permanently prevent recovery.
        $rejected = Join-Path $OutputDirectory ('official-jdk.rejected-' + [Guid]::NewGuid().ToString('N') + '.zip')
        [IO.File]::Move($ioArchivePath, (ConvertTo-JavaSaveBuildIoPath $rejected))
    }
    $partial = Join-Path $OutputDirectory ('official-jdk.' + [Guid]::NewGuid().ToString('N') + '.partial')
    $ioPartial = ConvertTo-JavaSaveBuildIoPath $partial
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    Add-Type -AssemblyName System.Net.Http
    $client = [Net.Http.HttpClient]::new()
    $cancellation = [Threading.CancellationTokenSource]::new()
    $cancellation.CancelAfter(180000)
    $deadline = [Threading.Tasks.Task]::Delay(180000, $cancellation.Token)
    $response = $inputStream = $fileStream = $null
    try {
        # Own the file before receiving bytes. A partial response is never
        # published under the cache name consumed by subsequent builds.
        $fileStream = [IO.FileStream]::new($ioPartial, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::Read)
        $response = $client.GetAsync($Metadata.url, [Net.Http.HttpCompletionOption]::ResponseHeadersRead, $cancellation.Token).GetAwaiter().GetResult()
        $response.EnsureSuccessStatusCode() | Out-Null
        $length = $response.Content.Headers.ContentLength
        if ($null -ne $length -and $length -ne $Metadata.size) { throw 'Pinned official JDK download length changed.' }
        $inputStream = $response.Content.ReadAsStreamAsync().GetAwaiter().GetResult()
        $buffer = [byte[]]::new(65536)
        [long]$received = 0
        while ($true) {
            $cancellation.Token.ThrowIfCancellationRequested()
            $read = $inputStream.ReadAsync($buffer, 0, $buffer.Length, $cancellation.Token)
            # Windows PowerShell's .NET Framework response stream may ignore
            # cancellation during a body read. Race that read against the same
            # total deadline on this thread, then dispose the owned response.
            $finished = [Threading.Tasks.Task]::WhenAny([Threading.Tasks.Task[]]@($read, $deadline)).GetAwaiter().GetResult()
            if ([object]::ReferenceEquals($finished, $deadline)) { throw 'Pinned official JDK download timed out.' }
            $count = $read.GetAwaiter().GetResult()
            $cancellation.Token.ThrowIfCancellationRequested()
            if ($count -eq 0) { break }
            $received += $count
            if ($received -gt $Metadata.size) { throw 'Pinned official JDK download exceeds its bound.' }
            $fileStream.Write($buffer, 0, $count)
        }
        $fileStream.Flush($true)
    } finally {
        $cancellation.Cancel()
        if ($null -ne $fileStream) { $fileStream.Dispose() }
        if ($null -ne $inputStream) { $inputStream.Dispose() }
        if ($null -ne $response) { $response.Dispose() }
        $cancellation.Dispose()
        $client.Dispose()
    }
    if (!(Test-JavaSaveBuildArchive $partial $Metadata)) { throw 'Pinned official JDK archive identity changed.' }
    # Move refuses an unexpected existing destination; never overwrite one.
    [IO.File]::Move($ioPartial, $ioArchivePath)
    $archivePath
}

function Resolve-JavaSaveBuildJdk {
    param([string]$OutputDirectory, [string]$MetadataPath, [string]$JdkRoot)
    $metadata = Get-Content -LiteralPath $MetadataPath -Raw | ConvertFrom-Json
    if (!$JdkRoot) {
        $JdkRoot = Join-Path $OutputDirectory $metadata.root
        if (!(Test-Path -LiteralPath $JdkRoot -PathType Container)) {
            $archivePath = Resolve-JavaSaveBuildArchive $OutputDirectory $metadata
            Add-Type -AssemblyName System.IO.Compression.FileSystem
            $archive = [IO.Compression.ZipFile]::OpenRead($archivePath)
            try {
                [long]$expanded = 0
                foreach ($entry in $archive.Entries) {
                    $name = $entry.FullName.Replace('\','/')
                    $expanded += $entry.Length
                    if ($expanded -gt 2GB -or !$name.StartsWith($metadata.root+'/',[StringComparison]::Ordinal) -or
                        $name.Contains(':') -or ($name -split '/') -contains '..') { throw 'Invalid pinned JDK archive path or size.' }
                    $destination = [IO.Path]::GetFullPath((Join-Path $OutputDirectory $name))
                    if (!$destination.StartsWith($JdkRoot.TrimEnd('\')+'\',[StringComparison]::OrdinalIgnoreCase)) {
                        throw 'Pinned JDK entry escapes its owned directory.'
                    }
                }
                # Keep the existing name/size validation pass before writing files.
                # Normalize each name before adding the Windows I/O prefix; the
                # Framework bulk extractor retains ZIP slashes after that prefix.
                foreach ($entry in $archive.Entries) {
                    $destination = [IO.Path]::GetFullPath((Join-Path $OutputDirectory $entry.FullName.Replace('\','/')))
                    $ioDestination = ConvertTo-JavaSaveBuildIoPath $destination
                    if ($entry.Name.Length -eq 0) {
                        if ($entry.Length -ne 0) { throw 'Pinned JDK directory entry contains file data.' }
                        [IO.Directory]::CreateDirectory($ioDestination) | Out-Null
                    } else {
                        [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($ioDestination)) | Out-Null
                        [IO.Compression.ZipFileExtensions]::ExtractToFile($entry,$ioDestination)
                    }
                }
            } finally { $archive.Dispose() }
        }
    }
    $JdkRoot = (Resolve-Path -LiteralPath $JdkRoot).Path
    Assert-PlainControlBuildOutput $JdkRoot
    foreach ($entry in $metadata.executables.PSObject.Properties) {
        $file = Join-Path $JdkRoot $entry.Name
        $ioFile = ConvertTo-JavaSaveBuildIoPath $file
        if (([IO.File]::GetAttributes($ioFile) -band [IO.FileAttributes]::ReparsePoint) -ne 0 -or
            (Get-FileHash -LiteralPath $ioFile -Algorithm SHA256).Hash.ToLowerInvariant() -ne $entry.Value) {
            throw ('Pinned build JDK image changed: '+$entry.Name)
        }
    }
    $JdkRoot
}

function Resolve-JavaSaveCompileReference {
    param([string]$OutputDirectory, [string]$MetadataPath)
    $metadata = Get-Content -LiteralPath $MetadataPath -Raw | ConvertFrom-Json
    $file = Join-Path $OutputDirectory $metadata.entry
    if (!(Test-Path -LiteralPath $file -PathType Leaf)) {
        [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
        Invoke-WebRequest -UseBasicParsing -Uri $metadata.source -OutFile $file
    }
    if (([IO.File]::GetAttributes($file) -band [IO.FileAttributes]::ReparsePoint) -ne 0 -or
        (Get-Item -LiteralPath $file).Length -ne $metadata.bytes -or
        (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant() -ne $metadata.sha256) {
        throw 'Pinned official Java compile reference identity changed.'
    }
    $file
}
