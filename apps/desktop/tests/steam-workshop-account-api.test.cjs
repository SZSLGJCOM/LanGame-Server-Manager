const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

function snapshot(fields = {}) {
  return { target: { kind: "instance", id: "instance-a" }, session_id: "session-a", revision: 1, accepted_input_revision: null,
    phase: "ready", input_mode: "command", events: ["ready"], account_name: null, steamcmd_root: null, error: null, ...fields };
}
function load(invoke, lan = false, native = false) {
  const filename = path.resolve(__dirname, "../src/steam-workshop-account-api.ts");
  const module = { exports: {} };
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), {
    module, exports: module.exports, Error,
    require(name) {
      assert.equal(name, "./api-transport");
      return { invokeOrMock: invoke, canUseLocalDesktopApi: () => native || !lan };
    }
  });
  return module.exports;
}

test("login input is delivered only through its bound local credential command", async () => {
  const calls = [];
  const api = load(async (command, args) => {
    calls.push({ command, args });
    return snapshot({ revision: 2, accepted_input_revision: 1, phase: "connecting", input_mode: "disabled" });
  });
  const result = await api.sendSteamWorkshopAccountInput({ kind: "instance", id: "instance-a" }, "session-a", 1, "synthetic-private-input");
  assert.equal(result.accepted_input_revision, 1);
  assert.equal(calls.length, 1);
  assert.equal(calls[0].command, "send_steam_workshop_account_input");
  assert.equal(calls[0].args.input, "synthetic-private-input");
  assert.equal(calls[0].args.expectedRevision, 1);
  assert.equal(calls[0].args.target.kind, "instance");
  assert.equal(calls[0].args.target.id, "instance-a");
  assert.equal(calls[0].args.sessionId, "session-a");
});

test("known input rejection preserves only its allowlisted code and never its raw message", async () => {
  for (const code of ["steam_account_command_invalid", "steam_account_name_invalid", "steam_account_prompt_changed",
    "steam_account_input_invalid", "steam_account_input_pending", "steam_account_input_closed", "steam_account_session_unavailable"]) {
    const api = load(async (command) => {
      if (command === "read_steam_workshop_account_login") return snapshot();
      throw JSON.stringify({ code, message: "synthetic-sensitive-error-input", input: "synthetic-private-input" });
    });
    await assert.rejects(api.sendSteamWorkshopAccountInput({ kind: "instance", id: "instance-a" }, "session-a", 1, "synthetic-private-input"), (error) => {
      assert.ok(error instanceof api.SteamWorkshopAccountInputError);
      assert.equal(error.code, code);
      assert.equal(error.message, code);
      assert.doesNotMatch(String(error), /sensitive|private/);
      return true;
    });
  }
});

test("a lost reply is recovered by reading the accepted prompt receipt without resending", async () => {
  const commands = [];
  const api = load(async (command) => {
    commands.push(command);
    if (command === "send_steam_workshop_account_input") throw new Error("synthetic-private-input");
    return snapshot({ revision: 4, accepted_input_revision: 1, phase: "password", input_mode: "secret" });
  });
  const recovered = await api.sendSteamWorkshopAccountInput({ kind: "instance", id: "instance-a" }, "session-a", 1, "synthetic-private-input");
  assert.equal(recovered.accepted_input_revision, 1);
  assert.deepEqual(commands, ["send_steam_workshop_account_input", "read_steam_workshop_account_login"]);
});

test("unknown input failures never reflect private text or automatically retry the secret", async () => {
  const commands = [];
  const api = load(async (command) => {
    commands.push(command);
    throw new Error("synthetic-sensitive-input-and-password");
  });
  await assert.rejects(api.sendSteamWorkshopAccountInput({ kind: "instance", id: "instance-a" }, "session-a", 1, "synthetic-password"), (error) => {
    assert.equal(error.code, "steam_account_input_unconfirmed");
    assert.doesNotMatch(String(error), /synthetic|password|sensitive/);
    return true;
  });
  assert.deepEqual(commands, ["send_steam_workshop_account_input", "read_steam_workshop_account_login"]);
});

test("a malformed or stale backend snapshot produces a safe protocol diagnosis", async () => {
  for (const fields of [{ accepted_input_revision: undefined }, { accepted_input_revision: 1 },
    { target: { kind: "instance", id: "another-instance" } }, { target: { kind: "module", id: "instance-a" } },
    { target: null }, { instance_id: "instance-a" }, { session_id: "another-session" }, { revision: 0 }, { input_mode: "invalid" }]) {
    const api = load(async () => snapshot(fields));
    await assert.rejects(api.sendSteamWorkshopAccountInput({ kind: "instance", id: "instance-a" }, "session-a", 1, "synthetic-password"), (error) => {
      assert.equal(error.code, "steam_account_backend_out_of_date");
      assert.doesNotMatch(String(error), /synthetic|password/);
      return true;
    });
  }
});

test("every login phase accepts only its credential-safe input mode", async () => {
  const modes = { ready: "command", queued: "disabled", input_submitted: "disabled", preparing_steamcmd: "disabled",
    connecting: "disabled", authenticating: "disabled", verifying_cache: "disabled", guard_code: "secret", email_code: "secret", password: "secret",
    steam_guard: "secret", mobile_confirmation: "disabled", authenticated: "disabled", failed: "disabled", cancelled: "disabled" };
  for (const [phase, expectedMode] of Object.entries(modes)) for (const mode of ["command", "secret", "disabled"]) {
    const api = load(async () => snapshot({ phase, input_mode: mode, events: ["ready", phase],
      ...(phase === "authenticated" ? { account_name: "Fixture_Account", steamcmd_root: "fixture" } : {}) }));
    const request = api.startSteamWorkshopAccountLogin({ kind: "instance", id: "instance-a" }, "session-a");
    if (mode === expectedMode) assert.equal((await request).input_mode, expectedMode);
    else await assert.rejects(request, (error) => error.code === "steam_account_backend_out_of_date");
  }
});

test("actual token and email challenges recover an accepted receipt without resending input", async () => {
  for (const [phase, input] of [["guard_code", "A1B2C"], ["email_code", "M4I7L"]]) {
    const calls = [];
    const api = load(async (command, args) => {
      calls.push({ command, args });
      if (command === "send_steam_workshop_account_input") throw new Error("Synthetic response lost after acceptance");
      return snapshot({ revision: 4, accepted_input_revision: 2, phase: "authenticating", input_mode: "disabled", events: ["ready", "password", phase, "authenticating"] });
    });
    const result = await api.sendSteamWorkshopAccountInput({ kind: "instance", id: "instance-a" }, "session-a", 2, input);
    assert.equal(result.accepted_input_revision, 2);
    assert.deepEqual(calls.map((call) => call.command), ["send_steam_workshop_account_input", "read_steam_workshop_account_login"]);
    assert.equal(calls[0].args.input, input);
    assert.equal(calls[0].args.expectedRevision, 2);
  }
});

test("empty credential input is rejected before delivery or receipt polling", async () => {
  let calls = 0;
  const api = load(async () => { calls++; return snapshot(); });
  await assert.rejects(api.sendSteamWorkshopAccountInput({ kind: "instance", id: "instance-a" }, "session-a", 2, ""),
    (error) => error.code === "steam_account_input_invalid");
  assert.equal(calls, 0);
});

test("accepted password and token receipts remain read-only throughout native verification", async () => {
  for (const phase of ["input_submitted", "preparing_steamcmd", "connecting", "authenticating", "verifying_cache"]) {
    const calls = [];
    const api = load(async (command) => {
      calls.push(command);
      if (command === "send_steam_workshop_account_input") throw new Error("Synthetic accepted response lost");
      return snapshot({ revision: 8, accepted_input_revision: 5, phase, input_mode: "disabled",
        events: ["ready", "password", "guard_code", phase] });
    });
    const result = await api.sendSteamWorkshopAccountInput({ kind: "instance", id: "instance-a" }, "session-a", 5, "A1B2C");
    assert.equal(result.phase, phase); assert.equal(result.input_mode, "disabled");
    assert.equal(result.accepted_input_revision, 5);
    assert.deepEqual(calls, ["send_steam_workshop_account_input", "read_steam_workshop_account_login"]);
  }
});

test("module login is scoped to a real module target without an invented instance", async () => {
  const target = { kind: "module", id: "fixture-module" }, calls = [];
  const api = load(async (command, args) => { calls.push({ command, args }); return snapshot({ target }); });
  await api.startSteamWorkshopAccountLogin(target, "session-a");
  await api.readSteamWorkshopAccountLogin(target, "session-a");
  await api.cancelSteamWorkshopAccountLogin(target, "session-a");
  assert.equal(calls.length, 3);
  for (const { args } of calls) { assert.equal(args.target, target); assert.equal(Object.hasOwn(args, "instanceId"), false); }
});

test("receipt recovery cannot accept a snapshot for another instance or session", async () => {
  const api = load(async (command) => {
    if (command === "send_steam_workshop_account_input") throw new Error("synthetic-private-error");
    return snapshot({ target: { kind: "instance", id: "another-instance" }, revision: 3, accepted_input_revision: 1 });
  });
  await assert.rejects(api.sendSteamWorkshopAccountInput({ kind: "instance", id: "instance-a" }, "session-a", 1, "synthetic-password"), (error) => {
    assert.equal(error.code, "steam_account_input_unconfirmed");
    return true;
  });
});

test("LAN credential input is rejected before any transport call", async () => {
  let calls = 0;
  const api = load(async () => { calls++; return snapshot(); }, true);
  await assert.rejects(api.sendSteamWorkshopAccountInput({ kind: "instance", id: "instance-a" }, "session-a", 1, "synthetic-password"), (error) => {
    assert.equal(error.code, "steam_account_interactive_unsupported");
    return true;
  });
  assert.equal(calls, 0);
});

test("native login and revocation remain available when browser LAN routing is also enabled", async () => {
  const calls = [], target = { kind: "instance", id: "instance-a" };
  const api = load(async (command) => {
    calls.push(command);
    return snapshot();
  }, true, true);
  assert.equal(api.canUseSteamWorkshopAccountLogin(), true);
  await api.startSteamWorkshopAccountLogin(target, "session-a");
  await api.clearSteamServerAccountAuthorization(target);
  assert.deepEqual(calls, ["start_steam_workshop_account_login", "clear_steam_server_account_authorization"]);
});

test("restoring anonymous revokes only the bound server authorization and is local-only", async () => {
  const target = { kind: "module", id: "fixture-module" }, calls = [];
  const api = load(async (command, args) => { calls.push({ command, args }); });
  await api.clearSteamServerAccountAuthorization(target);
  assert.equal(calls.length, 1); assert.equal(calls[0].command, "clear_steam_server_account_authorization");
  assert.deepEqual(Object.keys(calls[0].args), ["target"]); assert.equal(calls[0].args.target, target);
  await assert.rejects(load(() => assert.fail("LAN must not revoke local authorization"), true).clearSteamServerAccountAuthorization(target), (error) => error.code === "steam_account_interactive_unsupported");
});
