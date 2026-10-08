// Runs a feature-gated desktop host against isolated data and a real WebView2.
const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const os = require("node:os");
const path = require("node:path");
const { spawn, execFile, execFileSync } = require("node:child_process");
const { promisify } = require("node:util");
const { randomUUID, createHash } = require("node:crypto");

const execFileAsync = promisify(execFile);
const desktopRoot = path.resolve(__dirname, "..");
const outputLimit = 128 * 1024;
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function deadline(promise, ms, label) {
  let timer;
  try {
    return await Promise.race([promise, new Promise((_, reject) => {
      timer = setTimeout(() => reject(new Error(`${label} exceeded ${ms} ms`)), ms);
    })]);
  } finally { clearTimeout(timer); }
}

async function processSnapshot() {
  const { stdout } = await execFileAsync("powershell.exe", [
    "-NoProfile", "-NonInteractive", "-Command",
    "@(Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,@{Name='Created';Expression={$_.CreationDate.ToUniversalTime().Ticks.ToString()}}) | ConvertTo-Json -Compress",
  ], { windowsHide: true, timeout: 10000, maxBuffer: 4 * 1024 * 1024 });
  const rows = JSON.parse(stdout);
  assert.ok(Array.isArray(rows) && rows.length > 0 && rows.length < 20000);
  return rows;
}

function recordTree(rows, rootPid, observed) {
  const current = new Map(rows.map((row) => [row.ProcessId, row]));
  const included = new Set();
  if (!observed.has(rootPid) || current.get(rootPid)?.Created === observed.get(rootPid)) included.add(rootPid);
  // Creation times prevent a reused PID from being mistaken for an owned process.
  // Keep exited parents as roots: a still-running child can outlive its parent
  // and first appear after the host has exited, during the cleanup snapshots.
  for (const [pid, created] of observed) {
    if (!current.has(pid) || current.get(pid).Created === created) included.add(pid);
  }
  for (let pass = 0; pass < rows.length; pass++) {
    let added = false;
    for (const row of rows) {
      const parentCreated = current.get(row.ParentProcessId)?.Created ?? observed.get(row.ParentProcessId);
      if (included.has(row.ParentProcessId) && !included.has(row.ProcessId)
          && row.Created && parentCreated && BigInt(row.Created) >= BigInt(parentCreated)) {
        included.add(row.ProcessId);
        added = true;
      }
    }
    if (!added) break;
  }
  for (const row of rows) {
    if (included.has(row.ProcessId)) {
      assert.match(row.Created, /^\d+$/, "Owned process creation time is unavailable");
      assert.ok(!observed.has(row.ProcessId) || observed.get(row.ProcessId) === row.Created,
        "A reused process identity cannot replace an observed owner");
      observed.set(row.ProcessId, row.Created);
    }
  }
  assert.ok(observed.size <= 128, "Desktop fixture exceeded the owned process budget");
}

async function removeScratch(scratch) {
  const target = path.resolve(scratch);
  assert.equal(path.dirname(target), path.resolve(os.tmpdir()));
  assert.ok(path.basename(target).startsWith("langame-reliability-desktop-"));
  assert.equal((await fs.lstat(target)).isSymbolicLink(), false);
  await fs.rm(target, { recursive: true, force: false, maxRetries: 10, retryDelay: 200 });
}

async function run(executable, output) {
  assert.equal(process.platform, "win32", "The native desktop fixture requires Windows/WebView2");
  assert.ok(path.isAbsolute(executable) && path.isAbsolute(output), "Use absolute executable and output paths");
  assert.ok((await fs.stat(executable)).isFile());
  const relativeOutput = path.relative(path.resolve(desktopRoot, "../.."), output);
  assert.ok(relativeOutput.startsWith(`..${path.sep}`) || path.isAbsolute(relativeOutput), "Evidence must be outside the repository");
  await assert.rejects(fs.stat(output), { code: "ENOENT" }, "Evidence must not overwrite an existing report");
  const scratch = await fs.mkdtemp(path.join(os.tmpdir(), "langame-reliability-desktop-"));
  const observed = new Map();
  let server, child, exited, monitor, monitorError, stopMonitor = false;
  let hostReport, failure, stderr = "", treeExited = false;
  const cleanupErrors = [];
  const watchdog = setTimeout(() => {
    console.error(`Desktop acceptance exceeded its 240-second command deadline; retained scratch: ${scratch}`);
    if (child?.pid && child.exitCode === null && child.signalCode === null) {
      try {
        execFileSync("taskkill.exe", ["/PID", String(child.pid), "/T", "/F"], {
          windowsHide: true, timeout: 5000, stdio: "ignore",
        });
      } catch (error) { console.error(`Owned desktop termination failed: ${error}`); }
    }
    process.exit(1);
  }, 240000);
  watchdog.unref();
  try {
    await execFileAsync(process.execPath, [
      path.join(desktopRoot, "node_modules/typescript/bin/tsc"), "--ignoreConfig", "--noEmit", "--strict",
      "--target", "ES2020", "--module", "ESNext", "--moduleResolution", "bundler",
      "--jsx", "react-jsx", "--esModuleInterop", "--skipLibCheck",
      "src/vite-env.d.ts", "tests/helpers/desktop-reliability.tsx",
    ], { cwd: desktopRoot, windowsHide: true, timeout: 30000, maxBuffer: outputLimit });
    const [{ createServer }, { default: react }] = await Promise.all([import("vite"), import("@vitejs/plugin-react")]);
    server = await createServer({
      configFile: false, root: desktopRoot, cacheDir: path.join(scratch, "vite-cache"),
      appType: "mpa", logLevel: "error", plugins: [react()],
      server: { host: "127.0.0.1", port: 0, strictPort: true },
    });
    await deadline(server.listen(), 15000, "Fixture server startup");
    const address = server.httpServer.address();
    assert.ok(address && typeof address !== "string");
    const config = {
      root: path.join(scratch, "host"),
      url: `http://127.0.0.1:${address.port}/tests/helpers/desktop-reliability.html`,
      nonce: randomUUID(),
    };
    const configPath = path.join(scratch, "config.json");
    await fs.writeFile(configPath, JSON.stringify(config), { flag: "wx" });
    child = spawn(executable, ["--desktop-reliability", configPath], {
      windowsHide: true, stdio: ["ignore", "pipe", "pipe"],
    });
    exited = new Promise((resolve) => {
      child.once("error", (error) => resolve({ error: String(error) }));
      child.once("exit", (code, signal) => resolve({ code, signal }));
    });
    for (const stream of [child.stdout, child.stderr]) {
      stream.on("data", (data) => { stderr = (stderr + data.toString()).slice(-outputLimit); });
    }
    monitor = (async () => {
      while (!stopMonitor) {
        const rows = await processSnapshot();
        recordTree(rows, child.pid, observed);
        if (!stopMonitor) await sleep(500);
      }
    })().catch((error) => { monitorError = error; });
    const exit = await deadline(exited, 195000, "Native desktop acceptance");
    hostReport = JSON.parse(await fs.readFile(path.join(config.root, "report.json"), "utf8"));
    assert.equal(exit.code, 0, `Host exit: ${JSON.stringify(exit)}; report: ${JSON.stringify(hostReport)}`);
    assert.equal(hostReport.passed, true, JSON.stringify(hostReport));
    assert.equal(hostReport.server_exited, true);
    assert.equal(hostReport.output_worker_joined, true);
    assert.equal(hostReport.profile_verified, true);
    for (const key of ["close_hid_window", "browser_recovery_preserved_hidden", "reopened_after_browser_recovery"]) {
      assert.equal(hostReport.window_visibility[key], true, `window_visibility.${key}`);
    }
    const status = hostReport.status;
    assert.equal(status.stage, "finished");
    assert.equal(status.server_alive, false);
    assert.equal(status.command_count, 3);
    assert.deepEqual(status.native_failures, ["renderer_exited", "browser_exited"]);
    assert.deepEqual(status.observations.event_command_counts, [1, 2, 3]);
    assert.deepEqual(status.observations.dom_command_counts, [1, 2, 3]);
    assert.equal(status.observations.max_tail_lines, 400);
    assert.equal(status.observations.dom_nodes_after_unmount, 0);
    assert.equal(status.observations.native_bridge, true);
    assert.deepEqual(status.observations.browser_errors, []);
    assert.equal(status.recovery.recoveries, 2);
    assert.equal(status.recovery.failures, 2);
    assert.equal(status.recovery.paused, false);
    assert.ok(Array.isArray(hostReport.browser_process_ids) && hostReport.browser_process_ids.length >= 2);
  } catch (error) { failure = error; }
  finally {
    if (child?.pid && child.exitCode === null && child.signalCode === null) {
      try {
        // Only the still-owned spawned host and its descendants may be terminated.
        await execFileAsync("taskkill.exe", ["/PID", String(child.pid), "/T", "/F"], {
          windowsHide: true, timeout: 10000, maxBuffer: outputLimit,
        });
        await deadline(exited, 10000, "Owned desktop host termination");
      } catch (error) { cleanupErrors.push(error); }
    }
    stopMonitor = true;
    if (monitor) await monitor;
    if (monitorError) cleanupErrors.push(monitorError);
    try {
      if (!child?.pid) treeExited = true;
      else {
        assert.ok(observed.has(child.pid), "The native host was never observed; process cleanup is unverified");
        const expires = Date.now() + 15000;
        for (;;) {
          const rows = await processSnapshot();
          recordTree(rows, child.pid, observed);
          const remaining = rows.filter((row) => observed.get(row.ProcessId) === row.Created);
          const nativeRemaining = rows.filter((row) => hostReport?.browser_process_ids?.includes(row.ProcessId));
          if (remaining.length === 0 && nativeRemaining.length === 0) { treeExited = true; break; }
          assert.ok(Date.now() < expires, `Owned desktop processes remain: ${remaining.map((row) => row.ProcessId)}`);
          await sleep(100);
        }
      }
    } catch (error) { cleanupErrors.push(error); }
    if (server) {
      try { await deadline(server.close(), 5000, "Fixture server shutdown"); }
      catch (error) { cleanupErrors.push(error); }
    }
  }
  const report = {
    status: !failure && !cleanupErrors.length && treeExited ? "passed" : "failed",
    executable_sha256: createHash("sha256").update(await fs.readFile(executable)).digest("hex"),
    host: hostReport, host_output: stderr,
    processes_observed: observed.size, processes_remaining: treeExited ? 0 : null,
    observed_processes: [...observed].map(([pid, created]) => ({ pid, created })),
    failures: [failure, ...cleanupErrors].filter(Boolean).map(String),
    retained_scratch: scratch,
  };
  // Failed evidence is retained for diagnosis; only verified, successful scratch is disposable.
  if (report.status === "passed") {
    try { await removeScratch(scratch); report.retained_scratch = null; }
    catch (error) { report.status = "failed"; report.failures.push(String(error)); }
  }
  await fs.mkdir(path.dirname(output), { recursive: true });
  await fs.writeFile(output, JSON.stringify(report, null, 2), { flag: "wx" });
  clearTimeout(watchdog);
  console.log(`DESKTOP_RELIABILITY ${JSON.stringify({ status: report.status, report: output, processes_observed: observed.size, retained_scratch: report.retained_scratch })}`);
  assert.equal(report.status, "passed", report.failures.join("; "));
}

module.exports = { recordTree };
if (require.main === module) {
  const args = process.argv.slice(2);
  if (args.length !== 4 || args[0] !== "--executable" || args[2] !== "--output") {
    console.error("Usage: node scripts/verify_desktop_reliability.cjs --executable ABS_EXE --output ABS_REPORT");
    process.exitCode = 1;
  } else {
    run(args[1], args[3]).catch((error) => { console.error(error); process.exitCode = 1; });
  }
}
