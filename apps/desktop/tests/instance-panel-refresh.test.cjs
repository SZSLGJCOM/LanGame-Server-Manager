const assert = require("node:assert/strict");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const desktopRoot = path.resolve(__dirname, "..");

require.extensions[".ts"] = function compileTypeScript(module, filename) {
  const source = require("node:fs").readFileSync(filename, "utf8");
  const outputText = transpileTypeScript(source, filename);
  module._compile(outputText, filename);
};

const {
  InstanceSelectionCursor,
  refreshInstancePanelForCurrentSelection
} = require(path.join(desktopRoot, "src", "instance-panel-refresh.ts"));

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

function panel(instanceId) {
  return { instanceId, marker: `panel-${instanceId}` };
}

function createHarness(instanceId = "server-a") {
  const selection = new InstanceSelectionCursor(instanceId);
  const cachedPanels = new Map();
  let selectedPanel = panel(instanceId);
  let bootstrapRefreshCount = 0;

  return {
    selection,
    cachedPanels,
    get selectedPanel() {
      return selectedPanel;
    },
    select(nextInstanceId) {
      selection.capture(nextInstanceId);
      selectedPanel = panel(nextInstanceId);
    },
    ports: {
      getCurrentInstanceId: () => selection.current(),
      cacheInstancePanel: (targetId, nextPanel) => cachedPanels.set(targetId, nextPanel),
      replaceSelectedInstancePanel: (nextPanel) => {
        selectedPanel = nextPanel;
      },
      reloadBootstrap: async () => {
        bootstrapRefreshCount += 1;
      }
    },
    get bootstrapRefreshCount() {
      return bootstrapRefreshCount;
    }
  };
}

for (const mutation of ["settings save", "player access", "runtime command", "window suppression"]) {
  test(`${mutation}: completing A after selecting B updates only A caches`, async () => {
    const loaded = deferred();
    const harness = createHarness("server-a");
    const refresh = refreshInstancePanelForCurrentSelection(
      "server-a",
      () => loaded.promise,
      harness.ports
    );

    harness.select("server-b");
    loaded.resolve(panel("server-a"));

    assert.equal(await refresh, "cached-only");
    assert.equal(harness.selection.current(), "server-b");
    assert.deepEqual(harness.selectedPanel, panel("server-b"));
    assert.deepEqual(harness.cachedPanels.get("server-a"), panel("server-a"));
    assert.equal(harness.bootstrapRefreshCount, 1);
  });
}

test("a silent runtime command can update A's cache without refreshing bootstrap or replacing B", async () => {
  const harness = createHarness("server-a");
  delete harness.ports.reloadBootstrap;
  harness.select("server-b");

  const result = await refreshInstancePanelForCurrentSelection(
    "server-a",
    async () => panel("server-a"),
    harness.ports
  );

  assert.equal(result, "cached-only");
  assert.equal(harness.bootstrapRefreshCount, 0);
  assert.deepEqual(harness.selectedPanel, panel("server-b"));
  assert.deepEqual(harness.cachedPanels.get("server-a"), panel("server-a"));
});

test("selection changing while A bootstrap refresh is pending cannot replace B's panel", async () => {
  const bootstrap = deferred();
  const harness = createHarness("server-a");
  let bootstrapStarted = false;
  harness.ports.reloadBootstrap = () => {
    bootstrapStarted = true;
    return bootstrap.promise;
  };

  const refresh = refreshInstancePanelForCurrentSelection(
    "server-a",
    async () => panel("server-a"),
    harness.ports
  );
  await Promise.resolve();
  await Promise.resolve();
  assert.equal(bootstrapStarted, true, "the selection change must occur inside the bootstrap await window");

  harness.select("server-b");
  bootstrap.resolve();

  assert.equal(await refresh, "cached-only");
  assert.equal(harness.selection.current(), "server-b");
  assert.deepEqual(harness.selectedPanel, panel("server-b"));
  assert.deepEqual(harness.cachedPanels.get("server-a"), panel("server-a"));
});

test("a still-selected instance refreshes bootstrap before replacing its selected panel", async () => {
  const events = [];
  const harness = createHarness("server-a");
  harness.ports.cacheInstancePanel = (targetId, nextPanel) => {
    events.push(`cache:${targetId}`);
    harness.cachedPanels.set(targetId, nextPanel);
  };
  harness.ports.reloadBootstrap = async () => {
    events.push("bootstrap");
  };
  harness.ports.replaceSelectedInstancePanel = (nextPanel) => {
    events.push(`replace:${nextPanel.instanceId}`);
    harness.select(nextPanel.instanceId);
  };

  const result = await refreshInstancePanelForCurrentSelection(
    "server-a",
    async () => panel("server-a"),
    harness.ports
  );

  assert.equal(result, "selected");
  assert.deepEqual(events, ["cache:server-a", "bootstrap", "replace:server-a"]);
  assert.deepEqual(harness.selectedPanel, panel("server-a"));
  assert.deepEqual(harness.cachedPanels.get("server-a"), panel("server-a"));
});

test("a direct selection intent is visible before React state commits", () => {
  const selection = new InstanceSelectionCursor("server-a");
  const directUpdate = selection.prepare("server-b");

  assert.equal(selection.current(), "server-b");
  assert.equal(directUpdate.commit("server-a"), "server-b");
  assert.equal(selection.current(), "server-b");
});

test("an older queued functional update cannot overwrite a newer direct selection intent", () => {
  const selection = new InstanceSelectionCursor("server-a");
  const olderUpdate = selection.prepare((current) => current);
  const newerUpdate = selection.prepare("server-b");

  assert.equal(selection.current(), "server-b", "direct navigation must be visible before React commits it");
  assert.equal(olderUpdate.commit("server-a"), "server-a");
  assert.equal(selection.current(), "server-b", "the stale updater must not roll the cursor back");
  assert.equal(newerUpdate.commit("server-a"), "server-b");
  assert.equal(selection.current(), "server-b");
});

test("the latest functional selection intent updates the cursor when React commits it", () => {
  const selection = new InstanceSelectionCursor("server-a");
  const functionalUpdate = selection.prepare((current) => current === "server-a" ? "server-b" : current);

  assert.equal(selection.current(), "server-a", "a functional update needs React's current state before it can resolve");
  assert.equal(functionalUpdate.commit("server-a"), "server-b");
  assert.equal(selection.current(), "server-b");
});

test("a direct selection intent can reset the cursor to null", () => {
  const selection = new InstanceSelectionCursor("server-a");
  const reset = selection.prepare(null);

  assert.equal(selection.current(), null);
  assert.equal(reset.commit("server-a"), null);
  assert.equal(selection.current(), null);
});
