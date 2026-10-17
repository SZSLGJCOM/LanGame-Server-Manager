const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
require.extensions[".ts"] = (module, filename) => module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
const { requiresSteamAppAccount, runSteamInstallationWithAccount } = require("../src/steam-installation-account.ts");
const access = { code: "steam_app_account_required", message: "Access is required", app_id: 12345, output_excerpt: "No subscription" };
const target = { kind: "module", id: "fixture-game" };

test("only an explicit server-app access contract can request account recovery", () => {
  for (const value of [access, JSON.stringify(access), new Error(JSON.stringify(access))]) assert.equal(requiresSteamAppAccount(value, access.app_id), true);
  for (const app_id of [null, "12345", 0, -1, 1.5, Infinity, Number.MAX_SAFE_INTEGER + 1]) assert.equal(requiresSteamAppAccount({ ...access, app_id }, access.app_id), false);
  for (const key of ["message", "output_excerpt"]) for (const value of [null, undefined, 4, []]) assert.equal(requiresSteamAppAccount({ ...access, [key]: value }, access.app_id), false);
  for (const reason of ["No subscription", "Failure", "AccessDenied", { ...access, code: "steamcmd_command_failed" }, { ...access, code: "steam_workshop_account_required" }]) assert.equal(requiresSteamAppAccount(reason, access.app_id), false);
  assert.equal(requiresSteamAppAccount(access, access.app_id + 1), false);
  for (const expected of [undefined, null, 0, -1, NaN]) assert.equal(requiresSteamAppAccount(access, expected), false);
});

test("anonymous installation resumes once with its authenticated account and exact original target", async () => {
  const events = [];
  const result = await runSteamInstallationWithAccount(async () => { events.push("anonymous"); throw new Error(JSON.stringify(access)); },
    async (account, received) => { events.push(account); assert.equal(received, target); return "installed"; },
    { target, expectedAppId: access.app_id, isCurrentOwner: () => true, requestAccount: async () => { events.push("login"); return "Confirmed_Account"; } });
  assert.equal(result, "installed"); assert.deepEqual(events, ["anonymous", "login", "Confirmed_Account"]);
});

test("an authenticated access rejection never opens another login or falls back to anonymous", async () => {
  let calls = 0;
  await assert.rejects(runSteamInstallationWithAccount(() => assert.fail("no anonymous fallback"), async () => { calls++; throw access; },
    { target, expectedAppId: access.app_id, loginAccount: "Confirmed_Account", isCurrentOwner: () => true, requestAccount: () => assert.fail("no authentication loop") }), (reason) => reason === access);
  assert.equal(calls, 1);
});

test("generic failure and network errors do not request login", async () => {
  for (const reason of [new Error("Failure"), new Error("connection reset"), { ...access, code: "steamcmd_command_failed" }, { ...access, app_id: access.app_id + 1 }]) {
    await assert.rejects(runSteamInstallationWithAccount(async () => { throw reason; }, () => assert.fail("no retry"),
      { target, expectedAppId: access.app_id, isCurrentOwner: () => true, requestAccount: () => assert.fail("no login") }), (error) => error === reason);
  }
});

test("cancellation and changed ownership cannot resume an old installation", async () => {
  let owned = true;
  await assert.rejects(runSteamInstallationWithAccount(async () => { throw access; }, () => assert.fail("stale target cannot retry"),
    { target, expectedAppId: access.app_id, isCurrentOwner: () => owned, requestAccount: async () => { owned = false; return "Confirmed_Account"; } }), /installation_cancelled/);
  await assert.rejects(runSteamInstallationWithAccount(() => assert.fail("stale target cannot start"), () => assert.fail("no retry"),
    { target, expectedAppId: access.app_id, isCurrentOwner: () => false, requestAccount: () => assert.fail("no login") }), /installation_cancelled/);
  const cancelled = new Error("installation_cancelled");
  await assert.rejects(runSteamInstallationWithAccount(async () => { throw access; }, () => assert.fail("cancelled login cannot retry"),
    { target, expectedAppId: access.app_id, isCurrentOwner: () => true, requestAccount: async () => { throw cancelled; } }), (reason) => reason === cancelled);
});
