const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");

const root = path.resolve(__dirname, "..", "..", "..");
const modulesRoot = path.join(root, "modules");
const tauriConfig = JSON.parse(
  fs.readFileSync(path.join(root, "apps", "desktop", "src-tauri", "tauri.conf.json"), "utf8"),
);
const storageSource = fs.readFileSync(path.join(root, "crates", "app-storage", "src", "lib.rs"), "utf8");

const expectedModuleIds = [
  "abioticfactor",
  "arksurvivalascended",
  "arksurvivalevolved",
  "astroneer",
  "barotrauma",
  "conanexiles",
  "corekeeper",
  "dontstarve",
  "enshrouded",
  "humanitz",
  "minecraft",
  "necesse",
  "nightingale",
  "palworld",
  "projectzomboid",
  "returntomoria",
  "rimworld",
  "romestead",
  "runescapedragonwilds",
  "rust",
  "satisfactory",
  "scum",
  "sevendaystodie",
  "sonsoftheforest",
  "soulmask",
  "squad",
  "terraria",
  "theforest",
  "unturned",
  "valheim",
  "vrising",
  "windrose",
];

function listFilesRecursively(directory) {
  if (!fs.existsSync(directory)) {
    return [];
  }

  return fs.readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const entryPath = path.join(directory, entry.name);
    return entry.isDirectory() ? listFilesRecursively(entryPath) : [entryPath];
  });
}

test("NSIS bundle maps the complete internal game catalog to the runtime modules resource", () => {
  assert.equal(tauriConfig.bundle.resources?.["../../../modules/"], "modules/");
});

test("local installers need no release key and keep the update feed dormant", () => {
  assert.equal(tauriConfig.bundle.createUpdaterArtifacts, false);
  assert.deepEqual(tauriConfig.plugins.updater.endpoints, []);
});

test("installer identity, supported languages and maintenance policy are explicit", () => {
  const windows = tauriConfig.bundle.windows;
  assert.equal(tauriConfig.identifier, "cn.langame.servermanager");
  assert.equal(tauriConfig.bundle.publisher, "LanGame Team");
  assert.equal(windows.allowDowngrades, false);
  assert.equal(windows.nsis.installMode, "currentUser");
  assert.deepEqual(windows.nsis.languages, ["English", "SimpChinese"]);
  assert.equal(windows.nsis.displayLanguageSelector, true);
  const tauriRoot = path.join(root, "apps", "desktop", "src-tauri");
  for (const file of [tauriConfig.bundle.licenseFile, windows.nsis.installerIcon,
    windows.nsis.uninstallerIcon, windows.nsis.installerHooks,
    ...Object.values(windows.nsis.customLanguageFiles)]) {
    assert.ok(fs.statSync(path.resolve(tauriRoot, file)).isFile(), `Missing installer input: ${file}`);
  }
});

test("ordinary installers embed the official WebView2 bootstrapper for both installer languages", () => {
  assert.deepEqual(tauriConfig.bundle.windows.webviewInstallMode, {
    type: "embedBootstrapper", silent: true,
  }, "missing runtimes require Microsoft connectivity or the separate offline installer");
});

test("installer branding uses complete, correctly sized 24-bit NSIS bitmap inputs", () => {
  const nsis = tauriConfig.bundle.windows.nsis;
  const tauriRoot = path.join(root, "apps", "desktop", "src-tauri");
  assert.equal(nsis.uninstallerHeaderImage, nsis.headerImage, "install and uninstall must share the same brand header");
  for (const [field, width, height] of [["headerImage", 150, 57], ["sidebarImage", 164, 314]]) {
    const bitmap = fs.readFileSync(path.resolve(tauriRoot, nsis[field]));
    assert.equal(bitmap.toString("ascii", 0, 2), "BM");
    assert.equal(bitmap.readUInt32LE(2), bitmap.length);
    assert.equal(bitmap.readUInt32LE(10), 54, "bitmap pixels follow a BITMAPINFOHEADER");
    assert.equal(bitmap.readUInt32LE(14), 40);
    assert.equal(bitmap.readInt32LE(18), width);
    assert.equal(bitmap.readInt32LE(22), height);
    assert.equal(bitmap.readUInt16LE(26), 1);
    assert.equal(bitmap.readUInt16LE(28), 24, "NSIS inputs use an opaque 24-bit RGB format");
    assert.equal(bitmap.readUInt32LE(30), 0, "NSIS branding bitmaps must be uncompressed");
    assert.equal(bitmap.length, 54 + Math.ceil(width * 3 / 4) * 4 * height);
    const source = fs.readFileSync(path.resolve(tauriRoot, nsis[field].replace(/\.bmp$/u, ".svg")), "utf8");
    assert.match(source, /apps\/desktop\/src\/assets\/langame-logo(?:-dark)?\.svg/);
    assert.match(source, /aria-label="LANGAME"/);
    assert.match(source, /aria-label="SERVER MANAGER"/);
  }
});

test("installer languages expose the same messages and accurately describe retained data", () => {
  const tauriRoot = path.join(root, "apps", "desktop", "src-tauri");
  const catalogs = Object.values(tauriConfig.bundle.windows.nsis.customLanguageFiles)
    .map((file) => fs.readFileSync(path.resolve(tauriRoot, file), "utf8"));
  const keys = (source) => [...source.matchAll(/^LangString\s+(\w+)/gm)].map((match) => match[1]).sort();
  assert.deepEqual(keys(catalogs[0]), keys(catalogs[1]));
  for (const source of catalogs) {
    assert.ok(keys(source).includes("lgsmCloseBeforeMaintenance"));
    assert.ok(keys(source).includes("lgsmProcessCheckFailed"));
    assert.match(source, /deleteAppData[^\n]*(keep servers and settings|保留服务器与设置)/);
  }
});

test("the bundled game catalog keeps all 32 module definitions and template sources", () => {
  const actualModuleIds = fs
    .readdirSync(modulesRoot, { withFileTypes: true })
    .filter((entry) => entry.isDirectory() && fs.existsSync(path.join(modulesRoot, entry.name, "module.toml")))
    .map((entry) => entry.name)
    .sort();

  assert.deepEqual(actualModuleIds, expectedModuleIds);

  for (const moduleId of expectedModuleIds) {
    const moduleRoot = path.join(modulesRoot, moduleId);
    for (const requiredFile of ["module.toml", "schema.json", "config-sources.toml"]) {
      const requiredPath = path.join(moduleRoot, requiredFile);
      assert.ok(fs.statSync(requiredPath).isFile(), `${moduleId}/${requiredFile} must be bundled`);
      assert.ok(fs.statSync(requiredPath).size > 0, `${moduleId}/${requiredFile} must not be empty`);
    }
  }

  const templateFiles = listFilesRecursively(modulesRoot).filter((filePath) =>
    filePath.endsWith(".hbs"),
  );
  assert.ok(templateFiles.length >= 105, "the catalog must retain its complete template source set");
  for (const templatePath of templateFiles) {
    assert.ok(fs.statSync(templatePath).size > 0, `${path.relative(root, templatePath)} must not be empty`);
  }
});

test("desktop storage uses per-user local data and does not treat cwd ./modules as an install contract", () => {
  assert.match(storageSource, /env::var(?:_os)?\("LOCALAPPDATA"\)/);
  assert.doesNotMatch(storageSource, /PathBuf::from\("C:\/ProgramData"\)/);
  assert.doesNotMatch(storageSource, /let\s+workspace_root\s*=\s*PathBuf::from\("\."\)/);
  assert.doesNotMatch(storageSource, /modules_root:\s*workspace_root\.join\("modules"\)/);
  assert.doesNotMatch(storageSource, /legacy_database_path/);
  assert.doesNotMatch(storageSource, /legacy_settings_path/);
});
