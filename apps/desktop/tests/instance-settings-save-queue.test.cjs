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

const { InstanceSettingsSaveQueue } = require(path.join(
  desktopRoot,
  "src",
  "views",
  "settings",
  "instance-settings-save-queue.ts"
));
const { normalizeConfigurationSaveError } = require(path.join(
  desktopRoot,
  "src",
  "views",
  "settings",
  "configuration-save-error.ts"
));

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

async function settleQueue() {
  await new Promise((resolve) => setImmediate(resolve));
  await Promise.resolve();
}

function input(id, settingsJson) {
  return {
    id,
    bind_ip: "0.0.0.0",
    auto_backup_on_stop: false,
    backup_retention_count: 3,
    settings_json: settingsJson,
    ports: []
  };
}

function createQueue(execute, overrides = {}) {
  const statuses = [];
  const queue = new InstanceSettingsSaveQueue({
    instanceId: "server-a",
    settingsBaseline: '{"roster":[]}',
    savedSignature: "base",
    execute: async (nextInput, expectedSettingsJson) => {
      const acknowledged = await execute(nextInput, expectedSettingsJson);
      return acknowledged === undefined ? nextInput.settings_json : acknowledged;
    },
    onStatusChange: (status) => statuses.push(status),
    ...overrides
  });
  return { queue, statuses };
}

test("successive edits use the persisted acknowledgement when storage normalizes a draft", async () => {
  const calls = [];
  let persisted = '{"name":"A","rate":1}';
  const { queue } = createQueue((nextInput, expectedSettingsJson) => {
    calls.push(expectedSettingsJson);
    assert.equal(expectedSettingsJson, persisted, "CAS must use the stored representation");
    persisted = JSON.stringify({ ...JSON.parse(nextInput.settings_json), rate: 1 });
    return persisted;
  }, { settingsBaseline: persisted });
  queue.enqueue({ input: input("server-a", '{"name":"B"}'), signature: "first" });
  await queue.whenIdle();
  queue.enqueue({ input: input("server-a", '{"name":"C"}'), signature: "second" });
  await queue.whenIdle();
  assert.equal(queue.isSaved("second"), true);
  assert.deepEqual(calls, ['{"name":"A","rate":1}', '{"name":"B","rate":1}']);
});

test("queued edits wait for the canonical settings returned by the preceding save", async () => {
  const saved = deferred();
  const calls = [];
  const { queue } = createQueue((nextInput, expectedSettingsJson) => {
    calls.push(expectedSettingsJson);
    return calls.length === 1 ? saved.promise : nextInput.settings_json;
  });
  queue.enqueue({ input: input("server-a", '{"name":"A"}'), signature: "first" });
  await settleQueue();
  queue.enqueue({ input: input("server-a", '{"name":"B"}'), signature: "second" });
  saved.resolve('{"name":"A","normalized":true}');
  await queue.whenIdle();
  assert.deepEqual(calls, ['{"roster":[]}', '{"name":"A","normalized":true}']);
});

test("flushLatest waits for the latest debounced settings to persist", async () => {
  const save = deferred();
  const calls = [];
  const { queue } = createQueue((nextInput) => { calls.push(nextInput); return save.promise; });
  const latest = { input: input("server-a", "latest"), signature: "latest" };
  queue.markDirty(latest.signature);
  assert.equal(typeof queue.flushLatest, "function", "starting needs an awaitable save barrier");
  let completed = false;
  const barrier = queue.flushLatest(latest).then(() => { completed = true; });
  await settleQueue();
  assert.deepEqual(calls, [latest.input]);
  assert.equal(completed, false);
  save.resolve();
  await barrier;
  assert.equal(queue.isSaved(latest.signature), true);
});

test("flushLatest serializes an active write and the newest draft with its CAS baseline", async () => {
  const first = deferred();
  const last = deferred();
  const calls = [];
  const { queue } = createQueue((nextInput, expectedSettingsJson) => {
    calls.push([nextInput.settings_json, expectedSettingsJson]);
    return calls.length === 1 ? first.promise : last.promise;
  });
  queue.enqueue({ input: input("server-a", "first"), signature: "first" });
  await settleQueue();
  assert.equal(typeof queue.flushLatest, "function");
  let completed = false;
  const barrier = queue.flushLatest({ input: input("server-a", "latest"), signature: "latest" })
    .then(() => { completed = true; });
  first.resolve();
  await settleQueue();
  assert.deepEqual(calls, [["first", '{"roster":[]}'], ["latest", "first"]]);
  assert.equal(completed, false);
  last.resolve();
  await barrier;
});

for (const message of ["disk is read-only", "precondition failed"]) {
  test(`flushLatest rejects ${message} and does not implicitly retry a failed draft`, async () => {
    let calls = 0;
    const { queue } = createQueue(() => { calls += 1; throw new Error(message); });
    const latest = { input: input("server-a", "latest"), signature: "latest" };
    assert.equal(typeof queue.flushLatest, "function");
    await assert.rejects(queue.flushLatest(latest), { message });
    await assert.rejects(queue.flushLatest(latest), { message });
    assert.equal(calls, 1);
  });
}

test("closing queue remains awaitable and propagates its final write failure", async () => {
  const save = deferred();
  const { queue } = createQueue(() => save.promise);
  queue.flushAndDispose({ input: input("server-a", "latest"), signature: "latest" });
  assert.equal(typeof queue.whenIdle, "function");
  const rejection = assert.rejects(queue.whenIdle(), /offline/);
  save.reject(new Error("offline"));
  await rejection;
  await assert.rejects(queue.whenIdle(), /offline/);
});

test("invalid draft rejection exposes a stable UI error code", async () => {
  const { queue } = createQueue(() => assert.fail("invalid settings must not persist"));
  await assert.rejects(queue.flushLatest(null), { code: "instance_settings_draft_invalid" });
});

test("reports dirty, saving, and saved around a successful request", async () => {
  const save = deferred();
  const { queue, statuses } = createQueue(() => save.promise);

  queue.enqueue({ input: input("server-a", '{"name":"A"}'), signature: "draft-a" });
  assert.deepEqual(statuses, [{ state: "dirty" }, { state: "saving" }]);

  save.resolve();
  await settleQueue();
  assert.deepEqual(statuses, [
    { state: "dirty" },
    { state: "saving" },
    { state: "saved" }
  ]);
  assert.equal(queue.isSaved("draft-a"), true);
});

test("marks a scheduled draft dirty before debounce enqueues it", () => {
  const { queue, statuses } = createQueue(() => undefined);

  queue.markDirty("draft-a");

  assert.deepEqual(statuses, [{ state: "dirty" }]);
  assert.equal(queue.isSaved("draft-a"), false);
});

test("does not report false success when the observed draft changes during a save", async () => {
  const first = deferred();
  const second = deferred();
  const { queue, statuses } = createQueue(() => (
    statuses.filter((status) => status.state === "saving").length === 1
      ? first.promise
      : second.promise
  ));

  queue.enqueue({ input: input("server-a", '{"name":"A"}'), signature: "draft-a" });
  await settleQueue();
  queue.markDirty("base");
  first.resolve();
  await settleQueue();
  assert.equal(statuses.at(-1).state, "dirty");

  queue.enqueue({ input: input("server-a", '{"roster":[]}'), signature: "base" });
  await settleQueue();
  second.resolve();
  await settleQueue();
  assert.equal(statuses.at(-1).state, "saved");
});

test("coalesces pending requests and uses the preceding successful draft as their CAS baseline", async () => {
  const first = deferred();
  const latest = deferred();
  const calls = [];
  const { queue } = createQueue((nextInput, expectedSettingsJson) => {
    calls.push({ nextInput, expectedSettingsJson });
    return calls.length === 1 ? first.promise : latest.promise;
  });

  queue.enqueue({ input: input("server-a", '{"name":"A"}'), signature: "draft-a" });
  await settleQueue();
  queue.enqueue({ input: input("server-a", '{"name":"B"}'), signature: "draft-b" });
  queue.enqueue({ input: input("server-a", '{"name":"C"}'), signature: "draft-c" });
  assert.equal(calls.length, 1, "later drafts must wait for the in-flight save");
  assert.equal(calls[0].expectedSettingsJson, '{"roster":[]}');

  first.resolve();
  await settleQueue();
  assert.equal(calls.length, 2);
  assert.equal(calls[1].nextInput.settings_json, '{"name":"C"}');
  assert.equal(calls[1].expectedSettingsJson, '{"name":"A"}');

  latest.resolve();
  await settleQueue();
  assert.equal(queue.isSaved("draft-c"), true);
  assert.equal(queue.isSaved("draft-b"), false);
});

test("reports a generic failure without advancing the saved signature", async () => {
  const save = deferred();
  const { queue, statuses } = createQueue(() => save.promise);

  queue.enqueue({ input: input("server-a", '{"name":"A"}'), signature: "draft-a" });
  await settleQueue();
  save.reject(new Error("disk is read-only"));
  await settleQueue();

  assert.deepEqual(statuses.at(-1), { state: "failed", message: "disk is read-only" });
  assert.equal(queue.isSaved("draft-a"), false);
  assert.equal(queue.isSaved("base"), true);
});

test("a new invalid draft cancels an older queued draft while the active save finishes", async () => {
  const active = deferred();
  const calls = [];
  const { queue, statuses } = createQueue((nextInput) => {
    calls.push(nextInput.settings_json);
    return active.promise;
  });
  queue.enqueue({ input: input("server-a", '{"name":"active"}'), signature: "active" });
  await settleQueue();
  queue.enqueue({ input: input("server-a", '{"name":"stale"}'), signature: "stale" });
  queue.markDirty("invalid");

  active.resolve();
  await settleQueue();

  assert.deepEqual(calls, ['{"name":"active"}']);
  assert.equal(statuses.at(-1).state, "dirty");
  assert.equal(queue.isSaved("invalid"), false);
});

test("reverting to the active draft cancels a queued intermediate edit", async () => {
  const active = deferred();
  const calls = [];
  const { queue, statuses } = createQueue((nextInput) => {
    calls.push(nextInput.settings_json);
    return active.promise;
  });
  queue.enqueue({ input: input("server-a", '{"name":"active"}'), signature: "active" });
  await settleQueue();
  queue.enqueue({ input: input("server-a", '{"name":"stale"}'), signature: "stale" });
  assert.equal(queue.markDirty("active"), false);

  active.resolve();
  await settleQueue();

  assert.deepEqual(calls, ['{"name":"active"}']);
  assert.equal(statuses.at(-1).state, "saved");
  assert.equal(queue.isSaved("active"), true);
});

test("observing the same failed draft preserves the failure until explicit retry", async () => {
  const calls = [];
  const { queue, statuses } = createQueue(() => {
    calls.push("save");
    return Promise.reject(new Error("disk is read-only"));
  });

  queue.enqueue({ input: input("server-a", '{"name":"A"}'), signature: "draft-a" });
  await settleQueue();
  const statusCount = statuses.length;

  assert.equal(queue.markDirty("draft-a"), false);
  await settleQueue();
  assert.equal(statuses.length, statusCount);
  assert.deepEqual(statuses.at(-1), { state: "failed", message: "disk is read-only" });
  assert.deepEqual(calls, ["save"]);
});

test("classifies compare-and-set precondition failures as conflicts", async () => {
  const save = deferred();
  const { queue, statuses } = createQueue(() => save.promise);

  queue.enqueue({ input: input("server-a", '{"name":"A"}'), signature: "draft-a" });
  await settleQueue();
  save.reject(
    new Error(
      "instance `server-a` settings changed while this edit was pending; reload the server settings and retry"
    )
  );
  await settleQueue();

  assert.equal(statuses.at(-1).state, "conflict");
  assert.match(statuses.at(-1).message, /settings changed while this edit was pending/i);
});

test("normalizes structured compare-and-set error codes", () => {
  const codedError = new Error("stale settings");
  codedError.code = "PreconditionFailed";
  assert.deepEqual(
    normalizeConfigurationSaveError({ code: "PreconditionFailed", message: "stale settings" }),
    { state: "conflict", message: "stale settings" }
  );
  assert.deepEqual(normalizeConfigurationSaveError(codedError), {
    state: "conflict",
    message: "stale settings"
  });
  assert.deepEqual(normalizeConfigurationSaveError("network unavailable"), {
    state: "failed",
    message: "network unavailable"
  });
});

test("retry replays the last failed request against the unchanged CAS baseline", async () => {
  const first = deferred();
  const second = deferred();
  const calls = [];
  const { queue, statuses } = createQueue((nextInput, expectedSettingsJson) => {
    calls.push({ nextInput, expectedSettingsJson });
    return calls.length === 1 ? first.promise : second.promise;
  }, { settingsBaseline: '{"roster":["player"]}' });

  queue.enqueue({ input: input("server-a", '{"name":"A"}'), signature: "draft-a" });
  await settleQueue();
  first.reject(new Error("precondition failed"));
  await settleQueue();
  assert.equal(statuses.at(-1).state, "conflict");

  queue.retry();
  await settleQueue();
  assert.deepEqual(calls.map((call) => call.expectedSettingsJson), [
    '{"roster":["player"]}',
    '{"roster":["player"]}'
  ]);

  second.resolve();
  await settleQueue();
  assert.equal(queue.isSaved("draft-a"), true);
  assert.equal(statuses.at(-1).state, "saved");
});

test("a newer request replaces a failed request and cannot retry stale settings", async () => {
  const first = deferred();
  const second = deferred();
  const calls = [];
  const { queue } = createQueue((nextInput) => {
    calls.push(nextInput.settings_json);
    return calls.length === 1 ? first.promise : second.promise;
  });

  queue.enqueue({ input: input("server-a", '{"name":"A"}'), signature: "draft-a" });
  await settleQueue();
  first.reject(new Error("disk is read-only"));
  await settleQueue();

  queue.enqueue({ input: input("server-a", '{"name":"B"}'), signature: "draft-b" });
  await settleQueue();
  second.resolve();
  await settleQueue();
  queue.retry();
  await settleQueue();

  assert.deepEqual(calls, ['{"name":"A"}', '{"name":"B"}']);
  assert.equal(queue.isSaved("draft-b"), true);
});

test("a newer pending request supersedes an in-flight failure without advancing the baseline", async () => {
  const first = deferred();
  const second = deferred();
  const calls = [];
  const { queue } = createQueue((_nextInput, expectedSettingsJson) => {
    calls.push(expectedSettingsJson);
    return calls.length === 1 ? first.promise : second.promise;
  }, { settingsBaseline: '{"roster":["player"]}' });

  queue.enqueue({ input: input("server-a", '{"name":"A"}'), signature: "draft-a" });
  await settleQueue();
  queue.enqueue({ input: input("server-a", '{"name":"B"}'), signature: "draft-b" });
  first.reject(new Error("write failed"));
  await settleQueue();

  assert.deepEqual(calls, ['{"roster":["player"]}', '{"roster":["player"]}']);
  second.resolve();
  await settleQueue();
  assert.equal(queue.isSaved("draft-b"), true);
});

test("reset clears failures and isolates an old in-flight save from another instance", async () => {
  const oldSave = deferred();
  const newSave = deferred();
  const calls = [];
  const { queue, statuses } = createQueue((nextInput, expectedSettingsJson) => {
    calls.push({ id: nextInput.id, expectedSettingsJson });
    return nextInput.id === "server-a" ? oldSave.promise : newSave.promise;
  }, { settingsBaseline: "a-base", savedSignature: "a-saved" });

  queue.enqueue({ input: input("server-a", "a-draft"), signature: "a-draft" });
  await settleQueue();
  queue.reset("server-b", "b-base", "b-saved");
  assert.equal(statuses.at(-1).state, "saved");
  queue.retry();
  queue.enqueue({ input: input("server-b", "b-draft"), signature: "b-draft" });
  await settleQueue();
  assert.deepEqual(calls, [
    { id: "server-a", expectedSettingsJson: "a-base" },
    { id: "server-b", expectedSettingsJson: "b-base" }
  ]);

  oldSave.resolve();
  newSave.resolve();
  await settleQueue();
  assert.equal(queue.isSaved("b-draft"), true);
  assert.equal(queue.isSaved("a-draft"), false);
});

test("dispose ignores a late request resolution and emits no later status", async () => {
  const save = deferred();
  const { queue, statuses } = createQueue(() => save.promise);

  queue.enqueue({ input: input("server-a", '{"name":"A"}'), signature: "draft-a" });
  await settleQueue();
  queue.dispose();
  const statusCountAtDispose = statuses.length;
  save.resolve();
  await settleQueue();

  assert.equal(statuses.length, statusCountAtDispose);
  assert.equal(queue.isSaved("draft-a"), false);
});

test("dispose cancels an executor that has not started", async () => {
  let calls = 0;
  const { queue } = createQueue(() => {
    calls += 1;
  });

  queue.enqueue({ input: input("server-a", '{"name":"A"}'), signature: "draft-a" });
  queue.dispose();
  await settleQueue();

  assert.equal(calls, 0);
  assert.equal(queue.isSaved("base"), false);
});

test("flushAndDispose immediately persists the latest request before its debounce fires", async () => {
  const calls = [];
  const { queue } = createQueue((nextInput, expectedSettingsJson) => {
    calls.push({ nextInput, expectedSettingsJson });
  });
  const latest = { input: input("server-a", '{"name":"latest"}'), signature: "draft-latest" };
  queue.markDirty(latest.signature);
  queue.flushAndDispose(latest);
  await settleQueue();
  assert.deepEqual(calls, [{
    nextInput: latest.input,
    expectedSettingsJson: '{"roster":[]}'
  }]);
});

test("flushAndDispose never persists a disabled or invalid draft without a valid request", async () => {
  let calls = 0;
  const { queue } = createQueue(() => {
    calls += 1;
  });
  queue.markDirty("invalid-draft");
  queue.flushAndDispose(null);
  await settleQueue();
  assert.equal(calls, 0);
});

test("flushAndDispose replaces stale pending work and drains the final request after an in-flight save", async () => {
  const first = deferred();
  const final = deferred();
  const calls = [];
  const { queue, statuses } = createQueue((nextInput, expectedSettingsJson) => {
    calls.push({ settingsJson: nextInput.settings_json, expectedSettingsJson });
    return calls.length === 1 ? first.promise : final.promise;
  });
  queue.enqueue({ input: input("server-a", '{"name":"active"}'), signature: "draft-active" });
  await settleQueue();
  queue.enqueue({ input: input("server-a", '{"name":"stale"}'), signature: "draft-stale" });
  const statusCountAtClose = statuses.length;
  queue.flushAndDispose({ input: input("server-a", '{"name":"final"}'), signature: "draft-final" });

  first.resolve();
  await settleQueue();
  assert.deepEqual(calls, [
    { settingsJson: '{"name":"active"}', expectedSettingsJson: '{"roster":[]}' },
    { settingsJson: '{"name":"final"}', expectedSettingsJson: '{"name":"active"}' }
  ]);
  final.resolve();
  await settleQueue();
  assert.equal(statuses.length, statusCountAtClose, "a closing queue must not update unmounted React state");
});

test("closing with an invalid draft drops queued work and only finishes the active save", async () => {
  const active = deferred();
  const calls = [];
  const { queue } = createQueue((nextInput) => {
    calls.push(nextInput.settings_json);
    return active.promise;
  });
  queue.enqueue({ input: input("server-a", '{"name":"active"}'), signature: "active" });
  await settleQueue();
  queue.enqueue({ input: input("server-a", '{"name":"stale"}'), signature: "stale" });
  queue.flushAndDispose(null);

  active.resolve();
  await settleQueue();

  assert.deepEqual(calls, ['{"name":"active"}']);
});

test("flushAndDispose retries the retained failed final request against the unchanged CAS baseline", async () => {
  const calls = [];
  const { queue } = createQueue((_nextInput, expectedSettingsJson) => {
    calls.push(expectedSettingsJson);
    if (calls.length === 1) {
      return Promise.reject(new Error("precondition failed"));
    }
  }, { settingsBaseline: '{"server":"baseline"}' });
  const final = { input: input("server-a", '{"name":"final"}'), signature: "draft-final" };
  queue.enqueue(final);
  await settleQueue();
  queue.flushAndDispose(final);
  await settleQueue();

  assert.deepEqual(calls, ['{"server":"baseline"}', '{"server":"baseline"}']);
});

test("flushAndDispose lets an identical in-flight final request finish without duplicating it", async () => {
  const save = deferred();
  let calls = 0;
  const { queue, statuses } = createQueue(() => {
    calls += 1;
    return save.promise;
  });
  const final = { input: input("server-a", '{"name":"final"}'), signature: "draft-final" };
  queue.enqueue(final);
  await settleQueue();
  const statusCountAtClose = statuses.length;
  queue.flushAndDispose(final);
  save.resolve();
  await settleQueue();

  assert.equal(calls, 1);
  assert.equal(statuses.length, statusCountAtClose);
});

test("a failed in-flight final request settles a closing queue without retry loops", async () => {
  const save = deferred();
  let calls = 0;
  const { queue, statuses } = createQueue(() => {
    calls += 1;
    return save.promise;
  });
  const final = { input: input("server-a", '{"name":"final"}'), signature: "draft-final" };
  queue.enqueue(final);
  await settleQueue();
  const statusCountAtClose = statuses.length;
  queue.flushAndDispose(final);
  save.reject(new Error("offline"));
  await settleQueue();
  queue.retry();
  await settleQueue();

  assert.equal(calls, 1);
  assert.equal(statuses.length, statusCountAtClose);
});

test("reset cancels a StrictMode-style simulated close before execution and revives the queue", async () => {
  const calls = [];
  const { queue } = createQueue((nextInput) => {
    calls.push(nextInput.id);
  });
  queue.flushAndDispose({ input: input("server-a", "a-draft"), signature: "a-draft" });
  queue.reset("server-a", "a-base", "a-base-signature");
  queue.enqueue({ input: input("server-a", "a-next"), signature: "a-next" });
  await settleQueue();

  assert.deepEqual(calls, ["server-a"]);
  assert.equal(queue.isSaved("a-next"), true);
});

test("autosave hook exposes status and retry while marking drafts before debounce", () => {
  const source = fs.readFileSync(path.join(
    desktopRoot,
    "src",
    "views",
    "settings",
    "useAutoSaveInstanceSettings.ts"
  ), "utf8");
  const markDirtyIndex = source.indexOf("queue?.markDirty(signature)");
  const debounceIndex = source.indexOf("window.setTimeout");

  assert.match(source, /UseAutoSaveInstanceSettingsResult[\s\S]*status:\s*InstanceSettingsSaveStatus;[\s\S]*retry\(\): void/);
  assert.ok(markDirtyIndex >= 0 && markDirtyIndex < debounceIndex);
  assert.match(source, /if \(options\.disabled \|\| !shouldEnqueue\)/);
  assert.match(source, /queue\?\.reset\([\s\S]*?queue\?\.flushAndDispose\([\s\S]*?options\.details\.summary\.id/);
  assert.match(source, /latestValidRequest\s*=\s*options\.disabled\s*\?\s*null\s*:\s*\{\s*input,\s*signature\s*\}/);
  assert.match(source, /latestValidRequestRef\.current\.set\(input\.id,\s*latestValidRequest\)/);
  assert.match(source, /saveQueueInstanceIdRef\.current\s*!==\s*input\.id/);
  assert.doesNotMatch(
    source.slice(debounceIndex),
    /return \(\) => \{[^}]*flushAndDispose/,
    "ordinary debounce dependency cleanup must not flush a draft"
  );
  assert.match(source, /saveQueueRef\.current\?\.retry\(\)/);
  assert.match(source, /return \{ status, retry \};/);
});

test("a missing persistence acknowledgement fails without marking the draft as saved", async () => {
  const statuses = [];
  const baselines = [];
  const queue = new InstanceSettingsSaveQueue({
    instanceId: "server-a",
    settingsBaseline: '{"name":"stored"}',
    savedSignature: "stored",
    execute: async (_nextInput, expectedSettingsJson) => {
      baselines.push(expectedSettingsJson);
      return undefined;
    },
    onStatusChange: (status) => statuses.push(status)
  });
  queue.enqueue({ input: input("server-a", '{"name":"draft"}'), signature: "draft" });
  await assert.rejects(queue.whenIdle(), /Saved settings were not returned by the server/);
  assert.equal(statuses.at(-1).state, "failed");
  assert.equal(queue.isSaved("draft"), false);
  queue.retry();
  await assert.rejects(queue.whenIdle(), /Saved settings were not returned by the server/);
  assert.deepEqual(baselines, ['{"name":"stored"}', '{"name":"stored"}'], "a missing ACK cannot advance the CAS baseline");
  assert.equal(queue.isSaved("draft"), false);
});
