const assert = require("node:assert/strict");
const { spawnSync } = require("node:child_process");
const crypto = require("node:crypto");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const test = require("node:test");

const root = path.resolve(__dirname, "..", "..", "..");
const tauriConfigPath = path.join(root, "apps", "desktop", "src-tauri", "tauri.conf.json");
const cargoTomlPath = path.join(root, "apps", "desktop", "src-tauri", "Cargo.toml");
const mainPath = path.join(root, "apps", "desktop", "src-tauri", "src", "main.rs");
const appUpdatesPath = path.join(root, "apps", "desktop", "src-tauri", "src", "app_updates.rs");
const runtimeLifecyclePath = path.join(root, "apps", "desktop", "src-tauri", "src", "commands_runtime_lifecycle.rs");
const updaterBuildScriptPath = path.join(root, "scripts", "build_desktop_update_artifacts.ps1");
const artifactName = "LanGame Server Manager_0.1.0_x64-setup.exe";
const feedUrl = "https://github.com/SZSLGJCOM/LanGame-Server-Manager/releases/latest/download/latest.json";

// Encoding fixtures only: these bytes are deliberately not real signing credentials
// or valid cryptographic signatures. Actual signature verification belongs to Tauri.
function encodedRecord(lines) {
  return Buffer.from(lines.join("\n") + "\n").toString("base64");
}
function fixtureSignature(keyId = 7) {
  return encodedRecord([
    "untrusted comment: synthetic test signature",
    Buffer.concat([Buffer.from("ED"), Buffer.alloc(8, keyId), Buffer.alloc(64, 3)]).toString("base64"),
    "trusted comment: synthetic fixture",
    Buffer.alloc(64, 4).toString("base64"),
  ]);
}
const fixturePublicKey = encodedRecord([
  "untrusted comment: synthetic test public key",
  Buffer.concat([Buffer.from("Ed"), Buffer.alloc(8, 7), Buffer.alloc(32, 2)]).toString("base64"),
]);

function createFixture(t) {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "langame-updater-"));
  t.after(() => fs.rmSync(tempRoot, { recursive: true, force: true }));
  const repositoryRoot = path.join(tempRoot, "repository");
  const scriptRoot = path.join(repositoryRoot, "scripts");
  const desktopRoot = path.join(repositoryRoot, "apps", "desktop");
  const tauriRoot = path.join(desktopRoot, "src-tauri");
  const artifactRoot = path.join(tempRoot, "artifacts");
  const outputRoot = path.join(tempRoot, "output");
  for (const directory of [scriptRoot, tauriRoot, artifactRoot]) fs.mkdirSync(directory, { recursive: true });
  for (const name of ["build_desktop_update_artifacts.ps1", "generate_desktop_update_manifest.py"]) {
    fs.copyFileSync(path.join(root, "scripts", name), path.join(scriptRoot, name));
  }
  fs.writeFileSync(path.join(repositoryRoot, "Cargo.toml"), '[workspace.package]\nversion = "0.1.0"\n');
  fs.writeFileSync(path.join(tauriRoot, "Cargo.toml"), '[package]\nversion.workspace = true\n');
  fs.writeFileSync(path.join(desktopRoot, "package.json"), JSON.stringify({ version: "0.1.0" }));
  const configPath = path.join(tauriRoot, "tauri.conf.json");
  fs.writeFileSync(configPath, JSON.stringify({
    productName: "LanGame Server Manager", version: "0.1.0",
    bundle: { windows: { webviewInstallMode: { type: "offlineInstaller", silent: true } } },
    plugins: { updater: { pubkey: fixturePublicKey, endpoints: [] } },
  }));
  fs.writeFileSync(path.join(artifactRoot, artifactName), "synthetic installer fixture");
  fs.writeFileSync(path.join(artifactRoot, artifactName + ".sig"), "\ufeff" + fixtureSignature() + "\r\n");
  return { repositoryRoot, scriptRoot, desktopRoot, configPath, artifactRoot, outputRoot, tempRoot };
}

function runPreparation(fixture, args = [], outputRoot = fixture.outputRoot) {
  return spawnSync("powershell", [
    "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass",
    "-File", path.join(fixture.scriptRoot, "build_desktop_update_artifacts.ps1"),
    ...(outputRoot === null ? [] : ["-OutputRoot", outputRoot]), ...args,
  ], {
    cwd: fixture.repositoryRoot, encoding: "utf8", timeout: 30000,
    env: { ...process.env, TAURI_SIGNING_PRIVATE_KEY: "" },
  });
}
function readJson(file) { return JSON.parse(fs.readFileSync(file, "utf8").replace(/^\uFEFF/u, "")); }
function expectSuccess(result) { assert.equal(result.status, 0, result.stderr || result.stdout); }
function expectFailure(result, message) {
  assert.notEqual(result.status, 0, result.stderr || result.stdout);
  if (message) assert.match(`${result.stderr}\n${result.stdout}`, message);
}

test("desktop updater artifacts and plugin are configured", () => {
  const config = JSON.parse(fs.readFileSync(tauriConfigPath, "utf8"));
  const cargoToml = fs.readFileSync(cargoTomlPath, "utf8");
  const mainSource = fs.readFileSync(mainPath, "utf8");

  assert.equal(config.bundle.createUpdaterArtifacts, false, "ordinary local installers require no release signing key");
  assert.equal(config.plugins.updater.windows.installMode, "passive");
  assert.deepEqual(config.plugins.updater.endpoints, [], "pre-release builds must not contact an update feed");
  assert.match(config.plugins.updater.pubkey, /BEGIN PUBLIC KEY|^[A-Za-z0-9+/=]{32,}$/);
  assert.match(cargoToml, /tauri-plugin-updater/);
  assert.match(mainSource, /tauri_plugin_updater::Builder::new\(\)\.build\(\)/);
});

test("desktop update commands are registered through the app update module", () => {
  const mainSource = fs.readFileSync(mainPath, "utf8");
  assert.match(mainSource, /mod app_updates;/);
  assert.match(mainSource, /app_updates::check_app_update/);
  assert.match(mainSource, /app_updates::install_app_update/);
});

test("desktop updates download before stopping the runtime and installing", () => {
  const appUpdatesSource = fs.readFileSync(appUpdatesPath, "utf8");
  const lifecycleSource = fs.readFileSync(runtimeLifecyclePath, "utf8");
  assert.match(appUpdatesSource, /request_app_restart_shutdown\(app\)/);
  assert.doesNotMatch(appUpdatesSource, /\bapp\.restart\(\)/);
  assert.match(lifecycleSource, /AppShutdownCompletion::Restart => app_handle\.restart\(\)/);
  const download = appUpdatesSource.indexOf(".download(");
  const stop = appUpdatesSource.indexOf("runtime_service::stop_service(&app)");
  const install = appUpdatesSource.indexOf(".install(bytes)");
  assert.ok(download >= 0 && stop > download && install > stop,
    "a failed download must not stop servers, and installation must wait for coordinated shutdown");
  assert.match(appUpdatesSource, /InstallationAfterShutdown/u);
});

test("release preparation is offline by default and requires no signing key", (t) => {
  const fixture = createFixture(t);
  expectSuccess(runPreparation(fixture));
  const plan = readJson(path.join(fixture.outputRoot, "desktop-release-plan.json"));
  const config = readJson(path.join(fixture.outputRoot, "tauri.release.conf.json"));
  assert.equal(plan.mode, "Prepare");
  assert.equal(plan.published, false);
  assert.equal(plan.requested_updates_enabled, false);
  assert.equal(plan.requested_webview_install_mode, "offlineInstaller");
  assert.equal(plan.updates_enabled, undefined);
  assert.equal(plan.artifact_build_configuration, "not-inspected");
  assert.equal(plan.version, "0.1.0");
  assert.deepEqual(config.plugins.updater.endpoints, []);
  assert.equal(config.bundle.createUpdaterArtifacts, true);
  assert.deepEqual(config.bundle.windows.webviewInstallMode, { type: "offlineInstaller", silent: true });
  assert.match(config.build.beforeBuildCommand, /VITE_LANGAME_DESKTOP_UPDATES_ENABLED=false/);
  assert.deepEqual(fs.readdirSync(fixture.outputRoot).sort(), ["desktop-release-plan.json", "tauri.release.conf.json"]);
});

test("release preparation rejects a WebView2 mode that violates offline silent installation", async (t) => {
  for (const mode of ["downloadBootstrapper", "embedBootstrapper", "skip", "interactive-offline"]) {
    await t.test(mode, (subtest) => {
      const fixture = createFixture(subtest);
      const config = readJson(fixture.configPath);
      config.bundle.windows.webviewInstallMode = mode === "interactive-offline"
        ? { type: "offlineInstaller", silent: false } : { type: mode, silent: true };
      fs.writeFileSync(fixture.configPath, JSON.stringify(config));
      expectFailure(runPreparation(fixture), /offlineInstaller.*silent/);
      assert.equal(fs.existsSync(fixture.outputRoot), false);
    });
  }
});

test("GitHub update configuration requires an explicit opt-in and leaves source configuration untouched", (t) => {
  const fixture = createFixture(t);
  const sourceBefore = fs.readFileSync(fixture.configPath, "utf8");
  expectSuccess(runPreparation(fixture, ["-DryRun", "-EnableGitHubUpdates"]));
  const config = readJson(path.join(fixture.outputRoot, "tauri.release.conf.json"));
  assert.deepEqual(config.plugins.updater.endpoints, [feedUrl]);
  assert.match(config.build.beforeBuildCommand, /VITE_LANGAME_DESKTOP_UPDATES_ENABLED=true/);
  assert.equal(fs.readFileSync(fixture.configPath, "utf8"), sourceBefore);
  assert.equal(readJson(path.join(fixture.outputRoot, "desktop-release-plan.json")).published, false);
});

test("release preparation rejects version disagreement before writing output", (t) => {
  const fixture = createFixture(t);
  fs.writeFileSync(path.join(fixture.desktopRoot, "package.json"), '{"version":"0.2.0"}');
  expectFailure(runPreparation(fixture), /must share one stable/);
  assert.equal(fs.existsSync(fixture.outputRoot), false);
});

test("release preparation requires an explicit output outside the source repository", async (t) => {
  for (const mode of ["missing", "repository", "child", "relative", "extended", "device"]) {
    await t.test(mode, (subtest) => {
      const fixture = createFixture(subtest);
      const outputs = {
        missing: null, repository: fixture.repositoryRoot,
        child: path.join(fixture.repositoryRoot, "output"), relative: "output",
        extended: "\\\\?\\" + fixture.outputRoot, device: "\\\\.\\" + fixture.outputRoot,
      };
      expectFailure(runPreparation(fixture, [], outputs[mode]));
      assert.equal(fs.existsSync(fixture.outputRoot), false);
      assert.equal(fs.existsSync(path.join(fixture.repositoryRoot, "output")), false);
    });
  }
});

test("release export copies exactly the current-version installer, signature, manifest and checksum inventory", (t) => {
  const fixture = createFixture(t);
  fs.writeFileSync(path.join(fixture.artifactRoot, "LanGame Server Manager_0.0.1_x64-setup.exe"), "old installer");
  const notes = path.join(fixture.tempRoot, "notes.txt");
  fs.writeFileSync(notes, "本地发行验收说明\n");
  expectSuccess(runPreparation(fixture, ["-ArtifactRoot", fixture.artifactRoot, "-NotesFile", notes]));
  const manifest = readJson(path.join(fixture.outputRoot, "latest.json"));
  assert.equal(manifest.version, "0.1.0");
  assert.equal(manifest.notes, "本地发行验收说明\n");
  assert.equal(manifest.platforms["windows-x86_64"].signature, fixtureSignature());
  assert.equal(manifest.platforms["windows-x86_64"].url,
    "https://github.com/SZSLGJCOM/LanGame-Server-Manager/releases/download/v0.1.0/" + encodeURIComponent(artifactName));
  assert.equal(fs.readFileSync(path.join(fixture.outputRoot, artifactName), "utf8"), "synthetic installer fixture");
  const summary = readJson(path.join(fixture.outputRoot, "desktop-update-artifacts.json"));
  assert.equal(summary.published, false);
  assert.equal(summary.signature_validation, "format-and-key-id-only");
  assert.equal(summary.artifact_build_configuration, "not-inspected");
  assert.equal(summary.artifact_count, 3);
  assert.deepEqual(summary.artifacts.map((item) => item.name).sort(), [artifactName, artifactName + ".sig", "latest.json"]);
  const checksums = fs.readFileSync(path.join(fixture.outputRoot, "SHA256SUMS"), "utf8");
  for (const artifact of summary.artifacts) {
    const bytes = fs.readFileSync(path.join(fixture.outputRoot, artifact.name));
    assert.equal(artifact.bytes, bytes.length);
    assert.equal(artifact.sha256, crypto.createHash("sha256").update(bytes).digest("hex"));
    assert.ok(checksums.includes(`${artifact.sha256}  ${artifact.name}\n`));
  }
  assert.equal(fs.existsSync(path.join(fixture.outputRoot, "LanGame Server Manager_0.0.1_x64-setup.exe")), false);
  expectFailure(runPreparation(fixture, ["-ArtifactRoot", fixture.artifactRoot]), /never overwrites/);
});

test("release export rejects missing, malformed and wrong-key signatures", async (t) => {
  for (const mode of ["missing", "empty", "malformed", "wrong-key"]) {
    await t.test(mode, (subtest) => {
      const fixture = createFixture(subtest);
      const signature = path.join(fixture.artifactRoot, artifactName + ".sig");
      if (mode === "missing") fs.unlinkSync(signature);
      else fs.writeFileSync(signature, mode === "empty" ? "" : mode === "wrong-key" ? fixtureSignature(8) : "invalid-signature");
      expectFailure(runPreparation(fixture, ["-ArtifactRoot", fixture.artifactRoot]));
      assert.equal(fs.existsSync(path.join(fixture.outputRoot, "latest.json")), false);
      assert.equal(fs.existsSync(path.join(fixture.outputRoot, artifactName)), false);
    });
  }
});

test("managed workstations cannot accidentally invoke portable Cargo builds", (t) => {
  const fixture = createFixture(t);
  const managedScripts = path.join(fixture.tempRoot, "scripts");
  fs.mkdirSync(managedScripts);
  fs.writeFileSync(path.join(managedScripts, "invoke-codex-cargo.ps1"), "throw 'The marker must never be executed.'");
  expectFailure(runPreparation(fixture, ["-BuildPortable"]), /managed OPC host/);
  assert.equal(fs.existsSync(fixture.outputRoot), false);
});

test("managed installer builds require the workstation entry and do not silently use portable Cargo", (t) => {
  const fixture = createFixture(t);
  expectFailure(runPreparation(fixture, ["-BuildManaged"]), /managed Cargo entry/);
  assert.equal(fs.existsSync(fixture.outputRoot), false);
});

test("managed local installer builds reach the snapshot bundler with updates disabled and no key", (t) => {
  const fixture = createFixture(t);
  const managedScripts = path.join(fixture.tempRoot, "scripts");
  fs.mkdirSync(managedScripts);
  fs.writeFileSync(path.join(managedScripts, "invoke-codex-cargo.ps1"), `
param($Project, $CargoCommand, [switch]$SourceSnapshot, $DesktopBundle, $BundleConfigPath,
  $SnapshotReceiptPath, $SnapshotArtifacts, $WaitTimeoutSeconds,
  [switch]$DesktopUpdatesEnabled, [Parameter(ValueFromRemainingArguments=$true)]$CargoArguments)
@{project=$Project; command=$CargoCommand; snapshot=[bool]$SourceSnapshot; bundle=$DesktopBundle;
  config=$BundleConfigPath; artifacts=$SnapshotArtifacts; updates=[bool]$DesktopUpdatesEnabled;
  arguments=$CargoArguments} | ConvertTo-Json | Set-Content -Encoding UTF8 -LiteralPath
  (Join-Path (Split-Path -Parent $SnapshotReceiptPath) 'managed-invocation.json')
exit 42
`.replace("-LiteralPath\n  ", "-LiteralPath "));
  expectFailure(runPreparation(fixture, ["-BuildManaged"]), /Managed desktop build failed/);
  const invocation = readJson(path.join(fixture.outputRoot, "managed-invocation.json"));
  const config = readJson(path.join(fixture.outputRoot, "tauri.release.conf.json"));
  assert.equal(invocation.project, "LanGameServerManager");
  assert.equal(invocation.command, "build");
  assert.equal(invocation.snapshot, true);
  assert.equal(invocation.bundle, "LanGameServerManager");
  assert.equal(invocation.updates, false);
  assert.ok(invocation.artifacts.includes(`release/bundle/nsis/${artifactName}`));
  assert.ok(!invocation.artifacts.includes(".sig"));
  assert.equal(config.bundle.createUpdaterArtifacts, false);
  assert.deepEqual(config.bundle.windows.webviewInstallMode, { type: "offlineInstaller", silent: true });
  assert.deepEqual(config.plugins.updater.endpoints, []);
  assert.equal(fs.existsSync(path.join(fixture.outputRoot, artifactName)), false);
  assert.equal(fs.existsSync(path.join(fixture.outputRoot, "latest.json")), false);
});

test("managed GitHub builds fail before writing output when no update signing key is provided", (t) => {
  const fixture = createFixture(t);
  const managedScripts = path.join(fixture.tempRoot, "scripts");
  fs.mkdirSync(managedScripts);
  fs.writeFileSync(path.join(managedScripts, "invoke-codex-cargo.ps1"), "throw 'Must not build without a key.'");
  expectFailure(runPreparation(fixture, ["-BuildManaged", "-EnableGitHubUpdates"]), /TAURI_SIGNING_PRIVATE_KEY/);
  assert.equal(fs.existsSync(fixture.outputRoot), false);
});

for (const changedConfig of [false, true]) {
  test(`managed installer export ${changedConfig ? "rejects a changed" : "accepts the matching"} build configuration`, (t) => {
    const fixture = createFixture(t);
    const managedScripts = path.join(fixture.tempRoot, "scripts");
    fs.mkdirSync(managedScripts);
    // Replace only the external workstation builder/receipt-reader boundary.
    // This verifies the product export decision, not real compilation or signing.
    fs.writeFileSync(path.join(managedScripts, "codex-snapshot-build.psm1"), `
function Read-CodexSnapshotBuildReceipt { param($Path) Get-Content -Raw -LiteralPath $Path | ConvertFrom-Json }
Export-ModuleMember -Function Read-CodexSnapshotBuildReceipt
`);
    fs.writeFileSync(path.join(managedScripts, "invoke-codex-cargo.ps1"), `
param($Project, $CargoCommand, [switch]$SourceSnapshot, $DesktopBundle, $BundleConfigPath,
  $SnapshotReceiptPath, $SnapshotArtifacts, $WaitTimeoutSeconds,
  [switch]$DesktopUpdatesEnabled, [Parameter(ValueFromRemainingArguments=$true)]$CargoArguments)
$repo = Join-Path (Split-Path -Parent $PSScriptRoot) 'repository'
Import-Module (Join-Path $PSHOME 'Modules/Microsoft.PowerShell.Utility/Microsoft.PowerShell.Utility.psd1')
$payload = Join-Path (Split-Path -Parent $PSScriptRoot) 'artifacts/${artifactName}'
$configHash = (Get-FileHash -LiteralPath $BundleConfigPath -Algorithm SHA256).Hash.ToLowerInvariant()
${changedConfig ? "$configHash = '0' * 64" : ""}
@{
  receipt=@{project=$Project;sourceIdentitySha256=('a' * 64);artifacts=@(@{
    path=$payload;targetRelativePath='release/bundle/nsis/${artifactName}';
    sha256=(Get-FileHash -LiteralPath $payload -Algorithm SHA256).Hash.ToLowerInvariant()})}
  source=@{source=@{files=@(@{path='apps/desktop/src-tauri/tauri.conf.json';
    sha256=(Get-FileHash -LiteralPath (Join-Path $repo 'apps/desktop/src-tauri/tauri.conf.json') -Algorithm SHA256).Hash.ToLowerInvariant()})}}
  desktopBundle=@{kind='LanGameServerManager';inputConfigurationSha256=$configHash;updatesEnabled=$false;signed=$false}
} | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $SnapshotReceiptPath -Encoding UTF8
exit 0
`);
    const result = runPreparation(fixture, ["-BuildManaged"]);
    const output = path.join(fixture.outputRoot, artifactName);
    if (changedConfig) {
      expectFailure(result, /receipt configuration does not match/);
      assert.equal(fs.existsSync(output), false);
      assert.equal(fs.existsSync(path.join(fixture.outputRoot, "desktop-installer-artifacts.json")), false);
    } else {
      expectSuccess(result);
      assert.equal(fs.readFileSync(output, "utf8"), "synthetic installer fixture");
      const summary = readJson(path.join(fixture.outputRoot, "desktop-installer-artifacts.json"));
      assert.equal(summary.published, false);
      assert.equal(summary.updates_enabled, false);
      assert.equal(summary.updater_signature, "not-requested");
      assert.equal(fs.existsSync(path.join(fixture.outputRoot, "latest.json")), false);
      const hash = crypto.createHash("sha256").update(fs.readFileSync(output)).digest("hex");
      assert.equal(fs.readFileSync(path.join(fixture.outputRoot, "SHA256SUMS"), "utf8"), `${hash}  ${artifactName}\n`);
    }
  });
}
