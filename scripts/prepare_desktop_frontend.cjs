const fs = require("node:fs");
const path = require("node:path");
const { spawnSync } = require("node:child_process");

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, "utf8").replace(/^\uFEFF/, ""));
}

function findIncompleteDependencies(root) {
  const manifest = readJson(path.join(root, "package.json"));
  const lock = readJson(path.join(root, "package-lock.json"));
  const dependencies = { ...manifest.dependencies, ...manifest.devDependencies };
  const failures = [];
  for (const name of Object.keys(dependencies)) {
    if (!/^(?:@[a-z0-9][a-z0-9._-]*\/)?[a-z0-9][a-z0-9._-]*$/i.test(name)) {
      throw new Error(`Invalid desktop dependency name: ${name}`);
    }
    const locked = lock.packages?.[`node_modules/${name}`];
    if (!locked || typeof locked.version !== "string" || !locked.version) {
      throw new Error(`Desktop lockfile has no locked version for ${name}.`);
    }
    let installed;
    try {
      installed = readJson(path.join(root, "node_modules", name, "package.json"));
    } catch (error) {
      if (error.code !== "ENOENT" && !(error instanceof SyntaxError)) throw error;
      failures.push(`${name}: missing or invalid package.json`);
      continue;
    }
    if (installed?.version !== locked.version) {
      failures.push(`${name}: expected ${locked.version}, found ${installed?.version ?? "no version"}`);
    }
  }
  return failures;
}

function resolveNpmCli(npmCommand, root) {
  const bin = path.join(path.dirname(npmCommand), "node_modules/npm/bin");
  let cli = path.join(bin, "npm-cli.js");
  const prefixScript = path.join(bin, "npm-prefix.js");
  // Follow npm.cmd's routing when the host has a separately installed npm pin.
  if (fs.existsSync(prefixScript)) {
    const prefix = spawnSync(process.execPath, [prefixScript], {
      cwd: root, encoding: "utf8", windowsHide: true, timeout: 30000,
    });
    if (prefix.error) throw prefix.error;
    if (prefix.status !== 0 || !prefix.stdout.trim()) {
      throw new Error(`Installed npm prefix resolution failed: ${prefix.stderr.trim()}`);
    }
    const configuredCli = path.join(prefix.stdout.trim(), "node_modules/npm/bin/npm-cli.js");
    if (fs.existsSync(configuredCli)) cli = configuredCli;
  }
  if (!fs.statSync(cli).isFile()) throw new Error(`Installed npm CLI is unavailable: ${cli}`);
  return cli;
}

function installDependencies(root, npmCommand) {
  const sources = ["package.json", "package-lock.json"].map((name) => ({
    name, contents: fs.readFileSync(path.join(root, name)),
  }));
  const assertSourcesUnchanged = () => {
    for (const source of sources) {
      if (!source.contents.equals(fs.readFileSync(path.join(root, source.name)))) {
        throw new Error(`${source.name} changed during dependency preparation. Retry after source synchronization finishes.`);
      }
    }
  };
  const cli = resolveNpmCli(npmCommand, root);
  const options = [
    "--include=dev", "--include=optional", "--ignore-scripts", "--package-lock=true",
    "--package-lock-only=false", "--no-audit", "--no-fund",
    "--fetch-retries=2", "--fetch-timeout=60000",
  ];
  // A real npm ci removes .vite as well as dependencies. Validate the lock with
  // ci, then repair in place without saving either synchronized source file.
  for (const args of [
    ["ci", "--dry-run", ...options],
    ["install", "--no-save", "--dry-run=false", ...options],
  ]) {
    assertSourcesUnchanged();
    if (args[0] === "install") {
      // Do not trust a newer hidden index left behind by an interrupted install.
      const hiddenLock = path.join(root, "node_modules/.package-lock.json");
      const stat = fs.statSync(hiddenLock, { throwIfNoEntry: false });
      if (stat) fs.utimesSync(hiddenLock, stat.atime, new Date(0));
    }
    const result = spawnSync(process.execPath, [cli, ...args], {
      cwd: root, stdio: args[0] === "ci" ? ["inherit", "ignore", "inherit"] : "inherit",
      windowsHide: true, timeout: 15 * 60 * 1000,
    });
    if (result.error) throw result.error;
    if (result.status !== 0) {
      const error = new Error(`npm ${args[0]} failed with exit code ${result.status ?? result.signal}. Desktop startup stopped.`);
      error.exitCode = result.status ?? 1;
      throw error;
    }
    assertSourcesUnchanged();
  }
}

function main() {
  const args = process.argv.slice(2);
  if (args.length !== 1 && !(args.length === 3 && args[1] === "--install")) {
    throw new Error("Usage: node prepare_desktop_frontend.cjs <project-root> [--install <npm.cmd>]");
  }
  const root = path.resolve(args[0]);
  let failures = findIncompleteDependencies(root);
  if (failures.length === 0) return;
  if (args[1] === "--install") {
    process.stdout.write(`[LanGame] Preparing incomplete local frontend dependencies:\n${failures.join("\n")}\n`);
    installDependencies(root, args[2]);
    failures = findIncompleteDependencies(root);
    if (failures.length === 0) {
      process.stdout.write("[LanGame] Frontend dependencies are ready. Continuing startup.\n");
      return;
    }
    throw new Error(`Dependencies remain incomplete after npm install:\n${failures.join("\n")}`);
  }
  throw new Error(`Frontend dependencies are incomplete on this computer:\n${failures.join("\n")}\nRun start.bat to restore dependencies from package-lock.json.`);
}

try {
  main();
} catch (error) {
  process.stderr.write(`[LanGame] ${error instanceof Error ? error.message : String(error)}\n`);
  process.exitCode = Number.isInteger(error.exitCode) && error.exitCode > 0 ? error.exitCode : 1;
}
