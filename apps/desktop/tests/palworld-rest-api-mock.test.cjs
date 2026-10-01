const assert = require("node:assert/strict");
const fs = require("node:fs");
const Module = require("node:module");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (module, filename) => {
    module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  };
}
const originalResolveFilename = Module._resolveFilename;
Module._resolveFilename = function (request, parent, isMain, options) {
  if (typeof request === "string" && request.endsWith("?raw")) {
    return `${originalResolveFilename.call(this, request.slice(0, -4), parent, isMain, options)}?raw`;
  }
  return originalResolveFilename.call(this, request, parent, isMain, options);
};
require.extensions[".toml?raw"] = (module, filename) => {
  module._compile(`module.exports = ${JSON.stringify(fs.readFileSync(filename.slice(0, -4), "utf8"))};`, filename);
};

const { invokeMock } = require(path.join(__dirname, "../src/api-mock.ts"));
test.before(async () => {
  const status = await invokeMock("ensure_steamcmd_ready", { operationId: "fixture-steamcmd-ready" });
  assert.equal(status.ready, true);
});
const { resolveMockDeclaredRuntimeAction } = require(path.join(__dirname, "../src/api-mock/runtime-actions.ts"));

async function runningInstance(moduleId = "palworld") {
  await invokeMock("install_module_game", { moduleId });
  const details = await invokeMock("create_instance_record", {
    input: { name: "REST contract", module_id: moduleId }
  });
  await invokeMock("start_instance_process", { instanceId: details.summary.id });
  return details.summary.id;
}

test("Palworld declared actions encode exact JSON values and ignore client command metadata", async () => {
  const instanceId = await runningInstance();
  for (const [runtimeActionId, target, expected] of [
    ["show_players", undefined, { operation: "players" }],
    ["save_world", undefined, { operation: "save" }],
    ["broadcast", '  你好 "builders" \\ world; {{text}} && data  ',
      { operation: "announce", message: '  你好 "builders" \\ world; {{text}} && data  ' }],
    ["unban_player", " user; literal ", { operation: "unban", userid: " user; literal " }]
  ]) {
    const result = await invokeMock("send_instance_runtime_command", { input: {
      instanceId, runtimeActionId, runtimeActionTarget: target,
      command: "ignored", transport: "stdin", processKey: "caves"
    } });
    assert.deepEqual(JSON.parse(result.command), expected);
    assert.equal(result.process_key, "palworld_rest");
    assert.equal(result.display_name, "Palworld REST API");
    assert.equal(result.pid, 0);
    assert.match(result.response_text, /rest-api/);
  }
});

test("Palworld REST uses Unicode character limits, rejects controls and roles, and checks the action contract", async () => {
  const module = await invokeMock("read_module_details", { moduleId: "palworld" });
  const actions = module.runtime.player_actions;
  const resolve = (id, target, role) => resolveMockDeclaredRuntimeAction(actions, "palworld", {
    runtimeActionId: id, runtimeActionTarget: target, runtimeActionRole: role
  });
  for (const [id, limit] of [["broadcast", 240], ["unban_player", 128]]) {
    assert.ok(resolve(id, "界".repeat(limit)));
    assert.ok(resolve(id, "🎮".repeat(limit)));
    for (const target of ["", "   ", "界".repeat(limit + 1), "🎮".repeat(limit + 1),
      "line\nbreak", "tab\ttext", "delete\u007f", "control\u0085", "nul\u0000", {}]) {
      assert.throws(() => resolve(id, target), /target is invalid/);
    }
  }
  for (const role of ["admin", " ", "\t"]) {
    assert.throws(() => resolve("broadcast", "Hello world", role), /do not accept roles/);
  }
  assert.throws(() => resolve("save_world", " "), /target does not match/);
  assert.throws(() => resolveMockDeclaredRuntimeAction(actions, "conanexiles", {
    runtimeActionId: "save_world"
  }), /require the Palworld module/);
  const save = actions.find((action) => action.id === "save_world");
  assert.throws(() => resolveMockDeclaredRuntimeAction([{ ...save, command_template: "shutdown" }], "palworld", {
    runtimeActionId: "save_world"
  }), /contract is unsupported/);
  assert.throws(() => resolveMockDeclaredRuntimeAction([{ ...save, target_required: true }], "palworld", {
    runtimeActionId: "save_world"
  }), /target does not match/);
});

test("raw Palworld REST and RCON are rejected and moderation still requires a current snapshot", async () => {
  const instanceId = await runningInstance();
  for (const endpoint of ["send_instance_runtime_command", "send_instance_gm_command"]) {
    for (const transport of ["palworld_rest", " PALWORLD_REST ", "source_rcon", "humanitz_rcon"]) {
      await assert.rejects(invokeMock(endpoint, { input: {
        instanceId, transport, command: '{"operation":"save"}'
      } }), /declared runtime action|authenticated REST API/);
    }
    for (const runtimeActionId of ["kick_player", "ban_player"]) {
      await assert.rejects(invokeMock(endpoint, { input: {
        instanceId, runtimeActionId, runtimeActionTarget: "caller-supplied-user"
      } }), /snapshot service/);
    }
  }
  await assert.rejects(invokeMock("execute_instance_player_action", { input: {
    instance_id: instanceId, action_id: "kick_player", snapshot_id: "missing", player_key: "missing"
  } }), /snapshot/i);
});

test("Palworld broadcast endpoint renders typed JSON and enforces its broadcast validation", async () => {
  const instanceId = await runningInstance();
  const result = await invokeMock("send_instance_broadcast", { input: {
    instanceId, message: '  你好 "builders" \\ world  ', source: "manual"
  } });
  assert.equal(result.transport, "palworld_rest");
  assert.deepEqual(JSON.parse(result.commandPreview), {
    operation: "announce", message: '你好 "builders" \\ world'
  });
  for (const message of ["", "x".repeat(241), "line\nbreak", "control\u0085", "hello;quit"]) {
    await assert.rejects(invokeMock("send_instance_broadcast", { input: {
      instanceId, message, source: "manual"
    } }), /target is invalid|unsupported command syntax/);
  }
});

test("HumanitZ remote dispatch has no local process PID or stdin response", async () => {
  const instanceId = await runningInstance("humanitz");
  const result = await invokeMock("send_instance_runtime_command", { input: {
    instanceId, transport: "humanitz_rcon", command: "info"
  } });
  assert.equal(result.process_key, "humanitz_rcon");
  assert.equal(result.display_name, "RCON");
  assert.equal(result.pid, 0);
  assert.match(result.response_text, /humanitz-rcon/);
});

test("command-line action targets still reject command syntax after REST data rendering", () => {
  const action = {
    id: "kick_player", transport: "source_rcon", command_template: "kick {{target}}", target_required: true
  };
  for (const runtimeActionTarget of ["Player;quit", "Player&&quit", "{{Player}}", "two players"]) {
    assert.throws(() => resolveMockDeclaredRuntimeAction([action], "conanexiles", {
      runtimeActionId: action.id, runtimeActionTarget
    }), /unsupported command syntax|single-token/);
  }
});
