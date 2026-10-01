const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const desktopRoot = path.resolve(__dirname, "..");

require.extensions[".ts"] = function compileTypeScript(module, filename) {
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
};

function readSource(...segments) {
  return fs.readFileSync(path.join(desktopRoot, ...segments), "utf8");
}

function objectKeys(source, constantName) {
  const match = source.match(new RegExp(`const ${constantName} = \\{([\\s\\S]*?)\\n\\} as const;`));
  assert.ok(match, `${constantName} is missing`);
  return Array.from(match[1].matchAll(/^\s{2}([a-z_]+):/gm), (entry) => entry[1]).sort();
}

test("library completion notices localize every known install state", () => {
  const { formatInstallState } = require(path.join(desktopRoot, "src", "install-state-presentation.ts"));
  const translate = (key) => key;
  const expected = {
    Installed: "status.install.installed",
    Installing: "status.install.installing",
    Incomplete: "status.install.incomplete",
    Updating: "status.install.updating",
    Uninstalling: "status.install.uninstalling",
    Corrupted: "status.install.corrupted",
    NotInstalled: "status.install.notinstalled"
  };

  for (const [state, key] of Object.entries(expected)) {
    assert.equal(formatInstallState(state, translate), key, state);
  }
  assert.equal(formatInstallState("FutureState", translate), "FutureState");
  assert.equal(formatInstallState(null, translate), "status.install.unknown");

  const actions = readSource("src", "hooks", "useDesktopActions.ts");
  assert.equal((actions.match(/state: formatInstallState\(result\.install_state, t\)/g) ?? []).length, 1);
  assert.match(actions, /count: result\.cleanup\.removed_install_roots\.length/);
  assert.doesNotMatch(actions, /state: String\(result\.install_state\)/);
});

test("broadcast history exhaustively localizes known structured labels", () => {
  const source = readSource("src", "views", "servers", "AiBroadcastWorkbench.tsx");
  assert.deepEqual(objectKeys(source, "BROADCAST_STATUS_LABELS"), ["blocked", "failed", "generated", "sent"]);
  assert.deepEqual(objectKeys(source, "BROADCAST_SOURCE_LABELS"), ["manual", "periodic", "runtime_health", "shutdown", "startup"]);
  assert.deepEqual(objectKeys(source, "BROADCAST_INITIATOR_LABELS"), ["auto", "lifecycle", "manual", "system"]);
  assert.doesNotMatch(source, />\{event\.(?:status|source|initiator)\}<\/span>/);

  const catalogs = [
    readSource("src", "i18n-messages-en-extra.ts"),
    readSource("src", "i18n-messages-zh-extra.ts")
  ];
  for (const catalog of catalogs) {
    for (const key of [
      "status.generated", "status.sent", "status.failed", "status.blocked",
      "source.manual", "source.startup", "source.shutdown", "source.runtimeHealth", "source.periodic",
      "initiator.manual", "initiator.auto", "initiator.lifecycle", "initiator.system"
    ]) {
      assert.match(catalog, new RegExp(`"servers\\.broadcast\\.${key.replace(".", "\\.")}"\\s*:`), key);
    }
  }
});

test("workbench errors use the shared descriptor and localized action shells", () => {
  const files = {
    dst: readSource("src", "views", "servers", "DstWorldImportPanel.tsx"),
    mods: readSource("src", "views", "servers", "ModWorkbench.tsx"),
    modMutations: readSource("src", "views", "servers", "useModWorkbenchMutations.ts"),
    runtime: readSource("src", "views", "servers", "RuntimeSurfaceWorkbench.tsx")
  };

  for (const [name, source] of Object.entries(files)) {
    const formatter = name === "modMutations" ? /formatDesktopError\(t, error\)/ : /describeError\(error\)/;
    assert.match(source, formatter, `${name} must use its shared error formatter`);
    assert.doesNotMatch(source, /error instanceof Error \? error\.message : String\(error\)/, name);
  }

  assert.match(files.dst, /dst\.settings\.bootstrap\.importFailed/);
  for (const key of ["inventoryFailed", "manualStageFailed", "referenceFailed"]) {
    assert.match(files.mods, new RegExp(`servers\\.mods\\.${key}`), key);
  }
  assert.match(files.modMutations, /servers\.mods\.mutationFailed/);
  assert.match(files.runtime, /servers\.details\.runtimeLogReadError/);
});
