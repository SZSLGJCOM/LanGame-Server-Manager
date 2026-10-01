const assert = require("node:assert/strict");
const test = require("node:test");
const { deferred, mountWorkbench, settle } = require("./helpers/runtime-workbench.cjs");

test("reliability: repeated workbench lifecycles release every successful registration and ignore late callbacks", async (t) => {
  const resources = new Set();
  const warnings = [];
  t.mock.method(console, "warn", (...args) => warnings.push(args));
  let released = 0;
  let succeeded = 0;
  let failed = 0;
  for (let cycle = 0; cycle < 60; cycle += 1) {
    const registration = deferred();
    const recoveryRegistration = deferred();
    let readbacks = 0;
    const mounted = mountWorkbench(registration, { recoveryRegistration, onRetryReads: () => { readbacks += 1; } });
    assert.deepEqual(mounted.registrations, ["runtime-log-stream", "runtime-service-events-reset"]);
    const resolveRegistration = () => {
      for (const pending of [registration, recoveryRegistration]) {
        const resource = { cycle, pending };
        resources.add(resource);
        succeeded += 1;
        pending.resolve(() => {
          assert.equal(resources.delete(resource), true, "each native listener is released exactly once");
          released += 1;
        });
      }
    };
    if (cycle % 3 === 0) {
      resolveRegistration();
      await settle();
      assert.equal(resources.size, 2);
      mounted.receive({ lines: [`cycle ${cycle}`] });
      await settle();
      assert.deepEqual(mounted.snapshot().lines, [`cycle ${cycle}`]);
      mounted.reset();
      assert.equal(mounted.snapshot(), null);
      assert.equal(readbacks, 1);
    }
    mounted.dispose();
    const writesAtDisposal = mounted.writes.length;
    const readbacksAtDisposal = readbacks;
    if (cycle % 3 === 1) resolveRegistration();
    if (cycle % 3 === 2) {
      registration.reject(new Error("registration completed after navigation"));
      recoveryRegistration.reject(new Error("recovery registration completed after navigation"));
      failed += 2;
    }
    await settle();
    mounted.receive({ lines: ["event queued before native cleanup"] });
    mounted.reset();
    assert.equal(mounted.writes.length, writesAtDisposal);
    assert.equal(readbacks, readbacksAtDisposal);
    assert.equal(resources.size, 0, `cycle ${cycle} returned to its resource baseline`);
    assert.equal(released, succeeded);
  }
  assert.equal(warnings.length, failed);
  assert.ok(warnings.every(([message]) => /subscription failed/i.test(message)));
});

test("reliability: a retired workbench cannot update its replacement and cleanup failures are observed", async (t) => {
  const warnings = [];
  t.mock.method(console, "warn", (...args) => warnings.push(args));
  const oldRegistration = deferred();
  const old = mountWorkbench(oldRegistration);
  old.dispose();
  const oldWrites = old.writes.length;
  const nextRegistration = deferred();
  const next = mountWorkbench(nextRegistration, { instanceId: "next", logPath: "fixture/run-2.log" });
  t.after(next.dispose);
  let cleanupCalls = 0;
  oldRegistration.resolve(() => { cleanupCalls += 1; throw new Error("bridge cleanup failed"); });
  nextRegistration.resolve(() => {});
  await settle();
  const nextBefore = next.snapshot();
  old.receive({ instance_id: "next", log_path: "fixture/run-2.log", lines: ["stale callback"] });
  assert.equal(old.writes.length, oldWrites);
  assert.ok(Object.is(next.snapshot(), nextBefore));
  next.receive({ lines: ["current response"] });
  await settle();
  assert.deepEqual(next.snapshot().lines, ["current response"]);
  assert.equal(cleanupCalls, 1);
  assert.equal(warnings.length, 1);
  assert.match(warnings[0][0], /cleanup failed/i);
});

test("reliability: sustained console bursts retain a 400-line tail and isolate foreign streams", async (t) => {
  const registration = deferred();
  const mounted = mountWorkbench(registration);
  t.after(mounted.dispose);
  registration.resolve(() => {});
  await settle();
  for (let batch = 0; batch < 80; batch += 1) {
    mounted.receive({ lines: Array.from({ length: 125 }, (_, index) => `line ${batch * 125 + index}`) });
    assert.ok(mounted.snapshot().lines.length <= 400);
  }
  await settle();
  const snapshot = mounted.snapshot();
  assert.equal(snapshot.total_lines, 10_000);
  assert.equal(snapshot.truncated, true);
  assert.deepEqual(snapshot.lines, Array.from({ length: 400 }, (_, index) => `line ${9600 + index}`));
  mounted.receive({ instance_id: "foreign", lines: ["foreign instance"] });
  mounted.receive({ log_path: "fixture/old-run.log", lines: ["different run"] });
  mounted.receive({ lines: [] });
  assert.ok(Object.is(mounted.snapshot(), snapshot), "unrelated input preserves the existing tail");
});

test("reliability: repeated startup shard events share one bounded tail", async (t) => {
  const registration = deferred();
  const mounted = mountWorkbench(registration, { startupPending: true });
  t.after(mounted.dispose);
  registration.resolve(() => {});
  await settle();
  for (let index = 0; index < 1200; index += 1) {
    const process = index % 2 === 0 ? "master" : "caves";
    mounted.receive({ process_key: process, log_path: `fixture/${process}.log`, lines: [`event ${index}`] });
    assert.ok(mounted.snapshot().lines.length <= 400);
  }
  assert.equal(mounted.snapshot().total_lines, 1200);
  assert.equal(mounted.snapshot().lines[0], "[master] event 800");
  assert.equal(mounted.snapshot().lines.at(-1), "[caves] event 1199");
  assert.equal(mounted.snapshot().truncated, true);
});
