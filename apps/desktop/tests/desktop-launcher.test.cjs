const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");

const root = path.resolve(__dirname, "..", "..", "..");

test("desktop frontend dev server uses the fixed Tauri dev URL port", () => {
  const desktopPackage = JSON.parse(fs.readFileSync(path.join(root, "apps", "desktop", "package.json"), "utf8"));
  const tauriConfig = JSON.parse(fs.readFileSync(path.join(root, "apps", "desktop", "src-tauri", "tauri.conf.json"), "utf8"));
  assert.equal(desktopPackage.scripts.dev, "vite --host 127.0.0.1 --port 43170 --strictPort");
  assert.equal(tauriConfig.build.devUrl, "http://127.0.0.1:43170");
});

test("desktop app runs without elevation", () => {
  const windowsManifest = fs.readFileSync(path.join(root, "apps", "desktop", "src-tauri", "windows-app-manifest.xml"), "utf8");
  assert.match(windowsManifest, /requestedExecutionLevel\s+level="asInvoker"/);
  assert.doesNotMatch(windowsManifest, /requestedExecutionLevel\s+level="requireAdministrator"/);
});
