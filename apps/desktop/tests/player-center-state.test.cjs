const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const desktopRoot = path.resolve(__dirname, "..");

require.extensions[".ts"] = function compileTypeScript(module, filename) {
  const source = fs.readFileSync(filename, "utf8");
  const outputText = transpileTypeScript(source, filename);
  module._compile(outputText, filename);
};

const {
  beginPlayerCenterRequest,
  completePlayerCenterRequest,
  createPlayerCenterState,
  deriveLivePlayerPresentation,
  derivePlayerCenterViewModel,
  isAuthoritativeLivePlayerSnapshot,
  reducePlayerCenterState
} = require(path.join(desktopRoot, "src", "domain", "live-player-state.ts"));
const {
  LivePlayerRefreshController
} = require(path.join(desktopRoot, "src", "domain", "live-player-refresh.ts"));

function player(playerKey, displayName = `Player ${playerKey}`) {
  return {
    player_key: playerKey,
    display_name: displayName,
    identifiers: [],
    available_action_ids: ["kick"],
    ping_ms: null,
    session_started_at_unix_ms: null,
    role: null,
    attributes: []
  };
}

function snapshot(overrides = {}) {
  return {
    snapshot_id: "snapshot-1",
    instance_id: "server-a",
    status: "ready",
    source: "runtime_action",
    observed_at_unix_ms: 900,
    expires_at_unix_ms: 2_000,
    complete: true,
    truncated: false,
    stale: false,
    current_players: 1,
    max_players: 16,
    entries: [player("player-a")],
    issue: null,
    ...overrides
  };
}

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

function flushPromises() {
  return new Promise((resolve) => setImmediate(resolve));
}

class FakeScheduler {
  constructor(now = 1_000) {
    this.now = now;
    this.nextId = 1;
    this.tasks = new Map();
    this.cancelled = [];
  }

  schedule = (callback, delayMs) => {
    const handle = this.nextId++;
    this.tasks.set(handle, { callback, delayMs, dueAt: this.now + delayMs });
    return handle;
  };

  cancel = (handle) => {
    if (this.tasks.delete(handle)) {
      this.cancelled.push(handle);
    }
  };

  active() {
    return [...this.tasks.entries()];
  }

  run(handle) {
    const task = this.tasks.get(handle);
    assert.ok(task, `scheduled task ${handle} must exist`);
    this.tasks.delete(handle);
    this.now = task.dueAt;
    task.callback();
  }
}

test("player workspace derives live presentation without hiding players behind subviews", () => {
  const state = createPlayerCenterState({ instanceId: "server-a", snapshot: snapshot() });
  const view = derivePlayerCenterViewModel(state, 1_000);
  assert.equal(view.livePlayers.kind, "ready");
  assert.equal(view.livePlayers.rows.length, 1);
  assert.equal(view.livePlayers.selectedPlayerKey, null);
  assert.equal(Object.hasOwn(state, "activeSubview"), false);
  assert.equal(Object.hasOwn(view, "visibleSubviews"), false);
});

test("live snapshots map to explicit, non-overlapping presentation states", () => {
  const now = 1_000;
  const cases = [
    ["ready", snapshot(), "ready", true, false],
    ["authoritative empty", snapshot({ entries: [], current_players: 0 }), "empty", false, true],
    ["refreshing with old rows", snapshot({ status: "refreshing", stale: true }), "refreshing-with-rows", true, false],
    ["stopped", snapshot({ status: "stopped", entries: [] }), "stopped", false, true],
    ["unsupported", snapshot({ status: "unsupported", entries: [] }), "unsupported", false, true],
    ["misconfigured", snapshot({ status: "misconfigured", entries: [] }), "misconfigured", false, true],
    ["failed with stale rows", snapshot({ status: "failed", stale: true }), "failed-with-rows", true, true],
    ["failed without stale rows", snapshot({ status: "failed", stale: true, entries: [] }), "failed-without-rows", false, true],
    ["truncated", snapshot({ truncated: true }), "truncated", true, true],
    ["incomplete", snapshot({ complete: false }), "incomplete", true, true]
  ];

  for (const [label, input, expectedKind, tableVisible, stateVisible] of cases) {
    const presentation = deriveLivePlayerPresentation(input, now, input.entries[0]?.player_key ?? null);
    assert.equal(presentation.kind, expectedKind, label);
    assert.equal(presentation.tableVisible, tableVisible, `${label}: table visibility`);
    assert.equal(presentation.stateVisible, stateVisible, `${label}: state visibility`);
    assert.equal(
      presentation.actionPanelVisible,
      tableVisible && input.entries.length > 0,
      `${label}: selected-player panel visibility`
    );
  }
});

test("only fresh complete ready snapshots can authoritatively report nobody online", () => {
  const now = 1_000;
  const authoritativeEmpty = snapshot({ entries: [], current_players: 0 });

  assert.equal(isAuthoritativeLivePlayerSnapshot(authoritativeEmpty, now), true);
  assert.equal(deriveLivePlayerPresentation(authoritativeEmpty, now, null).kind, "empty");

  const nonAuthoritativeEmptySnapshots = [
    snapshot({ entries: [], status: "refreshing" }),
    snapshot({ entries: [], status: "failed", stale: true }),
    snapshot({ entries: [], complete: false }),
    snapshot({ entries: [], truncated: true }),
    snapshot({ entries: [], stale: true }),
    snapshot({ entries: [], expires_at_unix_ms: now }),
    snapshot({ entries: [], expires_at_unix_ms: null })
  ];

  for (const input of nonAuthoritativeEmptySnapshots) {
    const presentation = deriveLivePlayerPresentation(input, now, null);
    assert.notEqual(presentation.kind, "empty");
    assert.equal(presentation.authoritativeEmpty, false);
  }

  const expired = deriveLivePlayerPresentation(
    snapshot({ entries: [], expires_at_unix_ms: now - 1 }),
    now,
    null
  );
  assert.equal(expired.kind, "refreshing-without-rows");
  assert.equal(expired.refreshReason, "expired");
});

test("selection is scoped by player_key and clears when a replacement snapshot removes that key", () => {
  const duplicateName = "Same display name";
  let state = createPlayerCenterState({ instanceId: "server-a" });
  state = reducePlayerCenterState(state, {
    type: "snapshot-received",
    snapshot: snapshot({
      entries: [player("stable-row-a", duplicateName), player("stable-row-b", duplicateName)]
    })
  });
  state = reducePlayerCenterState(state, {
    type: "player-selected",
    playerKey: "stable-row-a"
  });
  assert.equal(state.selectedPlayerKey, "stable-row-a");

  state = reducePlayerCenterState(state, {
    type: "snapshot-received",
    snapshot: snapshot({
      snapshot_id: "snapshot-2",
      entries: [player("stable-row-b", duplicateName)]
    })
  });
  assert.equal(state.selectedPlayerKey, null);

  state = reducePlayerCenterState(state, {
    type: "player-selected",
    playerKey: duplicateName
  });
  assert.equal(state.selectedPlayerKey, null, "display names are never accepted as row identity");
});

test("instance and request generations reject obsolete snapshot completions", () => {
  let state = createPlayerCenterState({ instanceId: "server-a" });
  const older = beginPlayerCenterRequest(state);
  state = older.state;
  const newer = beginPlayerCenterRequest(state);
  state = newer.state;

  const unchanged = completePlayerCenterRequest(
    state,
    older.token,
    snapshot({ snapshot_id: "older" })
  );
  assert.equal(unchanged.snapshot, null);

  state = completePlayerCenterRequest(
    unchanged,
    newer.token,
    snapshot({ snapshot_id: "newer" })
  );
  assert.equal(state.snapshot.snapshot_id, "newer");

  const pendingA = beginPlayerCenterRequest(state);
  state = reducePlayerCenterState(pendingA.state, {
    type: "instance-changed",
    instanceId: "server-b"
  });
  state = completePlayerCenterRequest(
    state,
    pendingA.token,
    snapshot({ snapshot_id: "late-a" })
  );
  assert.equal(state.instanceId, "server-b");
  assert.equal(state.snapshot, null);
  assert.equal(state.selectedPlayerKey, null);
});

test("fresh authoritative snapshots schedule exactly one expiry refresh", async () => {
  const scheduler = new FakeScheduler();
  const refreshed = deferred();
  const emitted = [];
  const controller = new LivePlayerRefreshController({
    now: () => scheduler.now,
    schedule: scheduler.schedule,
    cancel: scheduler.cancel,
    refresh: (instanceId) => {
      assert.equal(instanceId, "server-a");
      return refreshed.promise;
    },
    onSnapshot: (next) => emitted.push(next.snapshot_id)
  });

  controller.setContext({
    instanceId: "server-a",
    visible: true,
    snapshot: snapshot({ expires_at_unix_ms: 1_250 })
  });
  assert.equal(scheduler.active().length, 1);
  const [handle, task] = scheduler.active()[0];
  assert.equal(task.delayMs, 250);

  scheduler.run(handle);
  assert.equal(scheduler.active().length, 0);
  refreshed.resolve(snapshot({ snapshot_id: "after-expiry", expires_at_unix_ms: 2_000 }));
  await flushPromises();

  assert.deepEqual(emitted, ["after-expiry"]);
  assert.equal(scheduler.active().length, 1, "accepted refresh schedules from its new expiry");
});

test("context changes replace timers while hidden, terminal, and disposed contexts schedule none", () => {
  const scheduler = new FakeScheduler();
  const controller = new LivePlayerRefreshController({
    now: () => scheduler.now,
    schedule: scheduler.schedule,
    cancel: scheduler.cancel,
    refresh: async () => snapshot(),
    onSnapshot: () => {}
  });

  controller.setContext({ instanceId: "server-a", visible: true, snapshot: snapshot() });
  const firstHandle = scheduler.active()[0][0];
  controller.setContext({
    instanceId: "server-a",
    visible: true,
    snapshot: snapshot({ snapshot_id: "replacement", expires_at_unix_ms: 3_000 })
  });
  assert.deepEqual(scheduler.cancelled, [firstHandle]);
  assert.equal(scheduler.active().length, 1);

  controller.setContext({ instanceId: "server-a", visible: false, snapshot: snapshot() });
  assert.equal(scheduler.active().length, 0);

  for (const status of ["stopped", "unsupported", "misconfigured", "failed", "refreshing"]) {
    controller.setContext({
      instanceId: "server-a",
      visible: true,
      snapshot: snapshot({ snapshot_id: status, status })
    });
    assert.equal(scheduler.active().length, 0, status);
  }

  controller.setContext({ instanceId: "server-a", visible: true, snapshot: snapshot({ snapshot_id: "last" }) });
  assert.equal(scheduler.active().length, 1);
  controller.dispose();
  assert.equal(scheduler.active().length, 0);

  controller.setContext({ instanceId: "server-a", visible: true, snapshot: snapshot({ snapshot_id: "ignored" }) });
  assert.equal(scheduler.active().length, 0);
});

test("refresh controller drops late reads, refreshes, instance responses, and disposed completions", async () => {
  const scheduler = new FakeScheduler();
  const refreshA = deferred();
  const emitted = [];
  const controller = new LivePlayerRefreshController({
    now: () => scheduler.now,
    schedule: scheduler.schedule,
    cancel: scheduler.cancel,
    refresh: () => refreshA.promise,
    onSnapshot: (next) => emitted.push(`${next.instance_id}:${next.snapshot_id}`)
  });

  controller.setContext({ instanceId: "server-a", visible: true, snapshot: null });
  const readA = deferred();
  const oldRead = controller.read(() => readA.promise);
  const oldRefresh = controller.refreshNow();

  controller.setContext({ instanceId: "server-b", visible: true, snapshot: null });
  const readB = deferred();
  const currentRead = controller.read(() => readB.promise);
  readB.resolve(snapshot({ instance_id: "server-b", snapshot_id: "current-b" }));
  assert.equal(await currentRead, true);

  readA.resolve(snapshot({ snapshot_id: "late-read-a" }));
  refreshA.resolve(snapshot({ snapshot_id: "late-refresh-a" }));
  assert.equal(await oldRead, false);
  assert.equal(await oldRefresh, false);
  assert.deepEqual(emitted, ["server-b:current-b"]);

  const disposedRead = deferred();
  const pendingDisposed = controller.read(() => disposedRead.promise);
  controller.dispose();
  disposedRead.resolve(snapshot({ instance_id: "server-b", snapshot_id: "disposed" }));
  assert.equal(await pendingDisposed, false);
  assert.deepEqual(emitted, ["server-b:current-b"]);
});

test("a newer same-instance refresh wins over an older read", async () => {
  const scheduler = new FakeScheduler();
  const oldRead = deferred();
  const newRefresh = deferred();
  const emitted = [];
  const controller = new LivePlayerRefreshController({
    now: () => scheduler.now,
    schedule: scheduler.schedule,
    cancel: scheduler.cancel,
    refresh: () => newRefresh.promise,
    onSnapshot: (next) => emitted.push(next.snapshot_id)
  });

  controller.setContext({ instanceId: "server-a", visible: true, snapshot: null });
  const olderPromise = controller.read(() => oldRead.promise);
  const newerPromise = controller.refreshNow();

  newRefresh.resolve(snapshot({ snapshot_id: "new-refresh" }));
  assert.equal(await newerPromise, true);
  oldRead.resolve(snapshot({ snapshot_id: "old-read" }));
  assert.equal(await olderPromise, false);
  assert.deepEqual(emitted, ["new-refresh"]);
});

test("read-only names remain visible without an empty member action panel", () => {
  const nameOnly = { ...player("query-row"), identifiers: [], available_action_ids: [] };
  const presentation = deriveLivePlayerPresentation(snapshot({ entries: [nameOnly] }), 1_000, "query-row");
  assert.equal(presentation.tableVisible, true);
  assert.equal(presentation.actionPanelVisible, false);
  assert.equal(presentation.actionsEnabled, false);
});

test("missing adapters, missing names and empty servers are different states", () => {
  for (const [code, expected] of [["adapter_unavailable", "adapter-unavailable"], ["names_unavailable", "count-only"]]) {
    const view = deriveLivePlayerPresentation(snapshot({ status: "unsupported", entries: [], issue: { code, setting_keys: [], summary: "unavailable" } }), 1_000, null);
    assert.equal(view.kind, expected);
    assert.equal(view.authoritativeEmpty, false);
  }
  const countOnly = deriveLivePlayerPresentation(snapshot({ entries: [], current_players: 5 }), 1_000, null);
  assert.equal(countOnly.kind, "count-only");
  assert.equal(countOnly.authoritativeEmpty, false);
  const anonymousQuery = snapshot({ entries: [], current_players: 5, complete: false, issue: { code: "names_unavailable", setting_keys: [], summary: "anonymous response" } });
  assert.equal(deriveLivePlayerPresentation(anonymousQuery, 1_000, null).kind, "count-only");
  const unknownQuery = deriveLivePlayerPresentation({ ...anonymousQuery, current_players: null }, 1_000, null);
  assert.equal(unknownQuery.kind, "incomplete");
  assert.equal(unknownQuery.authoritativeEmpty, false);
});

test("transient request failures retry at bounded delays, then require a manual retry", async () => {
  const scheduler = new FakeScheduler();
  let calls = 0;
  const controller = new LivePlayerRefreshController({
    now: () => scheduler.now, schedule: scheduler.schedule, cancel: scheduler.cancel,
    refresh: async () => { calls += 1; throw new Error("offline"); },
    onSnapshot: () => {}
  });
  controller.setContext({ instanceId: "server-a", visible: true, snapshot: null });
  await controller.refreshNow();
  for (const expectedDelay of [5_000, 10_000, 20_000]) {
    assert.equal(scheduler.active().length, 1);
    const [handle, task] = scheduler.active()[0];
    assert.equal(task.delayMs, expectedDelay);
    scheduler.run(handle);
    await flushPromises();
  }
  assert.equal(calls, 4);
  assert.equal(scheduler.active().length, 0);
  await controller.refreshNow();
  assert.equal(scheduler.active()[0][1].delayMs, 5_000);
  controller.dispose();
});

test("a failed snapshot recovers and resumes ordinary expiry scheduling", async () => {
  const scheduler = new FakeScheduler();
  let calls = 0;
  const controller = new LivePlayerRefreshController({
    now: () => scheduler.now, schedule: scheduler.schedule, cancel: scheduler.cancel,
    refresh: async () => ++calls === 1
      ? snapshot({ status: "failed", stale: true, issue: { code: "query_unavailable", setting_keys: [], summary: "query timed out" } })
      : snapshot({ expires_at_unix_ms: scheduler.now + 30_000 }),
    onSnapshot: () => {}
  });
  controller.setContext({ instanceId: "server-a", visible: true, snapshot: null });
  await controller.refreshNow();
  scheduler.run(scheduler.active()[0][0]);
  await flushPromises();
  assert.equal(calls, 2);
  assert.equal(scheduler.active()[0][1].delayMs, 30_000);
  controller.dispose();
});

test("authentication failures do not automatically repeat credentials", async () => {
  const scheduler = new FakeScheduler();
  const controller = new LivePlayerRefreshController({
    now: () => scheduler.now, schedule: scheduler.schedule, cancel: scheduler.cancel,
    refresh: async () => snapshot({ status: "failed", issue: { code: "authentication_failed", setting_keys: [], summary: "unauthorized" } }),
    onSnapshot: () => {}
  });
  controller.setContext({ instanceId: "server-a", visible: true, snapshot: null });
  await controller.refreshNow();
  assert.equal(scheduler.active().length, 0);
  controller.dispose();
});

test("a list adapter only consumes its own actions and preserves unrelated manual actions", () => {
  const { readManualPlayerActions } = require(path.join(desktopRoot, "src", "views", "servers", "player-center", "manual-player-action-model.ts"));
  const action = (id, kind = "kick") => ({ id, kind, target_required: true, command_template: "command {{target}}" });
  const runtime = {
    player_list: { action_id: "list_players", player_action_ids: ["kick_player"] },
    player_actions: [action("list_players"), action("kick_player"), action("unban_player"), action("broadcast", "broadcast")],
    player_management: { status: "runtime_actions" }
  };
  assert.deepEqual(readManualPlayerActions({ runtime }, []).map((entry) => entry.id), ["unban_player"]);
  assert.deepEqual(readManualPlayerActions({ runtime: { ...runtime, player_list: null } }, []).map((entry) => entry.id), ["list_players", "kick_player", "unban_player"]);
  assert.deepEqual(readManualPlayerActions({ runtime: { ...runtime, player_management: { status: "pending_adapter" } } }, []), []);
});
