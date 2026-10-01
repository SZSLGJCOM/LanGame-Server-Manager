param(
    [Parameter(Mandatory = $true)]
    [string[]]$ProcessName,

    [int]$DurationSeconds = 300,
    [int]$IntervalSeconds = 2,

    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$OutputPath
)

$ErrorActionPreference = "Stop"

if ($DurationSeconds -le 0) {
    throw "DurationSeconds must be greater than 0."
}

if ($IntervalSeconds -le 0) {
    throw "IntervalSeconds must be greater than 0."
}

$resolvedOutput = [System.IO.Path]::GetFullPath($OutputPath)
$outputDirectory = [System.IO.Path]::GetDirectoryName($resolvedOutput)
if ($outputDirectory -and -not [System.IO.Directory]::Exists($outputDirectory)) {
    [System.IO.Directory]::CreateDirectory($outputDirectory) | Out-Null
}

$startedAt = [DateTimeOffset]::Now
$deadline = $startedAt.AddSeconds($DurationSeconds)

while ([DateTimeOffset]::Now -lt $deadline) {
    $sampledAt = [DateTimeOffset]::Now
    foreach ($name in $ProcessName) {
        Get-Process -Name $name -ErrorAction SilentlyContinue | ForEach-Object {
            $sample = [ordered]@{
                sampled_at_unix_ms = $sampledAt.ToUnixTimeMilliseconds()
                process_name = $_.ProcessName
                pid = $_.Id
                cpu_seconds = $_.CPU
                working_set_bytes = $_.WorkingSet64
                private_memory_bytes = $_.PrivateMemorySize64
                handle_count = $_.HandleCount
                thread_count = $_.Threads.Count
                start_time = try { $_.StartTime.ToString("o") } catch { $null }
            }
            $sample | ConvertTo-Json -Compress | Add-Content -LiteralPath $resolvedOutput
        }
    }

    Start-Sleep -Seconds $IntervalSeconds
}

Write-Output "runtime baseline written to $resolvedOutput"
