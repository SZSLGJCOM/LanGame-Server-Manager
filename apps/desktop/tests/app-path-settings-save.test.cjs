const ts = require("@typescript/typescript6");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const {
  parseSource,
  sourceText,
  transpileTypeScript,
  visitSyntax
} = require("../scripts/typescript_source_tools.cjs");

const filename = path.join(__dirname, "../src/App.tsx");
const source = fs.readFileSync(filename, "utf8");
let declaration;
visitSyntax(parseSource(source, filename), (node) => {
  if (ts.isFunctionDeclaration(node) && node.name?.text === "handleSaveAppSettings") {
    declaration = node;
    return false;
  }
});
assert.ok(declaration, "The application must expose its path-settings save handler");

// Run the actual application handler with its persistence and refresh boundaries injected.
const handlerSource = transpileTypeScript(
  `${sourceText(source, declaration)}\nexport { handleSaveAppSettings };`,
  "app-path-settings-handler.ts"
);

function loadHandler(dependencies) {
  const exports = {};
  vm.runInNewContext(handlerSource, {
    exports,
    message: (key, params) => ({ key, params: { ...params } }),
    describeError: (error) => error instanceof Error ? error.message : String(error),
    ...dependencies
  }, { filename });
  return exports.handleSaveAppSettings;
}

function deferred() {
  let resolve;
  const promise = new Promise((resolvePromise) => { resolve = resolvePromise; });
  return { promise, resolve };
}

const draft = Object.freeze({
  games_root: "  C:\\LanGame\\games  ",
  servers_root: "  C:\\LanGame\\servers  ",
  archives_root: "  C:\\LanGame\\archives  ",
  steamcmd_root: "  C:\\LanGame\\steamcmd  "
});

test("a rejected path save reaches the caller unchanged without refreshing or reporting success", async () => {
  const failure = new Error("The selected path is read-only");
  const events = [];
  const activities = [];
  const save = loadHandler({
    updateAppSettings: async () => { events.push("write"); throw failure; },
    reloadBootstrap: async () => { events.push("reload"); },
    runSteamCmdProbe: async () => { events.push("probe"); },
    setActivity: (activity) => { activities.push(activity); }
  });

  await assert.rejects(save(draft), (error) => error === failure);

  assert.deepEqual(events, ["write"]);
  assert.deepEqual(activities, [{
    key: "settings.paths.failed",
    params: { message: failure.message }
  }]);
});

test("a successful path save trims input and waits for persistence, state refresh and probing before success", async () => {
  const write = deferred();
  const reload = deferred();
  const probe = deferred();
  const events = [];
  const activities = [];
  let submitted;
  const save = loadHandler({
    updateAppSettings: (settings) => {
      submitted = { ...settings };
      events.push("write");
      return write.promise;
    },
    reloadBootstrap: () => { events.push("reload"); return reload.promise; },
    runSteamCmdProbe: () => { events.push("probe"); return probe.promise; },
    setActivity: (activity) => { activities.push(activity); }
  });

  const saving = save(draft);
  assert.deepEqual(submitted, {
    games_root: "C:\\LanGame\\games",
    servers_root: "C:\\LanGame\\servers",
    archives_root: "C:\\LanGame\\archives",
    steamcmd_root: "C:\\LanGame\\steamcmd"
  });
  assert.deepEqual(events, ["write"]);
  assert.deepEqual(activities, []);

  write.resolve({ games_root: "C:\\LanGame\\persisted-games" });
  await new Promise((resolve) => setImmediate(resolve));
  assert.deepEqual(events, ["write", "reload"]);
  assert.deepEqual(activities, []);

  reload.resolve();
  await new Promise((resolve) => setImmediate(resolve));
  assert.deepEqual(events, ["write", "reload", "probe"]);
  assert.deepEqual(activities, []);

  probe.resolve();
  await saving;
  assert.deepEqual(activities, [{
    key: "settings.paths.saved",
    params: { path: "C:\\LanGame\\persisted-games" }
  }]);
});
