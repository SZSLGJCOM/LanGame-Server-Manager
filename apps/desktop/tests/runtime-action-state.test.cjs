const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

function loadRuntimeActionState() {
  const sourcePath = path.join(__dirname, "..", "src", "runtime-action-state.ts");
  const source = fs.readFileSync(sourcePath, "utf8");
  const outputText = transpileTypeScript(source, sourcePath);

  const module = { exports: {} };
  vm.runInNewContext(outputText, {
    module,
    exports: module.exports,
    require
  }, { filename: sourcePath });
  return module.exports;
}

test("resolveServerRuntimeAction exposes a busy start state while the start request is pending", () => {
  const { resolveServerRuntimeAction } = loadRuntimeActionState();

  const model = resolveServerRuntimeAction("Stopped", "starting");

  assert.equal(model.action, "start");
  assert.equal(model.displayedStatus, "Starting");
  assert.equal(model.disabled, true);
  assert.equal(model.ariaBusy, true);
  assert.equal(model.iconName, "refresh");
  assert.equal(model.labelKey, "status.instance.starting");
  assert.match(model.className, /is-busy/);
});

test("resolveServerRuntimeAction keeps running instances stoppable until stop is pending", () => {
  const { resolveServerRuntimeAction } = loadRuntimeActionState();

  const running = resolveServerRuntimeAction("Running", null);
  assert.equal(running.action, "stop");
  assert.equal(running.displayedStatus, "Running");
  assert.equal(running.disabled, false);
  assert.equal(running.ariaBusy, false);
  assert.equal(running.iconName, "square");
  assert.equal(running.labelKey, "common.stop");
  assert.match(running.className, /danger/);

  const stopping = resolveServerRuntimeAction("Running", "stopping");
  assert.equal(stopping.action, "stop");
  assert.equal(stopping.displayedStatus, "Stopping");
  assert.equal(stopping.disabled, true);
  assert.equal(stopping.ariaBusy, true);
  assert.equal(stopping.iconName, "refresh");
  assert.equal(stopping.labelKey, "status.instance.stopping");
  assert.match(stopping.className, /is-busy/);
});

test("resolveServerRuntimeAction keeps degraded instances stoppable while a process survives", () => {
  const { resolveServerRuntimeAction } = loadRuntimeActionState();

  const degraded = resolveServerRuntimeAction("Error", null, true);
  assert.equal(degraded.action, "stop");
  assert.equal(degraded.displayedStatus, "Error");
  assert.equal(degraded.disabled, false);
  assert.equal(degraded.ariaBusy, false);
  assert.equal(degraded.iconName, "square");
  assert.equal(degraded.labelKey, "common.stop");

  const stopping = resolveServerRuntimeAction("Error", "stopping", true);
  assert.equal(stopping.action, "stop");
  assert.equal(stopping.displayedStatus, "Stopping");
  assert.equal(stopping.disabled, true);
  assert.equal(stopping.ariaBusy, true);
});

test("resolveServerRuntimeAction keeps terminal errors startable", () => {
  const { resolveServerRuntimeAction } = loadRuntimeActionState();

  const terminal = resolveServerRuntimeAction("Error", null, false);

  assert.equal(terminal.action, "start");
  assert.equal(terminal.displayedStatus, "Error");
  assert.equal(terminal.disabled, false);
});

test("instanceHasRunningProcess uses live process rows before summary state", () => {
  const { instanceHasRunningProcess } = loadRuntimeActionState();
  const summary = {
    status: "Error",
    active_process_count: 1
  };

  assert.equal(instanceHasRunningProcess(summary), true);
  assert.equal(instanceHasRunningProcess(summary, {
    processes: [
      { process_key: "master", status: "running" },
      { process_key: "caves", status: "error" }
    ]
  }), true);
  assert.equal(instanceHasRunningProcess(summary, {
    processes: [
      { process_key: "master", status: "stopped" },
      { process_key: "caves", status: "error" }
    ]
  }), false);
  assert.equal(instanceHasRunningProcess({ status: "Running", active_process_count: 0 }), true);
  assert.equal(instanceHasRunningProcess({ status: "Error", active_process_count: 0 }), false);
});

test("runtimeProcessIsRunning rejects exited shard rows", () => {
  const { runtimeProcessIsRunning, runtimeProcessKeyIsRunning } = loadRuntimeActionState();

  assert.equal(runtimeProcessIsRunning({ status: "running" }), true);
  assert.equal(runtimeProcessIsRunning({ status: "Running" }), true);
  assert.equal(runtimeProcessIsRunning({ status: "error" }), false);
  assert.equal(runtimeProcessIsRunning({ status: "stopped" }), false);
  assert.equal(runtimeProcessKeyIsRunning({
    processes: [
      { process_key: "master", status: "running" },
      { process_key: "caves", status: "error" }
    ]
  }, "MASTER"), true);
  assert.equal(runtimeProcessKeyIsRunning({
    processes: [
      { process_key: "master", status: "running" },
      { process_key: "caves", status: "error" }
    ]
  }, "caves"), false);
});

test("pendingRuntimeActionForIntent maps button intent to optimistic state", () => {
  const { pendingRuntimeActionForIntent } = loadRuntimeActionState();

  assert.equal(pendingRuntimeActionForIntent("start"), "starting");
  assert.equal(pendingRuntimeActionForIntent("stop"), "stopping");
});
