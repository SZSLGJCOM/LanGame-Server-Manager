const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
require.extensions[".ts"] = (module, filename) => module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
const { WorkshopDownloadOperations } = require("../src/views/servers/workshop-download-operations.ts");
const { buildSteamWorkshopEnablementPlan } = require("../src/views/servers/steam-workshop-enablement-plan.ts");
const { loadWorkshopDownloadExecution } = require("./helpers/workshop-download-execution.cjs");
const deferred = () => Promise.withResolvers();
const tick = () => new Promise(setImmediate);
const request = (id) => ({ ids: [id], contentIds: [id], items: {}, locale: "en-US" });
const accountError = JSON.stringify({ code: "steam_workshop_account_required", message: "Account required", output_excerpt: "", consumer_app_id: 123 });

function fixture(overrides = {}) {
  const calls = { prepare: [], commit: [], discard: [], retain: [], download: [] };
  const timers = new Map();
  const results = new Map();
  let timerId = 0;
  const api = {
    prepareSteamWorkshopItems: async (instanceId, ids, attemptToken) => {
      calls.prepare.push({ instanceId, ids, attemptToken });
      results.set(attemptToken, { items: ids.map((item_id) => ({ item_id, expected_path_exists: true })) });
      return { ticket_id: attemptToken, consumer_app_id: 123 };
    },
    commitSteamWorkshopItems: async (instanceId, ticketId) => { calls.commit.push({ instanceId, ticketId }); return results.get(ticketId); },
    discardSteamWorkshopPreparation: async (instanceId, ticketId) => { calls.discard.push({ instanceId, ticketId }); },
    retainSteamWorkshopPreparation: async (instanceId, ticketId) => { calls.retain.push({ instanceId, ticketId }); },
    downloadSteamWorkshopItems: async (instanceId, ids, missingOnly, accountName, attemptToken) => {
      calls.download.push({ instanceId, ids, missingOnly, accountName, attemptToken });
      return { items: ids.map((item_id) => ({ item_id, expected_path_exists: true })) };
    },
    ...overrides
  };
  const execute = loadWorkshopDownloadExecution(api, {
    setInterval: (callback, interval) => { assert.equal(interval, 60_000); timers.set(++timerId, callback); return timerId; },
    clearInterval: (id) => timers.delete(id)
  });
  const store = new WorkshopDownloadOperations();
  return { calls, timers, store, api,
    renew: () => { for (const callback of timers.values()) callback(); },
    enqueue: (id, apply, options = {}) => store.enqueue("server", request(id), (operation) => execute(operation,
      { instanceId: "server", ids: [id], ...options }, apply)) };
}

test("preparation cannot contaminate another request's Palworld before/after inventory delta", async () => {
  const gates = [deferred(), deferred()], save = deferred(), inventory = [], plans = [], commits = [];
  const f = fixture({
    prepareSteamWorkshopItems: async (_instance, ids) => {
      await gates[Number(ids[0]) - 1].promise;
      return { ticket_id: ids[0], consumer_app_id: 123 };
    },
    commitSteamWorkshopItems: async (_instance, ticket) => {
      commits.push(ticket);
      inventory.push({ name: `Package${ticket === "1" ? "A" : "B"}`, inferred_id: `Package${ticket === "1" ? "A" : "B"}`,
        path: `Mods/${ticket === "1" ? "First" : "Second"}`, item_type: "directory" });
      return { items: [{ item_id: ticket, expected_path_exists: true }] };
    }
  });
  const run = (id) => f.enqueue(id, async (download) => {
    const before = [...inventory];
    await download();
    plans.push(buildSteamWorkshopEnablementPlan({ downloadedWorkshopItemIds: [id], inventoryBefore: before, inventoryAfter: [...inventory] }));
    if (id === "1") await save.promise;
  });
  const first = run("1"), second = run("2");
  gates[1].resolve(); await tick();
  assert.deepEqual(commits, []);
  gates[0].resolve(); await tick();
  assert.deepEqual(commits, ["1"]);
  assert.deepEqual(plans, [{ inferredIds: ["PackageA"], unresolvedWorkshopItemIds: [] }]);
  save.resolve(); await Promise.all([first, second]);
  assert.deepEqual(commits, ["1", "2"]);
  assert.deepEqual(plans[1], { inferredIds: ["PackageB"], unresolvedWorkshopItemIds: [] });
  assert.equal(f.timers.size, 0);
});

test("explicit account work waits for its FIFO commit slot and retains the original IDs", async () => {
  const f = fixture(), gate = deferred();
  const previous = f.store.enqueue("server", request("1"), (operation) => operation.apply(() => gate.promise));
  const completion = f.enqueue("2", async (download) => { await download(); }, { loginAccount: "Fixture_Account", ids: ["2", "3"] });
  await tick();
  assert.equal(f.calls.prepare.length, 0);
  assert.equal(f.calls.download.length, 0, "Account SteamCMD must not run while another request is committing");
  assert.equal(f.store.snapshot("server").requests[1].status, "ready");
  gate.resolve(); await Promise.all([previous, completion]);
  assert.equal(f.calls.download.length, 1);
  assert.deepEqual(f.calls.download[0].ids, ["2", "3"]);
  assert.equal(f.calls.download[0].missingOnly, true);
  assert.equal(f.calls.download[0].accountName, "Fixture_Account");
  assert.match(f.calls.download[0].attemptToken, /^[0-9a-f-]{36}$/);
  assert.equal(f.timers.size, 0);
});

test("anonymous account-required failures defer the prompt until commit and use a fresh correlated attempt", async () => {
  const tokens = [], gate = deferred();
  const f = fixture({ prepareSteamWorkshopItems: async (_instance, _ids, token) => { tokens.push(token); throw new Error(accountError); } });
  let prompts = 0;
  const previous = f.store.enqueue("server", request("1"), (operation) => operation.apply(() => gate.promise));
  const completion = f.enqueue("2", async (download) => { await download(); }, {
    onAccountRequired: async (retry) => { prompts++; return retry("Fixture_Account"); }
  });
  await tick(); assert.equal(prompts, 0); assert.equal(tokens.length, 1);
  gate.resolve(); await Promise.all([previous, completion]);
  assert.equal(prompts, 1);
  assert.equal(f.calls.download[0].accountName, "Fixture_Account");
  assert.notEqual(tokens[0], f.calls.download[0].attemptToken);
  assert.equal(f.calls.commit.length, 0);
});

test("ordinary preparation failures never fallback to an account download or apply settings", async () => {
  const f = fixture({ prepareSteamWorkshopItems: async () => { throw new Error("CDN unavailable"); } });
  await assert.rejects(f.enqueue("1", async () => { assert.fail("Failed preparation reached commit"); }, {
    onAccountRequired: async () => { assert.fail("Unverified error opened account login"); }
  }), /CDN unavailable/);
  assert.equal(f.calls.download.length, 0); assert.equal(f.calls.discard.length, 0); assert.equal(f.timers.size, 0);
});

for (const phase of ["validation", "commit", "settings"]) {
  test(`${phase} failure clears its keepalive and discards only an unconsumed preparation`, async () => {
    const f = fixture(phase === "commit" ? { commitSteamWorkshopItems: async () => { throw new Error("commit failed"); } } : {});
    await assert.rejects(f.enqueue("1", async (download) => {
      if (phase === "validation") throw new Error("validation failed");
      await download();
      if (phase === "settings") throw new Error("settings failed");
    }), new RegExp(`${phase} failed`));
    assert.equal(f.calls.discard.length, phase === "settings" ? 0 : 1);
    assert.equal(f.timers.size, 0);
  });
}

test("cleanup failure retains the deployment diagnosis and still stops ticket renewal", async () => {
  const f = fixture({ discardSteamWorkshopPreparation: async () => { throw new Error("cleanup offline"); } });
  await assert.rejects(f.enqueue("1", async () => { throw new Error("instance started"); }), /instance started.*cleanup offline/);
  assert.equal(f.timers.size, 0);
});

test("ready tickets renew independently of mounted subscribers with one request in flight and transient retry", async () => {
  const gate = deferred(), renewal = deferred();
  let renewals = 0;
  const f = fixture({ retainSteamWorkshopPreparation: async (instanceId, ticketId) => {
    assert.equal(instanceId, "server");
    assert.equal(ticketId, f.calls.prepare[0].attemptToken);
    if (++renewals === 1) await renewal.promise;
  } });
  const unsubscribe = f.store.subscribe(() => {});
  const previous = f.store.enqueue("server", request("1"), (operation) => operation.apply(() => gate.promise));
  const completion = f.enqueue("2", async (download) => { await download(); });
  await tick(); unsubscribe();
  assert.equal(f.store.snapshot("server").requests[1].status, "ready");
  assert.equal(f.timers.size, 1);
  f.renew(); f.renew(); assert.equal(renewals, 1, "Pending renewal cannot overlap the next interval");
  renewal.reject(new Error("temporary transport failure")); await tick();
  f.renew(); await tick(); assert.equal(renewals, 2);
  gate.resolve(); await Promise.all([previous, completion]);
  assert.equal(f.calls.commit.length, 1, "A transient lease transport failure cannot invent commit failure");
  assert.equal(f.timers.size, 0); f.renew(); assert.equal(renewals, 2);
});

test("an expired native ticket rejects the request and never saves configuration", async () => {
  const f = fixture({ commitSteamWorkshopItems: async () => { throw new Error("Workshop preparation lease expired"); } });
  let saved = false;
  await assert.rejects(f.enqueue("1", async (download) => { await download(); saved = true; }), /lease expired/);
  assert.equal(saved, false);
  assert.equal(f.store.snapshot("server").requests[0].status, "error");
  assert.equal(f.calls.discard.length, 1); assert.equal(f.timers.size, 0);
});
