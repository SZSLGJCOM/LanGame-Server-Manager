const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

require.extensions[".ts"] = function compileTypeScript(module, filename) {
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
};

const { resolveServerPrimaryAction, ServerInstallationRequestGate } = require(path.join(__dirname, "../src/server-primary-action.ts"));

function resolve(overrides = {}) {
  const input = {
    instanceId: "pz-a",
    moduleId: "pz",
    status: "Stopped",
    installation: { installState: "Installed", hasManagedInstallSource: true },
    ...overrides
  };
  if (!Object.hasOwn(overrides, "launchPlan")) {
    input.launchPlan = {
      instance_id: input.instanceId,
      install_state: input.installation?.installState ?? "Installed",
      uses_private_runtime: false
    };
  }
  return resolveServerPrimaryAction(input);
}

test("a missing managed installation offers installation rather than startup", () => {
  const action = resolve({ installation: { installState: "NotInstalled", hasManagedInstallSource: true } });
  assert.equal(action.action, "install");
  assert.equal(action.labelKey, "servers.actions.install");
  assert.equal(action.disabled, false);
  assert.equal(action.deleteBlocked, false);
});

for (const installState of ["Incomplete", "Corrupted"]) {
  test(`${installState} offers repair and returns to startup once installed`, () => {
    const action = resolve({ installation: { installState, hasManagedInstallSource: true } });
    assert.equal(action.action, "repair");
    assert.equal(action.labelKey, "servers.actions.repair");
    assert.equal(action.disabled, false);
    assert.equal(resolve().action, "start");
  });
}

test("running instances remain stoppable even when the shared files are absent or being repaired", () => {
  const action = resolve({
    status: "Running",
    installation: { installState: "NotInstalled", hasManagedInstallSource: true },
    pendingInstallation: "repair"
  });
  assert.equal(action.action, "stop");
  assert.equal(action.disabled, false);
  assert.equal(action.deleteBlocked, true);
});

test("degraded instances remain stoppable while a managed process survives", () => {
  const action = resolve({
    status: "Error",
    hasRunningProcess: true,
    installation: { installState: "Installed", hasManagedInstallSource: true }
  });

  assert.equal(action.action, "stop");
  assert.equal(action.displayedStatus, "Error");
  assert.equal(action.disabled, false);
  assert.equal(action.deleteBlocked, true);
});

for (const status of ["Starting", "Stopping"]) {
  test(`${status} preserves the runtime busy action before installation decisions`, () => {
    const action = resolve({ status, installation: { installState: "Corrupted", hasManagedInstallSource: true } });
    assert.equal(action.disabled, true);
    assert.equal(action.ariaBusy, true);
    assert.equal(action.deleteBlocked, true);
    assert.equal(action.labelKey, `status.instance.${status.toLowerCase()}`);
  });
}

test("an optimistic runtime action stays busy before the backend status changes", () => {
  const action = resolve({ pendingRuntime: "starting", installation: { installState: "NotInstalled", hasManagedInstallSource: true } });
  assert.equal(action.action, "start");
  assert.equal(action.disabled, true);
  assert.equal(action.labelKey, "status.instance.starting");
});

test("all stopped instances sharing a module reflect one pending repair", () => {
  const jobs = [{ id: "repair-pz", kind: "ValidateGame", target_id: "pz", status: "Running" }];
  for (const instanceId of ["pz-a", "pz-b"]) {
    const action = resolve({ instanceId, jobs });
    assert.equal(action.action, "repair");
    assert.equal(action.disabled, true);
    assert.equal(action.labelKey, "servers.actions.repairing");
  }
  assert.equal(resolve({ moduleId: "another-game", jobs }).disabled, false);
});

test("the immediate local installation intent blocks repeat activation before jobs arrive", () => {
  const action = resolve({ pendingInstallation: "install" });
  assert.equal(action.action, "install");
  assert.equal(action.disabled, true);
  assert.equal(action.ariaBusy, true);
  assert.equal(action.labelKey, "status.install.installing");
});

test("completed and failed jobs do not keep installation actions disabled", () => {
  for (const status of ["Completed", "Failed", "Cancelled"]) {
    const action = resolve({ jobs: [{ id: "old", kind: "DownloadGame", target_id: "pz", status }] });
    assert.equal(action.action, "start");
    assert.equal(action.disabled, false);
  }
});

test("missing files without a managed install source lead to the game library", () => {
  for (const installState of ["NotInstalled", "Corrupted", "Unknown"]) {
    const action = resolve({ installation: { installState, hasManagedInstallSource: false } });
    assert.equal(action.action, "library");
    assert.equal(action.labelKey, "servers.actions.openLibrary");
    assert.equal(action.disabled, false);
  }
});

test("unknown installation capability blocks only noninstalled instances", () => {
  assert.equal(resolve({ installation: { installState: "Installed", hasManagedInstallSource: null } }).action, "start");
  const action = resolve({ installation: { installState: "NotInstalled", hasManagedInstallSource: null } });
  assert.equal(action.action, "checking");
  assert.equal(action.disabled, true);
  assert.equal(action.labelKey, "servers.actions.checking");
});

test("a stale shared preview cannot override a newer shared installation status", () => {
  assert.equal(resolve({ launchPlan: { instance_id: "pz-a", install_state: "NotInstalled", uses_private_runtime: false } }).action, "start");
  assert.equal(resolve({
    installation: { installState: "Corrupted", hasManagedInstallSource: true },
    launchPlan: { instance_id: "pz-a", install_state: "Installed", uses_private_runtime: false }
  }).action, "repair");
});

test("a ready private runtime remains startable despite absent shared files and port preflight errors", () => {
  const action = resolve({
    installation: { installState: "NotInstalled", hasManagedInstallSource: true },
    launchPlan: { instance_id: "pz-a", install_state: "Installed", uses_private_runtime: true, ready_to_launch: false, executable_exists: true },
    jobs: [{ id: "shared-download", kind: "DownloadGame", target_id: "pz", status: "Running" }]
  });
  assert.equal(action.action, "start");
  assert.equal(action.disabled, false);
});

test("damaged private runtimes lead to the library instead of repairing an unrelated shared directory", () => {
  for (const install_state of ["NotInstalled", "Incomplete", "Corrupted"]) {
    const action = resolve({ launchPlan: { instance_id: "pz-a", install_state, uses_private_runtime: true } });
    assert.equal(action.action, "library");
    assert.equal(action.disabled, false);
  }
});

test("unverified missing installations wait for their own plan and ignore another instance plan", () => {
  const installation = { installState: "NotInstalled", hasManagedInstallSource: true };
  for (const launchPlan of [undefined, { instance_id: "pz-b", install_state: "Installed", uses_private_runtime: true }]) {
    const action = resolve({ installation, launchPlan });
    assert.equal(action.action, "checking");
    assert.equal(action.disabled, true);
  }
  assert.equal(resolve({ launchPlan: undefined }).action, "start", "known shared installed files preserve the existing start entry");
});

test("preview failures offer a library action instead of staying in checking", () => {
  const action = resolve({ launchPlan: undefined, launchFailed: true, installation: { installState: "NotInstalled", hasManagedInstallSource: null } });
  assert.equal(action.action, "library");
  assert.equal(action.disabled, false);
});

test("installation clicks route to install or validation and reject same-module duplicates immediately", async () => {
  let finish;
  const gate = new ServerInstallationRequestGate();
  const requests = [];
  const install = (moduleId, validate) => {
    requests.push({ moduleId, validate });
    return new Promise((resolve) => { finish = resolve; });
  };
  const first = gate.run("pz", "install", install);
  assert.equal(await gate.run("pz", "repair", install), false);
  assert.deepEqual(requests, [{ moduleId: "pz", validate: false }]);
  finish();
  assert.equal(await first, true);
  const second = gate.run("pz", "repair", install);
  assert.deepEqual(requests.at(-1), { moduleId: "pz", validate: true });
  finish();
  assert.equal(await second, true);
});

test("failed installation releases the module gate so repair can be retried", async () => {
  const gate = new ServerInstallationRequestGate();
  await assert.rejects(gate.run("pz", "repair", async () => { throw new Error("repair failed"); }), /repair failed/);
  assert.equal(await gate.run("pz", "repair", async () => undefined), true);
});
