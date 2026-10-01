// Feature-gated real Windows service acceptance. This is deliberately a
// native tray and IPC lifecycle test; it does not claim full UI or real-game coverage.
const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const os = require("node:os");
const { spawn, execFile } = require("node:child_process");
const { promisify } = require("node:util");
const { randomUUID, createHash } = require("node:crypto");
const execFileAsync = promisify(execFile);

async function run(executable, output) {
  assert.equal(process.platform, "win32", "Native runtime service acceptance requires Windows");
  assert.ok(path.isAbsolute(executable) && path.isAbsolute(output), "Use absolute executable and output paths");
  assert.ok((await fs.stat(executable)).isFile());
  const repo = path.resolve(__dirname, "../../..");
  const relativeOutput = path.relative(repo, output);
  assert.ok(relativeOutput.startsWith(`..${path.sep}`) || path.isAbsolute(relativeOutput), "Evidence must be outside the repository");
  await assert.rejects(fs.stat(output), { code: "ENOENT" }, "Do not overwrite existing evidence");
  await fs.mkdir(path.dirname(output), { recursive: true });
  const scratch = await fs.mkdtemp(path.join(os.tmpdir(), "langame-runtime-service-"));
  const config = { root: path.join(scratch, "fixture"), nonce: randomUUID() };
  const configPath = path.join(scratch, "config.json");
  await fs.writeFile(configPath, JSON.stringify(config), { flag: "wx" });
  let child, timer, logs = "", host, failure;
  try {
    child = spawn(executable, ["--runtime-service-fixture", configPath], {
      windowsHide: true, stdio: ["ignore", "pipe", "pipe"],
    });
    for (const stream of [child.stdout, child.stderr]) stream.on("data", (data) => { logs = (logs + data).slice(-128 * 1024); });
    const exited = new Promise((resolve, reject) => {
      child.once("error", reject);
      child.once("exit", (code, signal) => resolve({ code, signal }));
    });
    const result = await Promise.race([exited, new Promise((_, reject) => {
      timer = setTimeout(() => reject(new Error("Runtime service acceptance exceeded 220 seconds")), 220000);
    })]);
    assert.equal(result.code, 0, `Fixture exit ${JSON.stringify(result)}; ${logs}`);
    host = JSON.parse(await fs.readFile(path.join(config.root, "report.json"), "utf8"));
    assert.equal(host.passed, true);
    for (const key of ["started_by_first_client", "survived_first_client_exit", "reconnected_same_processes", "log_continuity", "save_before_stop", "shutdown_receipt_acknowledged", "service_exited", "game_exited", "owned_process_trees_joined", "stopped_state_persisted", "world_saved_and_backed_up"]) assert.equal(host[key], true, key);
    assert.equal(new Set(host.client_pids).size, 2);
    assert.equal(host.first_client.service_pid, host.second_client.service_pid);
    assert.equal(host.first_client.game_pid, host.second_client.game_pid);
    for (const key of ["localized_menu", "native_close_requested", "window_hidden", "tray_retained", "client_remained_open", "window_restored", "isolated_profile_verified"]) {
      assert.equal(host.first_client.tray[key], true, `tray.${key}`);
    }
    assert.equal(host.first_client.tray.same_service_pid, host.service_pid);
    assert.equal(host.second_client.disconnected_probes, 20);
    assert.equal(host.native_firewall_exercised, false);
  } catch (error) { failure = error; }
  finally {
    clearTimeout(timer);
    // The owned fixture process holds all nested kill-on-close Jobs. Never
    // target the production service name or a PID discovered outside this tree.
    if (child?.pid && child.exitCode === null && child.signalCode === null) {
      try { await execFileAsync("taskkill.exe", ["/PID", String(child.pid), "/T", "/F"], { windowsHide: true, timeout: 10000 }); }
      catch (error) { failure = new AggregateError([failure, error].filter(Boolean), "Fixture cleanup failed"); }
    }
  }
  const report = {
    status: failure ? "failed" : "passed", fixture: host,
    executable_sha256: createHash("sha256").update(await fs.readFile(executable)).digest("hex"),
    error: failure ? String(failure) : null, output: logs, retained_scratch: scratch,
    client_logs: {},
    coverage: "Real window close-to-tray/restore, named pipe/service/managed process lifecycle with separate clients and a synthetic loopback game; excludes native firewall changes, tray-menu click automation and full application rendering",
  };
  for (const role of ["start", "stop"]) {
    const file = path.join(config.root, "logs", `runtime-service-fixture-client-${role}.log`);
    try {
      const handle = await fs.open(file, "r");
      try {
        const { size } = await handle.stat();
        const buffer = Buffer.alloc(Math.min(size, 32 * 1024));
        const { bytesRead } = await handle.read(buffer, 0, buffer.length, Math.max(0, size - buffer.length));
        report.client_logs[role] = buffer.subarray(0, bytesRead).toString("utf8");
      } finally { await handle.close(); }
    } catch (error) {
      if (error.code !== "ENOENT") report.client_logs[role] = `Unable to read fixture log: ${error}`;
    }
  }
  await fs.mkdir(path.dirname(output), { recursive: true });
  await fs.writeFile(output, JSON.stringify(report, null, 2), { flag: "wx" });
  console.log(`RUNTIME_SERVICE_ACCEPTANCE ${JSON.stringify({ status: report.status, report: output, scratch })}`);
  assert.equal(report.status, "passed", `${report.error}\n${JSON.stringify(report.client_logs)}`);
}

if (require.main === module) {
  const args = process.argv.slice(2);
  if (args.length !== 4 || args[0] !== "--executable" || args[2] !== "--output") {
    console.error("Usage: node scripts/verify_runtime_service.cjs --executable ABS_EXE --output ABS_REPORT");
    process.exitCode = 1;
  } else run(args[1], args[3]).catch((error) => { console.error(error); process.exitCode = 1; });
}
