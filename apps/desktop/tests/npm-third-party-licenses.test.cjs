const assert = require("node:assert/strict");
const { spawnSync } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");
const { pathToFileURL } = require("node:url");
const test = require("node:test");

const desktopRoot = path.resolve(__dirname, "..");
const lockPath = path.join(desktopRoot, "package-lock.json");
const outputPath = path.join(desktopRoot, "public", "THIRD_PARTY_LICENSES.txt");
const scriptPath = path.join(
  desktopRoot,
  "scripts",
  "generate_npm_third_party_licenses.mjs"
);

function packageNameFromLockPath(lockPackagePath) {
  return lockPackagePath.slice(lockPackagePath.lastIndexOf("node_modules/") + 13);
}

function normalizeLicenseText(text) {
  return text.replace(/\r\n?/g, "\n").replace(/\n+$/u, "");
}

test("generated npm license inventory matches every locked production package", async () => {
  const lock = JSON.parse(fs.readFileSync(lockPath, "utf8"));
  const expectedPackageIds = Object.entries(lock.packages)
    .filter(([lockPackagePath, metadata]) =>
      lockPackagePath.startsWith("node_modules/") && metadata.dev !== true
    )
    .map(
      ([lockPackagePath, metadata]) =>
        `${packageNameFromLockPath(lockPackagePath)}@${metadata.version}`
    )
    .sort((left, right) => left.localeCompare(right, "en"));

  const generator = await import(pathToFileURL(scriptPath).href);
  const { document, packages } = await generator.generateLicenseDocument();
  const actualPackageIds = packages.map((dependency) => dependency.packageId);

  assert.deepEqual(actualPackageIds, expectedPackageIds);
  assert.equal(document, fs.readFileSync(outputPath, "utf8"));
  assert.equal(
    document.match(/^Package \/ 软件包:/gmu)?.length,
    expectedPackageIds.length
  );
});

test("hls.js attribution and derived-work terms are preserved verbatim", () => {
  const output = fs.readFileSync(outputPath, "utf8");
  const upstreamLicense = normalizeLicenseText(
    fs.readFileSync(path.join(desktopRoot, "node_modules", "hls.js", "LICENSE"), "utf8")
  );

  assert.match(output, /^Package \/ 软件包: hls\.js@1\.7\.3$/mu);
  assert.ok(output.includes(upstreamLicense));
  assert.match(output, /Copyright \(c\) 2017 Dailymotion/);
  assert.match(output, /Copyright \(c\) 2013-2015 Brightcove/);
  assert.match(output, /derived from the HLS library for video\.js/);
});

test("checked-in license output passes deterministic drift checking", () => {
  const result = spawnSync(process.execPath, [scriptPath, "--check"], {
    cwd: desktopRoot,
    encoding: "utf8"
  });

  assert.equal(result.status, 0, result.stderr || result.stdout);
  assert.match(result.stdout, /is current \(\d+ production packages\)/);
});

test("Vite frontendDist distribution keeps the public license artifact", () => {
  const viteConfig = fs.readFileSync(path.join(desktopRoot, "vite.config.ts"), "utf8");
  const tauriConfig = JSON.parse(
    fs.readFileSync(path.join(desktopRoot, "src-tauri", "tauri.conf.json"), "utf8")
  );

  assert.equal(path.relative(path.join(desktopRoot, "public"), outputPath), "THIRD_PARTY_LICENSES.txt");
  assert.match(viteConfig, /outDir:\s*["']dist["']/);
  assert.doesNotMatch(viteConfig, /publicDir\s*:/);
  assert.equal(tauriConfig.build.frontendDist, "../dist");
});
