const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const desktopRoot = path.resolve(__dirname, "..");

require.extensions[".ts"] = function compileTypeScript(module, filename) {
  const source = fs.readFileSync(filename, "utf8");
  module._compile(transpileTypeScript(source, filename), filename);
};

const {
  BroadcastPolicySaveQueue,
  setupBroadcastPolicySaveQueue
} = require(path.join(
  desktopRoot,
  "src",
  "views",
  "servers",
  "broadcast-policy-save-queue.ts"
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

function settleQueue() {
  return new Promise((resolve) => setImmediate(resolve));
}

function policy(marker, instanceId = "server-a") {
  return {
    instance_id: instanceId,
    enabled: marker !== "disabled",
    rules: {
      startup: { enabled: false, prompt: marker },
      shutdown: { enabled: false, prompt: null },
      runtime_health: { enabled: false, prompt: null },
      periodic: { enabled: false, interval_minutes: 30, prompt: null },
      tone: marker,
      cooldown_minutes: 10
    },
    updated_at_unix_ms: 0
  };
}

function createQueue(execute) {
  const saved = [];
  const saving = [];
  const errors = [];
  const queue = new BroadcastPolicySaveQueue({
    instanceId: "server-a",
    execute,
    onSaved: (nextPolicy) => saved.push(nextPolicy),
    onSavingChange: (value) => saving.push(value),
    onError: (error) => errors.push(error)
  });
  return { queue, saved, saving, errors };
}

test("serializes policy writes and coalesces queued edits to the latest value", async () => {
  const first = deferred();
  const latest = deferred();
  const calls = [];
  const { queue, saved, saving, errors } = createQueue((nextPolicy) => {
    calls.push(nextPolicy.rules.tone);
    return calls.length === 1 ? first.promise : latest.promise;
  });

  queue.enqueue(policy("first"));
  await Promise.resolve();
  queue.enqueue(policy("middle"));
  queue.enqueue(policy("latest"));
  assert.deepEqual(calls, ["first"]);

  first.resolve(policy("first-saved"));
  await settleQueue();
  assert.deepEqual(calls, ["first", "latest"]);
  assert.deepEqual(saved, [], "a superseded response must not replace the optimistic latest draft");

  latest.resolve(policy("latest"));
  await settleQueue();
  assert.deepEqual(saved.map((entry) => entry.rules.tone), ["latest"]);
  assert.deepEqual(saving, [true, false]);
  assert.deepEqual(errors, []);
});

test("a superseded failure does not block or report over a newer policy", async () => {
  const first = deferred();
  const latest = deferred();
  const calls = [];
  const { queue, saved, errors } = createQueue((nextPolicy) => {
    calls.push(nextPolicy.rules.tone);
    return calls.length === 1 ? first.promise : latest.promise;
  });

  queue.enqueue(policy("first"));
  await Promise.resolve();
  queue.enqueue(policy("latest"));
  first.reject(new Error("old write failed"));
  await settleQueue();

  assert.deepEqual(calls, ["first", "latest"]);
  assert.deepEqual(errors, []);
  latest.resolve(policy("latest"));
  await settleQueue();
  assert.deepEqual(saved.map((entry) => entry.rules.tone), ["latest"]);
});

test("dispose invalidates an in-flight response and drops queued work", async () => {
  const first = deferred();
  const calls = [];
  const { queue, saved, errors } = createQueue((nextPolicy) => {
    calls.push(nextPolicy.rules.tone);
    return first.promise;
  });

  queue.enqueue(policy("first"));
  await Promise.resolve();
  queue.enqueue(policy("never-run"));
  queue.dispose();
  first.resolve(policy("late"));
  await settleQueue();

  assert.deepEqual(calls, ["first"]);
  assert.deepEqual(saved, []);
  assert.deepEqual(errors, []);
});

test("a detached setup silently drains the latest queued policy", async () => {
  const first = deferred();
  const latest = deferred();
  const calls = [];
  const saved = [];
  const saving = [];
  const errors = [];
  const slot = { current: null };
  const cleanup = setupBroadcastPolicySaveQueue(slot, {
    instanceId: "server-a",
    execute: (nextPolicy) => {
      calls.push(nextPolicy.rules.tone);
      return calls.length === 1 ? first.promise : latest.promise;
    },
    onSaved: (nextPolicy) => saved.push(nextPolicy),
    onSavingChange: (value) => saving.push(value),
    onError: (error) => errors.push(error)
  });
  const queue = slot.current;

  queue.enqueue(policy("first"));
  await Promise.resolve();
  queue.enqueue(policy("latest"));
  cleanup();
  first.resolve(policy("first"));
  await settleQueue();
  assert.deepEqual(calls, ["first", "latest"]);

  latest.resolve(policy("latest"));
  await settleQueue();
  assert.deepEqual(saved, []);
  assert.deepEqual(errors, []);
  assert.deepEqual(saving, [true], "closing must not update an unmounted component");
});

test("StrictMode and remount reuse one active per-instance queue so the newest write wins", async () => {
  const first = deferred();
  const newest = deferred();
  const calls = [];
  const saved = [];
  const slot = { current: null };
  const options = {
    instanceId: "server-a",
    execute: (nextPolicy) => {
      calls.push(nextPolicy.rules.tone);
      return calls.length === 1 ? first.promise : newest.promise;
    },
    onSaved: (nextPolicy) => saved.push(nextPolicy.rules.tone),
    onSavingChange: () => {},
    onError: (error) => assert.fail(error)
  };

  const cleanupFirstSetup = setupBroadcastPolicySaveQueue(slot, options);
  const firstQueue = slot.current;
  firstQueue.enqueue(policy("first"));
  await Promise.resolve();
  firstQueue.enqueue(policy("stale-pending"));
  cleanupFirstSetup();
  assert.equal(slot.current, null);

  const cleanupSecondSetup = setupBroadcastPolicySaveQueue(slot, options);
  const secondQueue = slot.current;
  assert.equal(secondQueue, firstQueue, "an in-flight instance queue must survive a remount");
  secondQueue.enqueue(policy("newest"));
  await Promise.resolve();
  assert.deepEqual(calls, ["first"]);

  first.resolve(policy("stale-first"));
  await settleQueue();
  assert.deepEqual(calls, ["first", "newest"]);
  newest.resolve(policy("newest"));
  await settleQueue();
  assert.deepEqual(saved, ["newest"], "an older detached draft must not overwrite a remounted edit");

  cleanupSecondSetup();
  assert.equal(slot.current, null);

  const cleanupThirdSetup = setupBroadcastPolicySaveQueue(slot, options);
  assert.notEqual(slot.current, firstQueue, "an idle detached queue must be released");
  cleanupThirdSetup();
});

test("a remounted load waits for an inherited save before reading policy", async () => {
  const write = deferred();
  const slot = { current: null };
  let storedPolicy = policy("stored-old");
  const options = {
    instanceId: "server-a",
    execute: async (nextPolicy) => {
      await write.promise;
      storedPolicy = nextPolicy;
      return nextPolicy;
    },
    onSaved: () => {},
    onSavingChange: () => {},
    onError: (error) => assert.fail(error)
  };

  const cleanupFirstSetup = setupBroadcastPolicySaveQueue(slot, options);
  slot.current.enqueue(policy("saved-during-remount"));
  await Promise.resolve();
  cleanupFirstSetup();

  const cleanupSecondSetup = setupBroadcastPolicySaveQueue(slot, options);
  let readCompleted = false;
  const loadedPolicy = slot.current.waitUntilIdle().then(() => {
    readCompleted = true;
    return storedPolicy;
  });
  await settleQueue();
  assert.equal(readCompleted, false, "the load must not race an inherited write");

  write.resolve();
  assert.equal((await loadedPolicy).rules.tone, "saved-during-remount");
  cleanupSecondSetup();
});

test("the workbench routes policy changes through the serialized queue", () => {
  const source = fs.readFileSync(path.join(
    desktopRoot,
    "src",
    "views",
    "servers",
    "AiBroadcastWorkbench.tsx"
  ), "utf8");

  assert.match(source, /useRef<BroadcastPolicySaveQueue \| null>\(null\)/);
  assert.match(source, /setupBroadcastPolicySaveQueue\(policySaveQueueRef,/);
  assert.match(source, /policySaveQueueRef\.current\?\.enqueue\(normalized\)/);
  assert.match(source, /saveQueue\?\.waitUntilIdle\(\)/);
  assert.doesNotMatch(source, /const policySaveQueue = useMemo/);
  assert.doesNotMatch(source, /await updateInstanceBroadcastPolicy\(/);
  assert.match(source, /const instanceScope = useMemo\(\(\) => \(\{ instanceId \}\), \[instanceId\]\)/);
  assert.match(source, /policy\?\.instance_id === instanceId \? policy : null/);
  assert.match(source, /if \(!instanceStateReady\) \{\s*return;\s*\}/);
  assert.match(source, /currentInstanceScopeRef\.current !== operationScope/);
});
