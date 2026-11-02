const assert = require("node:assert/strict");
const { spawnSync } = require("node:child_process");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const test = require("node:test");

const launcherPath = path.resolve(__dirname, "../../..", "scripts/start_langame_desktop_local.ps1");
const launcherTest = process.platform === "win32" && fs.existsSync(launcherPath) ? test : test.skip;

function runRetentionFixture(body) {
  const fixture = fs.mkdtempSync(path.join(os.tmpdir(), "lgsm-launcher-retention-"));
  const script = String.raw`
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$tokens = $null; $errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile('${launcherPath.replaceAll("'", "''")}', [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw ($errors | Out-String) }
foreach ($name in @('Assert-NormalPathChain', 'Assert-LauncherRuntimeRoot', 'Remove-LauncherLog', 'Invoke-LauncherLogRetention')) {
  $definition = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
  if ($null -eq $definition) {
    if ($name -eq 'Remove-LauncherLog') { continue }
    throw "Missing production function $name."
  }
  Invoke-Expression $definition.Extent.Text
}
$runtimeRoot = $PSScriptRoot
$runtimeMarkerPath = Join-Path $runtimeRoot '.fixture-owner'
$runtimeMarkerContent = 'launcher-retention-test|1'
$logRetentionBytes = [int64]100
$logRetentionCount = 10
$logRetentionAge = [TimeSpan]::FromDays(7)
$activeLogBudgetBytes = [int64]0
$activeLogFileCount = 0
$logFinalizationReserveBytes = [int64]0
function New-FixtureLog([int]$Index, [int]$Bytes, [int]$AgeMinutes) {
  $file = Join-Path $runtimeRoot ('desktop-20261101-000000-{0:x8}.out.log' -f $Index)
  [IO.File]::WriteAllBytes($file, [byte[]]::new($Bytes))
  [IO.File]::SetLastWriteTimeUtc($file, [DateTime]::UtcNow.AddMinutes(-$AgeMinutes))
  return $file
}
${body}
Write-Output 'PASS: isolated launcher log retention'
`;
  fs.writeFileSync(path.join(fixture, "retention.ps1"), script);
  try {
    const result = spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass",
      "-File", path.join(fixture, "retention.ps1")], { encoding: "utf8", timeout: 15000, windowsHide: true });
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    assert.match(result.stdout, /PASS: isolated launcher log retention/);
  } finally {
    assert.equal(path.dirname(path.resolve(fixture)), path.resolve(os.tmpdir()));
    assert.ok(path.basename(fixture).startsWith("lgsm-launcher-retention-"));
    assert.equal(fs.lstatSync(fixture).isSymbolicLink(), false);
    fs.rmSync(fixture, { recursive: true, force: false, maxRetries: 3, retryDelay: 100 });
  }
}

launcherTest("expired launcher logs held without delete sharing do not block retention", () => runRetentionFixture(String.raw`
$locked = New-FixtureLog 1 4 12000
$expired = New-FixtureLog 2 4 13000
$recent = New-FixtureLog 3 4 1
$handle = [IO.File]::Open($locked, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
try {
  Invoke-LauncherLogRetention
  if (-not [IO.File]::Exists($locked) -or -not [IO.File]::Exists($recent) -or [IO.File]::Exists($expired)) {
    throw 'Age retention did not preserve the held log while deleting the expired unlocked log.'
  }
} finally { $handle.Dispose() }
Invoke-LauncherLogRetention
if ([IO.File]::Exists($locked)) { throw 'An expired log was not removed after its handle was released.' }
`));

launcherTest("a held oversized launcher log still consumes the remaining byte and count budget", () => runRetentionFixture(String.raw`
$logRetentionBytes = 8
$logRetentionCount = 1
$locked = New-FixtureLog 1 12 1
$older = New-FixtureLog 2 2 2
$oldest = New-FixtureLog 3 1 3
$handle = [IO.File]::Open($locked, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
try {
  Invoke-LauncherLogRetention
  if (-not [IO.File]::Exists($locked) -or [IO.File]::Exists($older) -or [IO.File]::Exists($oldest)) {
    throw 'The held oversized log was omitted from the retention budget.'
  }
} finally { $handle.Dispose() }
`));

launcherTest("count retention skips a held old launcher log and continues deleting older unlocked logs", () => runRetentionFixture(String.raw`
$logRetentionCount = 1
$newest = New-FixtureLog 1 1 1
$locked = New-FixtureLog 2 1 2
$oldest = New-FixtureLog 3 1 3
$handle = [IO.File]::Open($locked, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
try {
  Invoke-LauncherLogRetention
  if (-not [IO.File]::Exists($newest) -or -not [IO.File]::Exists($locked) -or [IO.File]::Exists($oldest)) {
    throw 'Count retention stopped at the held log or removed the newest retained log.'
  }
} finally { $handle.Dispose() }
`));

launcherTest("non-sharing launcher log deletion errors remain visible", () => runRetentionFixture(String.raw`
$readonly = New-FixtureLog 1 1 12000
[IO.File]::SetAttributes($readonly, [IO.FileAttributes]::ReadOnly)
$failure = $null
try {
  try { Invoke-LauncherLogRetention } catch { $failure = $_.Exception.GetBaseException() }
  if ($null -eq $failure -or $failure.HResult -ne -2147024891) {
    throw 'A non-sharing deletion failure was swallowed or changed.'
  }
  if (-not [IO.File]::Exists($readonly)) { throw 'The protected log disappeared.' }
} finally { [IO.File]::SetAttributes($readonly, [IO.FileAttributes]::Normal) }
`));
