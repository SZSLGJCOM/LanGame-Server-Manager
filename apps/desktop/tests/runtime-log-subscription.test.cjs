const assert = require("node:assert/strict");
const test = require("node:test");
const { deferred, mountWorkbench, settle } = require("./helpers/runtime-workbench.cjs");

test("startup subscription rejects delayed old shards and keeps early new shard events", async (t) => {
  const mounted = mountWorkbench({ promise: Promise.resolve(() => {}) }, {
    startupPending: true,
    startupBoundary: { instanceId: "fixture", startedAtUnixMs: 100,
      previousLogPaths: new Set(["fixture/run-1-main.log", "fixture/run-1-caves.log"]) }
  });
  t.after(mounted.dispose);
  await settle();
  mounted.receive({ log_path: "fixture/run-1-caves.log", lines: ["late previous shard"], emitted_at_unix_ms: 150 });
  mounted.receive({ log_path: "fixture/unknown-queued.log", lines: ["old queued event"], emitted_at_unix_ms: 99 });
  assert.deepEqual(mounted.snapshot()?.lines ?? [], []);
  mounted.receive({ log_path: "fixture/run-2-main.log", process_key: "main", lines: ["new main"], emitted_at_unix_ms: 100 });
  mounted.receive({ log_path: "fixture/run-2-caves.log", process_key: "caves", lines: ["new caves"], emitted_at_unix_ms: 101 });
  assert.deepEqual(mounted.snapshot().lines, ["[main] new main", "[caves] new caves"]);
});

test("mounting during startup accepts its already registered new run snapshot", (t) => {
  const mounted = mountWorkbench({ promise: Promise.resolve(() => {}) }, {
    startupPending: true, logPath: "fixture/run-2-main.log",
    startupBoundary: { instanceId: "fixture", startedAtUnixMs: 100,
      previousLogPaths: new Set(["fixture/run-1-main.log"]) },
    runtime: { recent_runs: [], diagnostics: [], health: { status: "starting" },
      log_tail: { source_path: "fixture/run-2-main.log", lines: ["early startup readback"], total_lines: 1 } }
  });
  t.after(mounted.dispose);
  assert.ok(JSON.stringify(mounted.tree).includes("early startup readback"));
  assert.equal(mounted.snapshot(), null, "the fallback must not seed the unpositioned shard aggregate");
  mounted.receive({ log_path: "fixture/run-2-main.log", lines: ["early startup readback"], emitted_at_unix_ms: 101 });
  assert.deepEqual(mounted.snapshot().lines, ["early startup readback"]);
});

test("startup boundaries belong only to their captured instance", (t) => {
  const mounted = mountWorkbench({ promise: Promise.resolve(() => {}) }, {
    instanceId: "other", startupPending: true,
    startupBoundary: { instanceId: "fixture", startedAtUnixMs: 100,
      previousLogPaths: new Set(["fixture/run-2-main.log"]) }
  });
  t.after(mounted.dispose);
  mounted.receive({ log_path: "fixture/run-2-main.log", lines: ["other instance startup"], emitted_at_unix_ms: 1 });
  assert.deepEqual(mounted.snapshot().lines, ["other instance startup"]);
});

for (const subscription of ["log", "event recovery"]) {
  const mount = (registration) => subscription === "log"
    ? mountWorkbench(registration)
    : mountWorkbench({ promise: Promise.resolve(() => {}) }, { recoveryRegistration: registration });

  test(`failed native ${subscription} subscription is handled while the workbench remains mounted`, async (t) => {
    const warnings = [];
    t.mock.method(console, "warn", (...args) => warnings.push(args));
    const registration = deferred();
    const mounted = mount(registration);
    t.after(mounted.dispose);
    registration.reject(new Error("native event bridge unavailable"));
    await settle();
    assert.equal(warnings.length, 1);
    assert.match(String(warnings[0][0]), new RegExp(`${subscription}.*subscription`, "i"));
  });

  test(`late ${subscription} registration is released once and late events cannot update an unmounted workbench`, async () => {
    const registration = deferred();
    const mounted = mount(registration);
    mounted.dispose();
    const writesAtDisposal = mounted.writes.length;
    let releases = 0;
    registration.resolve(() => { releases += 1; });
    await settle();
    mounted.receive({ instance_id: "fixture", lines: ["late event"] });
    mounted.reset();
    assert.equal(releases, 1);
    assert.equal(mounted.writes.length, writesAtDisposal);
  });

  test(`successful native ${subscription} registration is released when the workbench closes`, async (t) => {
    const registration = deferred();
    const mounted = mount(registration);
    t.after(mounted.dispose);
    let releases = 0;
    registration.resolve(() => { releases += 1; });
    await settle();
    assert.equal(releases, 0);
    mounted.dispose();
    await settle();
    assert.equal(releases, 1);
  });
}

test("runtime event reset clears the live tail and requests one readback only while mounted", async (t) => {
  const registration = deferred();
  let readbacks = 0;
  const mounted = mountWorkbench(registration, { onRetryReads: () => { readbacks += 1; } });
  t.after(mounted.dispose);
  registration.resolve(() => {});
  await settle();
  mounted.receive({ lines: ["stale service output"] });
  await settle();
  assert.deepEqual(mounted.snapshot().lines, ["stale service output"]);
  mounted.reset();
  assert.equal(mounted.snapshot(), null);
  assert.equal(readbacks, 1);
  mounted.dispose();
  const writesAtDisposal = mounted.writes.length;
  mounted.reset();
  assert.equal(readbacks, 1);
  assert.equal(mounted.writes.length, writesAtDisposal);
});
