const assert = require("node:assert/strict");
const fs = require("node:fs");
const Module = require("node:module");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const desktopRoot = path.resolve(__dirname, "..");
const repositoryRoot = path.resolve(desktopRoot, "..", "..");
const modulesRoot = path.join(repositoryRoot, "modules");
const matrixPath = path.join(repositoryRoot, "docs", "player-center-capability-matrix.md");
for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = function compileTypeScript(module, filename) {
    const source = fs.readFileSync(filename, "utf8");
    const outputText = transpileTypeScript(source, filename);
    module._compile(outputText, filename);
  };
}

const originalResolveFilename = Module._resolveFilename;
Module._resolveFilename = function resolveRawImports(request, parent, isMain, options) {
  if (typeof request === "string" && request.endsWith("?raw")) {
    const resolved = originalResolveFilename.call(this, request.slice(0, -4), parent, isMain, options);
    return `${resolved}?raw`;
  }
  return originalResolveFilename.call(this, request, parent, isMain, options);
};
require.extensions[".toml?raw"] = function compileRawToml(module, filename) {
  const source = fs.readFileSync(filename.slice(0, -4), "utf8");
  module._compile(`module.exports = ${JSON.stringify(source)};`, filename);
};

const { invokeMock } = require(path.join(desktopRoot, "src", "api-mock.ts"));
test.before(async () => {
  const status = await invokeMock("ensure_steamcmd_ready", { operationId: "fixture-steamcmd-ready" });
  assert.equal(status.ready, true);
});
const {
  MockLivePlayerStore,
  parseMockPlayerListFromModuleToml
} = require(path.join(desktopRoot, "src", "api-mock", "live-players.ts"));
const {
  mockModuleTomlById,
  readMockTomlArrayTables,
  readMockTomlBoolean,
  readMockTomlString,
  readMockTomlStringArray,
  readMockTomlTable
} = require(path.join(desktopRoot, "src", "api-mock", "module-assets.ts"));

const ALLOWED_TRANSPORTS = new Set([
  "battleye_rcon", "source_rcon", "stdin", "telnet", "websocket_rcon", "humanitz_rcon", "palworld_rest"
]);
const ACCESS_KINDS = ["admin", "allow", "block", "priority"];

function moduleIds() {
  return fs.readdirSync(modulesRoot)
    .filter((id) => fs.existsSync(path.join(modulesRoot, id, "module.toml")))
    .filter((id) => fs.existsSync(path.join(modulesRoot, id, "schema.json")))
    .sort();
}

function moduleSchema(moduleId) {
  return JSON.parse(fs.readFileSync(path.join(modulesRoot, moduleId, "schema.json"), "utf8"));
}

function accessFields(moduleId) {
  return Object.entries(moduleSchema(moduleId).properties ?? {})
    .filter(([, property]) => property && typeof property === "object"
      && Object.hasOwn(property, "x-lsgm-player-access-kind"));
}

function accessKinds(moduleId) {
  const present = new Set(accessFields(moduleId)
    .map(([, property]) => property["x-lsgm-player-access-kind"]));
  return ACCESS_KINDS.filter((kind) => present.has(kind));
}

function consumedActionIds(moduleId, declaredActionIds) {
  const consumed = new Set();
  for (const [, property] of accessFields(moduleId)) {
    const sync = property["x-lsgm-player-access-sync"];
    for (const actionId of sync?.consume_action_ids ?? []) {
      if (declaredActionIds.has(actionId)) {
        consumed.add(actionId);
      }
    }
  }
  return consumed;
}

function expectedAction(block) {
  return {
    id: readMockTomlString(block, "id") ?? "action",
    kind: readMockTomlString(block, "kind"),
    label: readMockTomlString(block, "label") ?? readMockTomlString(block, "id") ?? "action",
    label_zh_cn: readMockTomlString(block, "label_zh_cn"),
    transport: readMockTomlString(block, "transport") ?? "stdin",
    command_template: readMockTomlString(block, "command_template") ?? "",
    target_label: readMockTomlString(block, "target_label"),
    target_label_zh_cn: readMockTomlString(block, "target_label_zh_cn"),
    target_placeholder: readMockTomlString(block, "target_placeholder"),
    target_placeholder_zh_cn: readMockTomlString(block, "target_placeholder_zh_cn"),
    target_required: readMockTomlBoolean(block, "target_required") ?? false,
    target_encoding: readMockTomlString(block, "target_encoding"),
    role_values: readMockTomlStringArray(block, "role_values"),
    process_key: readMockTomlString(block, "process_key"),
    port_name: readMockTomlString(block, "port_name"),
    password_setting_key: readMockTomlString(block, "password_setting_key"),
    enabled_setting_key: readMockTomlString(block, "enabled_setting_key"),
    destructive: readMockTomlBoolean(block, "destructive") ?? false
  };
}

function manualActionIds(moduleId, details) {
  if (details.runtime.player_management?.status === "pending_adapter") return [];
  const actions = details.runtime.player_actions ?? [];
  const declaredIds = new Set(actions.map((action) => action.id));
  const hidden = new Set();
  if (details.runtime.player_list) {
    hidden.add(details.runtime.player_list.action_id);
    for (const actionId of details.runtime.player_list.player_action_ids) {
      hidden.add(actionId);
    }
  }
  const consumed = consumedActionIds(moduleId, declaredIds);
  return actions
    .filter((action) => action.kind !== "broadcast")
    .filter((action) => !hidden.has(action.id) && !consumed.has(action.id))
    .filter((action) => action.target_required === true || action.command_template.includes("{{target}}"))
    .map((action) => action.id);
}

function onlineMode(moduleId, details) {
  const list = details.runtime.player_list;
  if (list) return list.player_action_ids.length > 0 ? "structured" : "read_only";
  return manualActionIds(moduleId, details).length > 0 ? "manual_id" : "unsupported";
}

function evidenceFor(details) {
  const list = details.runtime.player_list;
  return list ? `codec:${list.response_codec}` : `unavailable:${details.runtime.player_management?.status ?? "pending_adapter"}`;
}

function parseCellList(value) {
  if (value === "—") {
    return [];
  }
  return value.replaceAll("`", "").split(",").map((item) => item.trim()).filter(Boolean);
}

function matrixRows() {
  const rows = new Map();
  for (const line of fs.readFileSync(matrixPath, "utf8").split(/\r?\n/)) {
    if (!/^\|\s*`[a-z0-9]+`\s*\|/.test(line)) {
      continue;
    }
    const cells = line.split("|").slice(1, -1).map((cell) => cell.trim());
    const id = cells[0].replaceAll("`", "");
    assert.equal(cells.length, 5, `matrix row ${id} must have five columns`);
    assert.ok(!rows.has(id), `matrix contains duplicate row ${id}`);
    rows.set(id, {
      mode: cells[1],
      manualActions: parseCellList(cells[2]),
      accessKinds: parseCellList(cells[3]),
      evidence: cells[4].replaceAll("`", "")
    });
  }
  return rows;
}

let detailsPromise;
function allModuleDetails() {
  if (!detailsPromise) {
    detailsPromise = Promise.all(moduleIds().map(async (moduleId) => [
      moduleId,
      await invokeMock("read_module_details", { moduleId, module_id: moduleId })
    ])).then((entries) => new Map(entries));
  }
  return detailsPromise;
}

test("mock TOML strings preserve basic, literal, and multiline command templates", () => {
  assert.equal(readMockTomlString('value = "say \\"hello\\""', "value"), 'say "hello"');
  assert.equal(readMockTomlString("value = 'print(\"[PLAYER]\")'", "value"), 'print("[PLAYER]")');
  assert.equal(readMockTomlString('value = """\nline one\nline two\n"""', "value"), "line one\nline two\n");
  assert.equal(readMockTomlString("value = '''\nraw \\ value\n'''", "value"), "raw \\ value\n");
});

test("all 32 module manifests project every declared runtime action into the mock API", async () => {
  const ids = moduleIds();
  const detailsById = await allModuleDetails();
  assert.equal(ids.length, 32);

  let projectedActionCount = 0;
  for (const moduleId of ids) {
    const expected = readMockTomlArrayTables(mockModuleTomlById[moduleId], "runtime.player_actions")
      .map(expectedAction);
    const projected = detailsById.get(moduleId).runtime.player_actions ?? [];
    assert.deepEqual(projected, expected, `${moduleId} mock action projection drifted from module.toml`);
    projectedActionCount += projected.length;

    const seen = new Set();
    for (const action of projected) {
      assert.ok(action.id && !seen.has(action.id), `${moduleId} has an empty or duplicate action id`);
      seen.add(action.id);
      assert.ok(action.label.trim(), `${moduleId}.${action.id} has no UI label`);
      assert.ok(action.label_zh_cn?.trim(), `${moduleId}.${action.id} has no Simplified Chinese UI label`);
      if (action.target_label) {
        assert.ok(
          action.target_label_zh_cn?.trim(),
          `${moduleId}.${action.id} has no Simplified Chinese target label`
        );
      }
      if (action.target_placeholder) {
        assert.ok(
          action.target_placeholder_zh_cn?.trim(),
          `${moduleId}.${action.id} has no Simplified Chinese target placeholder`
        );
      }
      assert.ok(action.command_template.trim(), `${moduleId}.${action.id} has no backend template`);
      assert.ok(ALLOWED_TRANSPORTS.has(action.transport), `${moduleId}.${action.id} has an unknown transport`);
      assert.equal(
        action.command_template.includes("{{target}}"),
        action.target_required,
        `${moduleId}.${action.id} target binding and target_required disagree`
      );
      assert.equal(
        action.command_template.includes("{{role}}"),
        action.role_values.length > 0,
        `${moduleId}.${action.id} role binding and role_values disagree`
      );
    }
  }
  assert.equal(projectedActionCount, ids.reduce((count, id) => count + readMockTomlArrayTables(mockModuleTomlById[id], "runtime.player_actions").length, 0));
});

test("online, manual-ID, access-control, and unsupported surfaces cover 32/32 modules", async () => {
  const ids = moduleIds();
  const detailsById = await allModuleDetails();
  const rows = matrixRows();
  assert.deepEqual([...rows.keys()].sort(), ids);

  for (const moduleId of ids) {
    const details = detailsById.get(moduleId);
    const management = readMockTomlString(readMockTomlTable(mockModuleTomlById[moduleId], "player_management"), "status");
    assert.equal(details.runtime.player_management?.status, management, `${moduleId} management projection`);
    assert.deepEqual(rows.get(moduleId), {
      mode: onlineMode(moduleId, details),
      manualActions: manualActionIds(moduleId, details),
      accessKinds: accessKinds(moduleId),
      evidence: evidenceFor(details)
    }, `${moduleId} matrix differs from module authority`);
  }

});

test("schema-backed access control is complete and never inferred from online players", async () => {
  const detailsById = await allModuleDetails();
  const modulesWithAccess = [];
  for (const moduleId of moduleIds()) {
    const actions = detailsById.get(moduleId).runtime.player_actions ?? [];
    const actionIds = new Set(actions.map((action) => action.id));
    const fields = accessFields(moduleId);
    if (fields.length > 0) {
      modulesWithAccess.push(moduleId);
    }
    for (const [fieldKey, property] of fields) {
      assert.ok(ACCESS_KINDS.includes(property["x-lsgm-player-access-kind"]), `${moduleId}.${fieldKey} kind is invalid`);
      assert.equal(typeof property["x-lsgm-player-access-codec"], "string", `${moduleId}.${fieldKey} codec is missing`);
      assert.equal(typeof property["x-lsgm-player-access-sync"], "object", `${moduleId}.${fieldKey} sync is missing`);
      const sync = property["x-lsgm-player-access-sync"];
      for (const key of ["add_action_id", "remove_action_id", "verify_action_id"]) {
        if (sync[key]) {
          assert.ok(actionIds.has(sync[key]), `${moduleId}.${fieldKey} references missing ${sync[key]}`);
        }
      }
      for (const actionId of sync.consume_action_ids ?? []) {
        assert.ok(actionIds.has(actionId), `${moduleId}.${fieldKey} consumes missing ${actionId}`);
      }
    }
  }
  const documentedAccessModules = [...matrixRows()]
    .filter(([, row]) => row.accessKinds.length > 0)
    .map(([moduleId]) => moduleId)
    .sort();
  assert.deepEqual(modulesWithAccess, documentedAccessModules,
    "schema-backed access modules must match the published capability matrix");
});

test("all declared actions have a deliberate Player Center or sibling surface owner", async () => {
  const detailsById = await allModuleDetails();
  const ownerCounts = { structured: 0, broadcast: 0, access: 0, manual: 0, runtimeConsole: 0, unavailable: 0 };
  for (const moduleId of moduleIds()) {
    const details = detailsById.get(moduleId);
    const actions = details.runtime.player_actions ?? [];
    const actionIds = new Set(actions.map((action) => action.id));
    const consumed = consumedActionIds(moduleId, actionIds);
    const structured = new Set();
    if (details.runtime.player_list) {
      structured.add(details.runtime.player_list.action_id);
      details.runtime.player_list.player_action_ids.forEach((id) => structured.add(id));
    }
    for (const action of actions) {
      const owner = structured.has(action.id)
        ? "structured"
        : action.kind === "broadcast"
          ? "broadcast"
          : consumed.has(action.id)
            ? "access"
            : details.runtime.player_management?.status === "pending_adapter"
              ? "unavailable"
              : action.target_required === true || action.command_template.includes("{{target}}")
              ? "manual"
              : "runtimeConsole";
      ownerCounts[owner] += 1;
    }
  }
  assert.equal(Object.values(ownerCounts).reduce((sum, count) => sum + count, 0),
    [...detailsById.values()].reduce((sum, details) => sum + (details.runtime.player_actions ?? []).length, 0));
  assert.equal(ownerCounts.manual, [...detailsById].reduce((sum, [id, details]) => sum + manualActionIds(id, details).length, 0));

});

test("all remaining target-bound manual actions dispatch through mock module authority", async () => {
  const detailsById = await allModuleDetails();
  let dispatched = 0;
  for (const moduleId of moduleIds()) {
    const details = detailsById.get(moduleId);
    const manualIds = manualActionIds(moduleId, details);
    if (manualIds.length === 0) {
      continue;
    }
    await invokeMock("install_module_game", { moduleId });
    const instance = await invokeMock("create_instance_record", {
      input: { name: `Coverage ${moduleId}`, module_id: moduleId }
    });
    await invokeMock("start_instance_process", {
      instanceId: instance.summary.id,
      instance_id: instance.summary.id
    });
    for (const actionId of manualIds) {
      const action = details.runtime.player_actions.find((candidate) => candidate.id === actionId);
      const result = await invokeMock("execute_instance_manual_player_action", {
        input: {
          instance_id: instance.summary.id,
          action_id: action.id,
          target: "Player123",
          role: action.role_values?.[0] ?? null
        }
      });
      assert.equal(result.action_id, action.id);
      assert.equal(result.status, "sent");
      assert.equal(result.command, undefined, `${moduleId}.${actionId} leaked a rendered command`);
      dispatched += 1;
    }
  }
  assert.equal(dispatched, [...detailsById].reduce((sum, [id, details]) => sum + manualActionIds(id, details).length, 0));
});

test("manual REST mock dispatch rejects client metadata and validates target data", async () => {
  await invokeMock("install_module_game", { moduleId: "palworld" });
  const instance = await invokeMock("create_instance_record", {
    input: { name: "Authoritative manual action", module_id: "palworld" }
  });
  await invokeMock("start_instance_process", {
    instanceId: instance.summary.id,
    instance_id: instance.summary.id
  });
  const baseInput = {
    instance_id: instance.summary.id,
    action_id: "unban_player",
    role: null
  };
  const result = await invokeMock("execute_instance_manual_player_action", {
    input: { ...baseInput, target: "Player123" }
  });
  assert.equal(result.action_id, "unban_player");
  assert.equal(result.status, "sent");

  await assert.rejects(
    invokeMock("execute_instance_manual_player_action", {
      input: { ...baseInput, target: "Player123", transport: "telnet" }
    }),
    /dispatch metadata/
  );
  await assert.rejects(
    invokeMock("execute_instance_manual_player_action", {
      input: { ...baseInput, action_id: "undeclared_action", target: "Player123" }
    }),
    /not an identity-bound/
  );
  const literalTargetResult = await invokeMock("execute_instance_manual_player_action", {
    input: { ...baseInput, target: "Player123;quit" }
  });
  assert.equal(literalTargetResult.status, "sent");
  await assert.rejects(
    invokeMock("execute_instance_manual_player_action", {
      input: { ...baseInput, target: "\nPlayer123" }
    }),
    /target is invalid/
  );
  await assert.rejects(
    invokeMock("execute_instance_manual_player_action", {
      input: { ...baseInput, target: "P".repeat(600) }
    }),
    /target is invalid/
  );
});

for (const [moduleId, actions] of [
  ["dontstarve", [["list_online_players", "KU_injected"], ["kick_userid", "KU_injected"]]],
  ["palworld", [["kick_player", "Player123"], ["ban_player", "Player123"]]]
]) {
  test(`structured ${moduleId} actions cannot bypass snapshot authorization through the manual endpoint`, async () => {
    const instance = await invokeMock("create_instance_record", {
      input: { name: "Structured action boundary", module_id: moduleId }
    });
    await invokeMock("start_instance_process", {
      instanceId: instance.summary.id,
      instance_id: instance.summary.id
    });
    for (const [actionId, target] of actions) {
      await assert.rejects(
        invokeMock("execute_instance_manual_player_action", {
          input: {
            instance_id: instance.summary.id,
            action_id: actionId,
            target,
            role: null
          }
        }),
        /authoritative player snapshot/
      );
    }
  });
}

test("all 32 mock live-player states follow their declared collectors", async () => {
  const detailsById = await allModuleDetails();
  const store = new MockLivePlayerStore({ now: () => 10_000 });
  for (const moduleId of moduleIds()) {
    const list = detailsById.get(moduleId).runtime.player_list ?? null;
    const context = { instance_id: `coverage-${moduleId}`, module_id: moduleId,
      settings: moduleId === "valheim" ? { public_server: 1 } : moduleId === "vrising" ? { list_on_steam: true } : {},
      running: true, player_list: list, current_players: list ? 2 : 5, max_players: 20 };
    const initial = store.read(context);
    assert.deepEqual(initial.entries, []);
    if (list) {
      assert.equal(initial.status, "refreshing");
      const snapshot = store.refresh(context);
      assert.equal(snapshot.status, "ready", moduleId);
      assert.equal(snapshot.complete, true, moduleId);
      assert.equal(snapshot.entries.length, 2, moduleId);
      for (const entry of snapshot.entries) {
        assert.ok(entry.available_action_ids.every((id) => list.player_action_ids.includes(id)), moduleId);
        if (list.player_action_ids.length === 0) assert.deepEqual(entry.available_action_ids, []);
      }
    } else {
      assert.equal(initial.status, "unsupported", moduleId);
      assert.equal(initial.complete, false);
      assert.equal(initial.current_players, 5);
      assert.equal(initial.issue?.code, "adapter_unavailable");
    }
  }
});

test("every deployed player-list codec has bounded fixtures and traceable evidence", () => {
  const fixtureRoot = path.join(desktopRoot, "src-tauri", "test-data", "live-players");
  const codecFixtures = {
    dst_client_table_v1: "dst", rust_player_list: "rust", ark_list_players: "ark",
    conan_list_players: "conan", humanitz_players: "humanitz", zomboid_players: "zomboid",
    seven_days_players: "seven-days", squad_list_players: "squad", minecraft_players: "minecraft",
    palworld_players: "palworld", nightingale_players: "nightingale", necesse_players: "necesse",
    romestead_players: "romestead", terraria_players: "terraria", astroneer_players: "astroneer",
    soulmask_players: "soulmask", satisfactory_frm_players: "satisfactory", barotrauma_players: "barotrauma",
    return_to_moria_players: "returntomoria", windrose_players: "windrose", dragonwilds_players: "runescapedragonwilds", scum_players: "scum"
  };
  for (const moduleId of moduleIds()) {
    const declared = /\[runtime\.player_list\]/.test(mockModuleTomlById[moduleId]);
    const list = parseMockPlayerListFromModuleToml(moduleId);
    assert.equal(Boolean(list), declared, `${moduleId} player-list projection`);
    if (!list) continue;
    if (list.response_codec === "a2s_players") {
      assert.equal(list.source, "server_query");
      assert.deepEqual(list.player_action_ids, []);
      assert.equal(list.identity_kind, "player_name");
      continue;
    }
    const fixture = codecFixtures[list.response_codec];
    assert.ok(fixture, `${moduleId} codec has no fixture record`);
    const files = fs.readdirSync(path.join(fixtureRoot, fixture));
    assert.ok(files.some((name) => /empty|zero|^counts\.txt$/.test(name)), `${moduleId} has no empty fixture`);
    assert.ok(files.some((name) => /normal|ready|^players\.txt$/.test(name)), `${moduleId} has no populated fixture`);
    if (fixture !== "dst") assert.ok(files.some((name) => /SOURCE|evidence/.test(name)), `${moduleId} has no evidence provenance`);
    if (list.source === "console_log") assert.deepEqual(list.player_action_ids, []);
  }
});
