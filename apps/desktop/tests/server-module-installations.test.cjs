const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

require.extensions[".ts"] = (module, filename) => {
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
};
const { ServerModuleInstallationReader } = require("../src/server-module-installations.ts");

const flush = () => new Promise((resolve) => setImmediate(resolve));
const summary = (id, state = "NotInstalled", extra = {}) => ({ id, name: id, version: "1", install_state: state, supported_platforms: ["windows"], ...extra });
const instance = (id, module_id = id, status = "Stopped") => ({ id, module_id, status });
const details = (id, install = {}, extra = {}) => ({ summary: summary(id, "NotInstalled", extra), install });
const preview = (id, extra = {}) => ({ instance_id: id, install_state: "Installed", uses_private_runtime: true, executable_exists: true, ready_to_launch: true, validation_issues: [], ...extra });
const options = (modules, instances = modules.map((item) => instance(item.id))) => ({ enabled: true, modules, instances, selectedInstanceModuleDetails: null, selectedLaunchPlan: null });

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

function harness(overrides = {}) {
  const errors = [];
  const changes = [];
  const reads = [];
  const reader = new ServerModuleInstallationReader({
    readModuleDetails: async (id) => { reads.push(`module:${id}`); return details(id, {}, { steam_app_id: 1 }); },
    previewInstanceLaunch: async (id) => { reads.push(`instance:${id}`); return preview(id); },
    onChange: (snapshot) => changes.push(snapshot),
    onError: (error) => errors.push(error),
    ...overrides
  });
  return { reader, errors, changes, reads, get snapshot() { return changes[changes.length - 1]; } };
}

test("only unique instance modules are read and selected details and preview are reused", async () => {
  const state = harness();
  state.reader.update({
    ...options([summary("pz", "NotInstalled", { steam_app_id: 380870 }), summary("unused")], [instance("a", "pz"), instance("b", "pz", "Running")]),
    selectedInstanceModuleDetails: details("pz", {}, { steam_app_id: 380870 }),
    selectedLaunchPlan: preview("a")
  });
  await flush();

  assert.deepEqual(state.reads, []);
  assert.deepEqual(Object.keys(state.snapshot.moduleInstallations), ["pz"]);
  assert.equal(state.snapshot.moduleInstallations.pz.hasManagedInstallSource, true);
  assert.equal(state.snapshot.instanceLaunchPlans.a.uses_private_runtime, true);
});

test("managed sources require an install spec for Steam, direct download and Minecraft", () => {
  for (const [install, extra, expected] of [
    [null, { steam_app_id: 1 }, false],
    [{}, { steam_app_id: 1 }, true],
    [{ download_url_windows: "https://example.invalid/server.zip" }, {}, true],
    [{ source: "minecraft_java" }, {}, true],
    [{}, {}, false]
  ]) {
    const state = harness();
    state.reader.update({ ...options([summary("m", "Installed", extra)]), selectedInstanceModuleDetails: details("m", install, extra) });
    assert.equal(state.snapshot.moduleInstallations.m.hasManagedInstallSource, expected);
    assert.deepEqual(state.reads, []);
  }
});

test("module details and instance previews share a four-read concurrency bound", async () => {
  const gates = [];
  let active = 0;
  let maximum = 0;
  const hold = (value) => {
    const gate = deferred();
    gates.push(() => gate.resolve(value));
    maximum = Math.max(maximum, ++active);
    return gate.promise.finally(() => { active--; });
  };
  const state = harness({
    readModuleDetails: (id) => hold(details(id)),
    previewInstanceLaunch: (id) => hold(preview(id))
  });
  state.reader.update(options(Array.from({ length: 6 }, (_, index) => summary(String(index)))));
  assert.equal(gates.length, 4);
  while (gates.length) {
    gates.shift()();
    await flush();
  }
  assert.equal(maximum, 4);
  assert.equal(Object.keys(state.snapshot.instanceLaunchPlans).length, 6);
});

test("equivalent polling summaries reuse source and preview reads while install changes invalidate previews", async () => {
  const state = harness({ previewInstanceLaunch: async (id) => { state.reads.push(`instance:${id}`); return preview(id, { uses_private_runtime: false }); } });
  state.reader.update(options([summary("m")]));
  await flush();
  const previous = state.snapshot;
  state.reader.update(options([summary("m")]));
  await flush();
  assert.strictEqual(state.snapshot, previous);
  assert.equal(state.reads.length, 2);

  state.reader.update(options([summary("m", "Corrupted")]));
  await flush();
  assert.equal(state.snapshot.moduleInstallations.m.installState, "Corrupted");
  assert.equal(state.reads.filter((key) => key === "module:m").length, 1);
  assert.equal(state.reads.filter((key) => key === "instance:m").length, 2);
});

test("preview reads are limited to static instances whose shared installation is not installed", async () => {
  const state = harness();
  state.reader.update(options([summary("installed", "Installed"), summary("missing")], [
    instance("a", "installed"), instance("b", "missing", "Running"), instance("c", "missing", "Starting"),
    instance("d", "missing", "Stopping"), instance("e", "missing", "Error"), instance("f", "missing", "Stopped")
  ]));
  await flush();
  assert.deepEqual(state.reads.filter((key) => key.startsWith("instance:")).sort(), ["instance:e", "instance:f"]);
});

test("private installation metadata is retained without interpreting unrelated port validation errors", async () => {
  const plan = preview("m", { ready_to_launch: false, validation_issues: [{ code: "port_binding_unavailable", severity: "error", message: "Port unavailable" }] });
  const state = harness({ previewInstanceLaunch: async () => plan });
  state.reader.update(options([summary("m")]));
  await flush();
  assert.strictEqual(state.snapshot.instanceLaunchPlans.m, plan);
  assert.deepEqual(state.snapshot.instanceLaunchFailures, {});
});

test("failed source and preview reads expose a safe fallback and retry on reentry", async () => {
  let failed = true;
  const state = harness({
    readModuleDetails: async () => { if (failed) throw new Error("detail unavailable"); return details("m", {}, { steam_app_id: 1 }); },
    previewInstanceLaunch: async () => { if (failed) throw new Error("preview unavailable"); return preview("m"); }
  });
  const input = options([summary("m")]);
  state.reader.update(input);
  await flush();
  assert.equal(state.snapshot.moduleInstallations.m.hasManagedInstallSource, false);
  assert.equal(state.snapshot.instanceLaunchFailures.m, true);
  assert.equal(state.snapshot.instanceLaunchPlans.m, undefined);
  assert.equal(state.errors.length, 2);
  state.reader.update({ ...input, modules: [summary("m")] });
  await flush();
  assert.equal(state.errors.length, 2);

  state.reader.update({ ...input, enabled: false });
  failed = false;
  state.reader.update(input);
  await flush();
  assert.equal(state.snapshot.moduleInstallations.m.hasManagedInstallSource, true);
  assert.equal(state.snapshot.instanceLaunchFailures.m, undefined);
  assert.equal(state.snapshot.instanceLaunchPlans.m.install_state, "Installed");
});

test("disabled or removed requests cannot apply late values or errors", async () => {
  const detailGate = deferred();
  const previewGate = deferred();
  const state = harness({ readModuleDetails: () => detailGate.promise, previewInstanceLaunch: () => previewGate.promise });
  const input = options([summary("m")]);
  state.reader.update(input);
  state.reader.update({ ...input, enabled: false, instances: [] });
  const changeCount = state.changes.length;
  detailGate.resolve(details("m", {}, { steam_app_id: 1 }));
  previewGate.reject(new Error("late failure"));
  await flush();
  assert.equal(state.changes.length, changeCount);
  assert.deepEqual(state.errors, []);
  assert.deepEqual(state.snapshot.instanceLaunchPlans, {});
});

test("reentry retains the global in-flight bound until cancelled native reads settle", async () => {
  const gates = [];
  const hold = (value) => { const gate = deferred(); gates.push(() => gate.resolve(value)); return gate.promise; };
  const state = harness({ readModuleDetails: (id) => hold(details(id)), previewInstanceLaunch: (id) => hold(preview(id)) });
  const input = options([summary("a"), summary("b"), summary("c")]);
  state.reader.update(input);
  assert.equal(gates.length, 4);
  state.reader.update({ ...input, enabled: false });
  state.reader.update(input);
  assert.equal(gates.length, 4);
  while (gates.length) { gates.shift()(); await flush(); }
  assert.equal(Object.keys(state.snapshot.instanceLaunchPlans).length, 3);
});

test("mismatched API identities are errors and never evidence for another instance or module", async () => {
  const state = harness({ readModuleDetails: async () => details("other"), previewInstanceLaunch: async () => preview("other") });
  state.reader.update(options([summary("m")]));
  await flush();
  assert.equal(state.errors.length, 2);
  assert.equal(state.snapshot.moduleInstallations.m.hasManagedInstallSource, false);
  assert.equal(state.snapshot.instanceLaunchFailures.m, true);
  assert.deepEqual(state.snapshot.instanceLaunchPlans, {});
});

test("a selected preview cached before shared-state changes is not reused as fresh filesystem evidence", async () => {
  const gate = deferred();
  const state = harness({ previewInstanceLaunch: () => gate.promise });
  const stalePlan = preview("m", { uses_private_runtime: false });
  state.reader.update({ ...options([summary("m", "Installed")]), selectedLaunchPlan: stalePlan });
  await flush();
  state.reader.update({ ...options([summary("m")]), selectedLaunchPlan: stalePlan });
  assert.equal(state.snapshot.instanceLaunchPlans.m, undefined);
  gate.resolve(preview("m", { install_state: "NotInstalled", uses_private_runtime: false, executable_exists: false }));
  await flush();
  assert.equal(state.snapshot.instanceLaunchPlans.m.install_state, "NotInstalled");
});

test("newly installed shared files remove older per-instance missing-file evidence", async () => {
  const stalePlan = preview("m", { install_state: "NotInstalled", uses_private_runtime: false });
  const state = harness({ previewInstanceLaunch: async () => stalePlan });
  state.reader.update({ ...options([summary("m")]), selectedLaunchPlan: stalePlan });
  await flush();
  assert.strictEqual(state.snapshot.instanceLaunchPlans.m, stalePlan);
  state.reader.update({ ...options([summary("m", "Installed")]), selectedLaunchPlan: stalePlan });
  assert.equal(state.snapshot.instanceLaunchPlans.m, undefined);
  assert.equal(state.snapshot.moduleInstallations.m.installState, "Installed");
});

test("a newer selected preview supersedes an in-flight result without losing its file evidence", async () => {
  const gate = deferred();
  const state = harness({ previewInstanceLaunch: () => gate.promise });
  const input = options([summary("m")]);
  state.reader.update(input);
  const current = preview("m", { install_state: "Corrupted" });
  state.reader.update({ ...input, selectedLaunchPlan: current });
  gate.resolve(preview("m"));
  await flush();
  assert.strictEqual(state.snapshot.instanceLaunchPlans.m, current);
});

test("synchronous read failures release queue slots and report a retryable fallback", async () => {
  const state = harness({
    readModuleDetails: () => { throw new Error("detail invocation failed"); },
    previewInstanceLaunch: () => { throw new Error("preview invocation failed"); }
  });
  state.reader.update(options(Array.from({ length: 5 }, (_, index) => summary(String(index)))));
  await flush();
  assert.equal(state.errors.length, 10);
  assert.equal(Object.keys(state.snapshot.instanceLaunchFailures).length, 5);
  assert.ok(Object.values(state.snapshot.moduleInstallations).every((value) => value.hasManagedInstallSource === false));
});

test("an installed shared module retains a selected private installation failure", async () => {
  const state = harness();
  const privatePlan = preview("m", { install_state: "Corrupted" });
  state.reader.update({ ...options([summary("m", "Installed")]), selectedLaunchPlan: privatePlan });
  await flush();

  assert.strictEqual(state.snapshot.instanceLaunchPlans.m, privatePlan);
  assert.equal(state.snapshot.moduleInstallations.m.installState, "Installed");
  assert.deepEqual(state.reads.filter((read) => read.startsWith("instance:")), []);
});

test("shared restoration and reentry keep known private scope for installed and corrupted instances", async () => {
  for (const installState of ["Installed", "Corrupted"]) {
    const gate = deferred();
    const state = harness({ previewInstanceLaunch: () => gate.promise });
    const privatePlan = preview("m", { install_state: installState });
    state.reader.update({ ...options([summary("m")]), selectedLaunchPlan: privatePlan });
    await flush();
    const restored = { ...options([summary("m", "Installed")]), selectedLaunchPlan: null };
    state.reader.update(restored);
    assert.strictEqual(state.snapshot.instanceLaunchPlans.m, privatePlan);

    state.reader.update({ ...restored, enabled: false });
    state.reader.update(restored);
    assert.strictEqual(state.snapshot.instanceLaunchPlans.m, privatePlan, "pending refresh must preserve private scope");
    gate.resolve(preview("m", { install_state: installState }));
    await flush();
    assert.equal(state.snapshot.instanceLaunchPlans.m.uses_private_runtime, true);
    assert.equal(state.snapshot.instanceLaunchPlans.m.install_state, installState);
  }
});
