const assert = require("node:assert/strict");
const { spawnSync } = require("node:child_process");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const test = require("node:test");

const root = path.resolve(__dirname, "..", "..", "..");
const checker = path.join(root, "scripts", "prepare_desktop_frontend.cjs");
const lockedVersions = {
  react: "19.3.0",
  vite: "8.3.4",
  "@vitejs/plugin-react": "6.1.2",
  "@typescript/typescript6": "6.0.2",
};

function fixture(t) {
  const project = fs.mkdtempSync(path.join(os.tmpdir(), "lgsm frontend dependencies "));
  t.after(() => fs.rmSync(project, { recursive: true, force: true }));
  const manifest = {
    name: "desktop-frontend-fixture",
    version: "0.0.0",
    dependencies: { react: "^19.3.0" },
    devDependencies: {
      vite: "^8.3.4",
      "@vitejs/plugin-react": "^6.1.2",
      "@typescript/typescript6": "6.0.2",
    },
  };
  const packages = { "": manifest };
  for (const [name, version] of Object.entries(lockedVersions)) {
    packages[`node_modules/${name}`] = { name, version };
  }
  fs.writeFileSync(path.join(project, "package.json"), JSON.stringify(manifest));
  fs.writeFileSync(path.join(project, "package-lock.json"), JSON.stringify({
    name: manifest.name, version: manifest.version, lockfileVersion: 3, packages,
  }));
  return project;
}

function install(project, name, version = lockedVersions[name]) {
  const packageRoot = path.join(project, "node_modules", name);
  fs.mkdirSync(packageRoot, { recursive: true });
  fs.writeFileSync(path.join(packageRoot, "package.json"), JSON.stringify({ name, version }));
}

function installAll(project, skip) {
  for (const name of Object.keys(lockedVersions)) {
    if (name !== skip) install(project, name);
  }
}

function check(project, ...args) {
  return spawnSync(process.execPath, [checker, project, ...args], {
    cwd: project, encoding: "utf8", timeout: 15000, windowsHide: true,
  });
}

function diagnostics(result) {
  return `${result.stdout}\n${result.stderr}`;
}

function snapshotFiles(project) {
  return fs.readdirSync(project, { recursive: true })
    .filter((entry) => fs.statSync(path.join(project, entry)).isFile())
    .sort()
    .map((entry) => [entry, fs.readFileSync(path.join(project, entry)).toString("base64")]);
}

test("an existing node_modules directory cannot hide the missing Vite React plugin", (t) => {
  const project = fixture(t);
  installAll(project, "@vitejs/plugin-react");
  const before = snapshotFiles(project);
  const result = check(project);
  assert.equal(result.status, 1, diagnostics(result));
  assert.match(result.stderr, /\[LanGame\]/);
  assert.match(result.stderr, /@vitejs\/plugin-react/);
  assert.deepEqual(snapshotFiles(project), before);
});

test("a completely absent dependency directory reports both runtime and development packages", (t) => {
  const project = fixture(t);
  const before = snapshotFiles(project);
  const result = check(project);
  assert.equal(result.status, 1, diagnostics(result));
  for (const name of Object.keys(lockedVersions)) {
    assert.ok(result.stderr.includes(name), result.stderr);
  }
  assert.equal(fs.existsSync(path.join(project, "node_modules")), false);
  assert.deepEqual(snapshotFiles(project), before);
});

test("every direct installed dependency must match its locked version", (t) => {
  const project = fixture(t);
  installAll(project);
  install(project, "@vitejs/plugin-react", "6.1.3");
  const before = snapshotFiles(project);
  const result = check(project);
  assert.equal(result.status, 1, diagnostics(result));
  assert.match(result.stderr, /@vitejs\/plugin-react/);
  assert.match(result.stderr, /6\.1\.2/);
  assert.match(result.stderr, /6\.1\.3/);
  assert.deepEqual(snapshotFiles(project), before);
});

test("a damaged installed package manifest fails with the affected package", (t) => {
  const project = fixture(t);
  installAll(project);
  fs.writeFileSync(path.join(project, "node_modules/@vitejs/plugin-react/package.json"), "{");
  const before = snapshotFiles(project);
  const result = check(project);
  assert.equal(result.status, 1, diagnostics(result));
  assert.match(result.stderr, /@vitejs\/plugin-react/);
  assert.deepEqual(snapshotFiles(project), before);
});

test("a complete locked installation passes without changing any fixture file", (t) => {
  const project = fixture(t);
  installAll(project);
  const before = snapshotFiles(project);
  const result = check(project);
  assert.equal(result.status, 0, diagnostics(result));
  assert.deepEqual(snapshotFiles(project), before);
});

function npmFixture(project, settings = {}) {
  const toolsRoot = path.join(project, "npm fixture");
  const bin = path.join(toolsRoot, "node_modules/npm/bin");
  fs.mkdirSync(bin, { recursive: true });
  fs.writeFileSync(path.join(toolsRoot, "npm.cmd"), "@echo off\r\nexit /b 99\r\n");
  fs.writeFileSync(path.join(project, "npm-fixture.json"), JSON.stringify(settings));
  fs.writeFileSync(path.join(bin, "npm-prefix.js"), String.raw`
    const fs = require("node:fs");
    const path = require("node:path");
    fs.writeFileSync("npm-prefix-called", "called");
    console.log(path.resolve(__dirname, "../../.."));
  `);
  fs.writeFileSync(path.join(bin, "npm-cli.js"), String.raw`
    const fs = require("node:fs");
    const path = require("node:path");
    const args = process.argv.slice(2);
    fs.appendFileSync("npm-calls.jsonl", JSON.stringify(args) + "\n");
    const settings = JSON.parse(fs.readFileSync("npm-fixture.json", "utf8"));
    if (settings.failCommand === args[0]) {
      console.error("synthetic npm failure");
      process.exit(19);
    }
    if (settings.changeCommand === args[0]) {
      fs.appendFileSync(settings.changeSource, "\n");
    }
    if (args[0] === "install") {
      const hiddenLock = "node_modules/.package-lock.json";
      if (settings.checkIndex && fs.statSync(hiddenLock).mtimeMs > 0) {
        console.error("stale hidden installation index was still trusted");
        process.exit(20);
      }
      const lock = JSON.parse(fs.readFileSync("package-lock.json", "utf8"));
      for (const [location, pkg] of Object.entries(lock.packages)) {
        if (!location || location === settings.skip) continue;
        fs.mkdirSync(location, { recursive: true });
        fs.writeFileSync(path.join(location, "package.json"), JSON.stringify(pkg));
      }
    }
  `);
  return path.join(toolsRoot, "npm.cmd");
}

function npmCalls(project) {
  return fs.readFileSync(path.join(project, "npm-calls.jsonl"), "utf8")
    .trim().split("\n").map(JSON.parse);
}

function sourceBytes(project) {
  return ["package.json", "package-lock.json"].map((name) => fs.readFileSync(path.join(project, name)));
}

test("a complete installation never invokes npm or resolves its prefix", (t) => {
  const project = fixture(t);
  installAll(project);
  const npm = npmFixture(project, { failCommand: "ci" });
  const before = snapshotFiles(project);
  const result = check(project, "--install", npm);
  assert.equal(result.status, 0, diagnostics(result));
  assert.equal(fs.existsSync(path.join(project, "npm-prefix-called")), false);
  assert.equal(fs.existsSync(path.join(project, "npm-calls.jsonl")), false);
  assert.deepEqual(snapshotFiles(project), before);
});

test("missing dependencies are repaired after lock validation without deleting the Vite cache", (t) => {
  const project = fixture(t);
  installAll(project, "@vitejs/plugin-react");
  const npm = npmFixture(project, { checkIndex: true });
  fs.mkdirSync(path.join(project, "node_modules/.vite"), { recursive: true });
  const hiddenLock = path.join(project, "node_modules/.package-lock.json");
  const cacheSentinel = path.join(project, "node_modules/.vite/preserved.txt");
  fs.writeFileSync(hiddenLock, '{"fixture":"keep hidden index contents"}');
  fs.writeFileSync(cacheSentinel, "keep the current Vite cache");
  const hiddenLockBefore = fs.readFileSync(hiddenLock);
  const cacheBefore = fs.readFileSync(cacheSentinel);
  const sourcesBefore = sourceBytes(project);
  const result = check(project, "--install", npm);
  assert.equal(result.status, 0, diagnostics(result));
  assert.match(result.stdout, /\[LanGame\]/);
  assert.deepEqual(sourceBytes(project), sourcesBefore);
  assert.deepEqual(fs.readFileSync(hiddenLock), hiddenLockBefore);
  assert.deepEqual(fs.readFileSync(cacheSentinel), cacheBefore);
  assert.equal(fs.statSync(hiddenLock).mtimeMs, 0);
  const calls = npmCalls(project);
  assert.equal(calls.length, 2);
  assert.equal(calls[0][0], "ci");
  assert.ok(calls[0].includes("--dry-run"));
  assert.equal(calls[1][0], "install");
  for (const args of calls) {
    for (const option of [
      "--include=dev", "--include=optional", "--ignore-scripts",
      "--package-lock=true", "--package-lock-only=false", "--no-audit", "--no-fund",
    ]) {
      assert.ok(args.includes(option), `Missing ${option} from ${JSON.stringify(args)}`);
    }
  }
  assert.ok(calls[1].includes("--no-save"));
  assert.ok(calls[1].includes("--dry-run=false"));
  assert.equal(check(project).status, 0);
  assert.equal(check(project, "--install", npm).status, 0);
  assert.deepEqual(npmCalls(project), calls);
});

test("repair can create a completely missing node_modules installation", (t) => {
  const project = fixture(t);
  const npm = npmFixture(project);
  const sourcesBefore = sourceBytes(project);
  assert.equal(fs.existsSync(path.join(project, "node_modules")), false);
  const result = check(project, "--install", npm);
  assert.equal(result.status, 0, diagnostics(result));
  assert.deepEqual(sourceBytes(project), sourcesBefore);
  for (const [name, version] of Object.entries(lockedVersions)) {
    const installed = JSON.parse(fs.readFileSync(path.join(project, "node_modules", name, "package.json"), "utf8"));
    assert.equal(installed.version, version);
  }
  assert.equal(npmCalls(project).length, 2);
});

for (const failCommand of ["ci", "install"]) {
  test(`npm ${failCommand} failure stops preparation and preserves its exit code`, (t) => {
    const project = fixture(t);
    const npm = npmFixture(project, { failCommand });
    const sourcesBefore = sourceBytes(project);
    const result = check(project, "--install", npm);
    assert.equal(result.status, 19, diagnostics(result));
    assert.match(result.stderr, /synthetic npm failure/);
    assert.match(result.stderr, /exit code 19/);
    assert.match(result.stderr, /\[LanGame\]/);
    assert.equal(npmCalls(project).length, failCommand === "ci" ? 1 : 2);
    assert.deepEqual(sourceBytes(project), sourcesBefore);
  });
}

test("a nominally successful install must still provide the missing React plugin", (t) => {
  const project = fixture(t);
  const npm = npmFixture(project, { skip: "node_modules/@vitejs/plugin-react" });
  const sourcesBefore = sourceBytes(project);
  const result = check(project, "--install", npm);
  assert.equal(result.status, 1, diagnostics(result));
  assert.match(result.stderr, /@vitejs\/plugin-react/);
  assert.equal(npmCalls(project).length, 2);
  assert.deepEqual(sourceBytes(project), sourcesBefore);
});

for (const changeSource of ["package.json", "package-lock.json"]) {
  for (const changeCommand of ["ci", "install"]) {
    test(`${changeSource} changing during npm ${changeCommand} stops preparation and preserves the synchronized source`, (t) => {
      const project = fixture(t);
      const npm = npmFixture(project, { changeSource, changeCommand });
      const changedPath = path.join(project, changeSource);
      const before = fs.readFileSync(changedPath, "utf8");
      const unchangedName = changeSource === "package.json" ? "package-lock.json" : "package.json";
      const unchangedBefore = fs.readFileSync(path.join(project, unchangedName));
      const result = check(project, "--install", npm);
      assert.equal(result.status, 1, diagnostics(result));
      assert.ok(result.stderr.includes(`${changeSource} changed`), result.stderr);
      assert.equal(fs.readFileSync(changedPath, "utf8"), before + "\n");
      assert.deepEqual(fs.readFileSync(path.join(project, unchangedName)), unchangedBefore);
      assert.equal(npmCalls(project).length, changeCommand === "ci" ? 1 : 2);
    });
  }
}

for (const sourceName of ["package.json", "package-lock.json"]) {
  test(`a damaged ${sourceName} fails before npm can modify the installation`, (t) => {
    const project = fixture(t);
    const npm = npmFixture(project);
    fs.writeFileSync(path.join(project, sourceName), "{");
    const before = snapshotFiles(project);
    const result = check(project, "--install", npm);
    assert.equal(result.status, 1, diagnostics(result));
    assert.match(result.stderr, /\[LanGame\]/);
    assert.equal(fs.existsSync(path.join(project, "npm-calls.jsonl")), false);
    assert.deepEqual(snapshotFiles(project), before);
  });
}
