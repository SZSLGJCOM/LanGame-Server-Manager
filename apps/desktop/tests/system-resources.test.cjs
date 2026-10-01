const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
const labelPath = path.join(__dirname, "../src/domain/system-volume-label.ts");
const labelScope = { exports: {} };
vm.runInNewContext(transpileTypeScript(fs.readFileSync(labelPath, "utf8"), labelPath), {
  module: labelScope, exports: labelScope.exports, require
}, { filename: labelPath });
const sourcePath = path.join(__dirname, "../src/domain/system-resources.ts");
const moduleScope = { exports: {} };
vm.runInNewContext(transpileTypeScript(fs.readFileSync(sourcePath, "utf8"), sourcePath), {
  module: moduleScope, exports: moduleScope.exports,
  require: (name) => name === "./system-volume-label" ? labelScope.exports : require(name)
}, { filename: sourcePath });
const { evaluateSystemResources: evaluate, RESOURCE_STALE_AFTER_MS } = moduleScope.exports;
const GIB = 1024 ** 3;
const NOW = 1_800_000_000_000;
const volume = (overrides = {}) => ({ id: "volume-d", label: "D:\\", paths: ["D:/games"],
  status: "valid", total_bytes: 500 * GIB, available_bytes: 300 * GIB, free_bytes: 300 * GIB, ...overrides });
function snapshot(overrides = {}) {
  return {
    telemetry: { observed_at_unix_ms: NOW, cpu: "valid", cpu_cores: "valid", memory: "valid",
      disk_capacity: "valid", disk_io: "valid", network: "valid" },
    cpu_percent: 40, cpu_single_core_peak_percent: 40,
    memory_percent: 40, memory_total_bytes: 32 * GIB, memory_available_bytes: Math.round(19.2 * GIB),
    memory_commit_used_bytes: 20 * GIB, memory_commit_limit_bytes: 64 * GIB,
    disk_read_latency_ms: 1, disk_write_latency_ms: 1, disk_label: "D:\\",
    disk_volumes: [volume()], network_receive_bps: 0, network_transmit_bps: 0, network_adapters: [],
    ...overrides
  };
}
function at(input, delta) {
  return { ...input, telemetry: { ...input.telemetry, observed_at_unix_ms: NOW + delta } };
}
function assess(input, delta = 0, previous) { return evaluate(at(input, delta), NOW + delta, previous?.observation); }

test("zero defaults and missing telemetry are unknown, never healthy or a score", () => {
  for (const input of [{}, { cpu_percent: 0, memory_percent: 0, disk_used_percent: 0 }, snapshot({ telemetry: null })]) {
    const model = evaluate(input, NOW);
    assert.equal(model.state, "unknown");
    assert.equal(model.cpuPercent, null);
    assert.equal(model.memoryAvailableBytes, null);
    assert.equal(model.network.receiveBps, null);
    assert.equal("healthScore" in model, false);
    assert.equal("safeMargin" in model, false);
  }
});

test("ordinary 40% resource use is normal regardless of traffic and instance inventory", () => {
  const input = snapshot({ memory_available_bytes: Math.round(19.2 * GIB) });
  for (const running_instances of [0, 1, 100]) {
    const model = evaluate({ ...input, running_instances, network_receive_bps: 100 * GIB }, NOW);
    assert.equal(model.state, "normal");
    assert.equal(model.issues.length, 0);
  }
});

test("an exhausted secondary business volume is critical even with missing CPU", () => {
  const input = snapshot({ cpu_percent: null, disk_volumes: [volume(),
    volume({ id: "volume-e", label: "E:\\", paths: ["E:/instances"], available_bytes: 0 })] });
  const model = evaluate(input, NOW);
  assert.equal(model.state, "critical");
  assert.equal(model.issues[0].code, "disk_space");
  assert.equal(model.issues[0].target, "E:");
  assert.equal(model.issues[0].value, 0);
  assert.ok(model.missing.includes("cpu"));
});

test("volume display and disk warnings never expose internal Windows volume identifiers", () => {
  const id = "\\\\?\\Volume{aabbccdd-1234-5678-abcd-123456789abc}\\";
  const model = evaluate(snapshot({ disk_volumes: [
    volume({ id, label: "\\\\?\\D:\\", available_bytes: 0 }),
    volume({ id: "second", label: id, paths: ["C:/Mount/Games/"], available_bytes: 0 }),
    volume({ id: "third", label: id, paths: [id], available_bytes: 0 }),
    volume({ id: "fourth", label: 42, paths: [], available_bytes: 0 })
  ] }), NOW);
  assert.equal(model.volumes[0].id, id);
  assert.equal(model.volumes[0].label, "D:");
  assert.equal(model.volumes[1].label, "C:\\Mount\\Games");
  assert.equal(model.volumes[2].label, "");
  assert.equal(model.issues[2].target, "");
  assert.equal(model.volumes[3].label, "");
  assert.ok(model.issues.every((issue) => !issue.target.includes("Volume{")));
});

test("large absolute disk headroom is not unhealthy merely because percent used is high", () => {
  const model = evaluate(snapshot({ memory_available_bytes: 20 * GIB, disk_volumes: [volume({
    total_bytes: 4000 * GIB, available_bytes: 100 * GIB
  })] }), NOW);
  assert.equal(model.state, "normal");
  assert.equal(model.volumes[0].usedPercent, 97.5);
});

test("low physical memory and commit capacity are independent explicit risks", () => {
  const physical = evaluate(snapshot({ memory_available_bytes: 0.5 * GIB }), NOW);
  assert.equal(physical.state, "critical");
  assert.ok(physical.issues.some((issue) => issue.code === "memory_available"));
  const commit = evaluate(snapshot({ memory_available_bytes: 20 * GIB,
    memory_commit_used_bytes: 63.75 * GIB }), NOW);
  assert.equal(commit.state, "critical");
  assert.ok(commit.issues.some((issue) => issue.code === "memory_commit"));
  assert.equal(commit.memoryCommitAvailableBytes, 0.25 * GIB);
});

test("invalid measurements cannot become normal or fabricate numeric readings", () => {
  for (const value of [null, undefined, NaN, Infinity, -1, 101]) {
    const model = evaluate(snapshot({ cpu_percent: value, memory_available_bytes: 20 * GIB }), NOW);
    assert.equal(model.state, "unknown");
    assert.equal(model.cpuPercent, null);
  }
  for (const changes of [
    { memory_available_bytes: 33 * GIB }, { memory_total_bytes: 0 },
    { memory_commit_used_bytes: 65 * GIB }, { memory_commit_limit_bytes: 0 },
    { disk_volumes: [] }, { disk_volumes: [volume({ status: "unavailable" })] },
    { disk_volumes: [volume({ available_bytes: 600 * GIB })] },
    { disk_volumes: [volume(), volume()] }, { disk_read_latency_ms: NaN }
  ]) assert.equal(evaluate(snapshot({ memory_available_bytes: 20 * GIB, ...changes }), NOW).state, "unknown");
});

test("channel quality overrides numeric zeros and leaves known critical issues visible", () => {
  const input = snapshot({ memory_available_bytes: 20 * GIB });
  input.telemetry.cpu = "warming_up";
  input.telemetry.disk_io = "unavailable";
  const model = evaluate(input, NOW);
  assert.equal(model.state, "unknown");
  assert.equal(model.cpuPercent, null);
  assert.equal(model.diskLatencyMs, null);
  assert.equal(model.quality.cpu, "warming_up");
  input.disk_volumes[0].available_bytes = 0;
  assert.equal(evaluate(input, NOW).state, "critical");
});

test("old data expires without a new snapshot and hides current-value claims", () => {
  const input = snapshot({ memory_available_bytes: 20 * GIB });
  assert.equal(evaluate(input, NOW + RESOURCE_STALE_AFTER_MS - 1).state, "normal");
  const model = evaluate(input, NOW + RESOURCE_STALE_AFTER_MS);
  assert.equal(model.state, "stale");
  assert.equal(model.sampledAt, NOW);
  assert.equal(model.cpuPercent, null);
  assert.equal(model.volumes[0].availableBytes, null);
  assert.equal(model.issues.length, 0);
  assert.equal(evaluate(at(input, 60_000), NOW).state, "unknown");
});

test("CPU pressure requires separated new observations and recovers with hysteresis", () => {
  const hot = snapshot({ memory_available_bytes: 20 * GIB, cpu_percent: 95 });
  const first = assess(hot);
  assert.equal(first.state, "normal");
  const cached = evaluate(hot, NOW + 65_000, first.observation);
  assert.equal(cached.state, "normal", "cached samples cannot confirm high CPU");
  const quick = assess(hot, 1_000, first);
  assert.equal(quick.state, "normal", "rapid refreshes cannot confirm sustained pressure");
  const second = assess(hot, 60_000, first);
  assert.equal(second.state, "watch");
  assert.ok(second.issues.some((issue) => issue.code === "cpu_load"));
  const middle = assess({ ...hot, cpu_percent: 85 }, 120_000, second);
  assert.equal(middle.state, "watch");
  const recovering = assess({ ...hot, cpu_percent: 50 }, 180_000, middle);
  assert.equal(recovering.state, "watch");
  const recovered = assess({ ...hot, cpu_percent: 50 }, 240_000, recovering);
  assert.equal(recovered.state, "normal");
});

test("a single-core bottleneck and I/O pressure are observed independently of average CPU", () => {
  for (const [changes, code] of [
    [{ cpu_percent: 5, cpu_single_core_peak_percent: 99 }, "cpu_core_load"],
    [{ cpu_percent: 5, disk_write_latency_ms: 30 }, "disk_latency"]
  ]) {
    const input = snapshot({ memory_available_bytes: 20 * GIB, ...changes });
    const first = assess(input);
    const second = assess(input, 60_000, first);
    assert.equal(second.state, "watch");
    assert.ok(second.issues.some((issue) => issue.code === code));
  }
});

test("a long gap or missing sample resets confirmation, rather than bridging unknown time", () => {
  const hot = snapshot({ memory_available_bytes: 20 * GIB, cpu_percent: 95 });
  const first = assess(hot);
  assert.equal(assess(hot, RESOURCE_STALE_AFTER_MS + 1, first).state, "normal");
  const failed = assess({ ...hot, cpu_percent: null }, 60_000, first);
  assert.equal(assess(hot, 120_000, failed).state, "normal");
});

test("an intervening cool sample interrupts high confirmation even inside the minimum interval", () => {
  const hot = snapshot({ cpu_percent: 95 });
  const first = assess(hot);
  const cool = assess({ ...hot, cpu_percent: 20 }, 5_000, first);
  const nextHigh = assess(hot, 30_000, cool);
  assert.equal(nextHigh.state, "normal", "separated spikes are not consecutive high samples");
});

test("an intervening high sample interrupts recovery even inside the minimum interval", () => {
  const hot = snapshot({ cpu_percent: 95 });
  const first = assess(hot);
  const active = assess(hot, 30_000, first);
  assert.equal(active.state, "watch");
  const cool = assess({ ...hot, cpu_percent: 20 }, 60_000, active);
  const spike = assess(hot, 65_000, cool);
  const nextLow = assess({ ...hot, cpu_percent: 20 }, 90_000, spike);
  assert.equal(nextLow.state, "watch", "recovery requires consecutive low samples");
});

test("network utilization uses one adapter's directional reported link capacity, never health", () => {
  const adapter = (name, speed, rx, tx, family_name = null) => ({ name, status: "Up", family_name, rate_status: "valid",
    link_speed_bps: speed, receive_bps: rx, transmit_bps: tx });
  const model = evaluate(snapshot({ memory_available_bytes: 20 * GIB, network_receive_bps: 12_500_000,
    network_transmit_bps: 12_500_000, network_adapters: [
      adapter("Ethernet", 1_000_000_000, 12_500_000, 12_500_000),
      adapter("VPN", 1, 100, 100, "Overlay"), adapter("Unknown speed", 0, 100, 100)
    ] }), NOW);
  assert.equal(model.state, "normal");
  assert.equal(model.network.adapterName, "Ethernet");
  assert.equal(model.network.receivePercent, 10);
  assert.equal(model.network.transmitPercent, 10);
  assert.equal(model.network.utilizationPercent, 10, "full-duplex directions must not be summed");
});

test("a valid host total does not turn an unmeasured adapter baseline into 0% utilization", () => {
  for (const rate_status of [undefined, "warming_up", "unavailable"]) {
    const model = evaluate(snapshot({ network_receive_bps: 500, network_adapters: [{
      name: "Ethernet", status: "Up", family_name: null, rate_status,
      link_speed_bps: 1_000_000_000, receive_bps: 0, transmit_bps: 0
    }] }), NOW);
    assert.equal(model.network.receiveBps, 500);
    assert.equal(model.network.utilizationPercent, null);
    assert.equal(model.network.adapterName, null);
  }
});
