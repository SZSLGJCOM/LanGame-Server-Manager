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

const { InstanceSelectionCursor } = require(path.join(desktopRoot, "src", "instance-panel-refresh.ts"));

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
