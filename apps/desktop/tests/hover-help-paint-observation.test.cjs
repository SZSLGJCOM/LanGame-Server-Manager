const ts = require("@typescript/typescript6");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { parseSource, sourceText, transpileTypeScript, visitSyntax } = require("../scripts/typescript_source_tools.cjs");

const filename = path.join(__dirname, "helpers/hover-help-browser.tsx");
const source = fs.readFileSync(filename, "utf8");
let declaration;
visitSyntax(parseSource(source, filename), (node) => {
  if (ts.isFunctionDeclaration(node) && node.name?.text === "observePaint") {
    declaration = node;
    return false;
  }
});
assert.ok(declaration, "Hover acceptance must retain its paint observation boundary");
const observerSource = transpileTypeScript(`${sourceText(source, declaration)}\nexport { observePaint };`, filename);

function browserFrames() {
  const frames = new Map();
  let nextFrame = 0;
  let visible = [];
  let acting = false;
  const unownedUpdates = [];
  const exports = {};
  vm.runInNewContext(observerSource, {
    exports,
    act: async (operation) => {
      acting = true;
      try { await operation(); } finally { acting = false; }
    },
    painted: () => visible.map((textContent) => ({ textContent })),
    requestAnimationFrame: (callback) => {
      frames.set(++nextFrame, callback);
      return nextFrame;
    },
    cancelAnimationFrame: (frame) => frames.delete(frame),
  }, { filename });
  return {
    observe: exports.observePaint,
    show: (...names) => { visible = names; },
    scheduleLayoutUpdate: () => {
      frames.set(++nextFrame, () => {
        if (!acting) unownedUpdates.push("Tooltip layout update escaped act");
      });
    },
    paint: () => {
      for (const [id, callback] of [...frames]) {
        frames.delete(id);
        callback(0);
      }
    },
    pendingFrames: () => frames.size,
    unownedUpdates,
  };
}

test("paint observation includes a final tooltip that appears between rendering frames", async () => {
  const browser = browserFrames();
  let settled = false;
  const observed = browser.observe(async () => { browser.show("gamma help"); });
  observed.then(() => { settled = true; });
  await new Promise(setImmediate);
  assert.equal(settled, false, "The observation must not finish before the final visible state is sampled");
  browser.paint();
  const report = await observed;
  assert.deepEqual([...report.seen], ["gamma help"]);
  assert.equal(report.maximum, 1);
  assert.equal(browser.pendingFrames(), 0, "Completed observation must cancel its next sampling frame");
});

test("paint observation preserves earlier unwanted or stacked tooltips", async () => {
  const browser = browserFrames();
  browser.show("alpha help", "beta help");
  const observed = browser.observe(async () => { browser.show("gamma help"); });
  await new Promise(setImmediate);
  browser.paint();
  const report = await observed;
  assert.deepEqual([...report.seen], ["alpha help", "beta help", "gamma help"]);
  assert.equal(report.maximum, 2);
  assert.equal(browser.pendingFrames(), 0);
});

test("a failed hover scenario cancels observation without hiding its error", async () => {
  const browser = browserFrames();
  const failure = new Error("Pointer dispatch failed");
  await assert.rejects(browser.observe(async () => { throw failure; }), (error) => error === failure);
  assert.equal(browser.pendingFrames(), 0);
});

test("the final sampled frame owns pending React tooltip layout updates", async () => {
  const browser = browserFrames();
  const observed = browser.observe(async () => {
    browser.show("gamma help");
    browser.scheduleLayoutUpdate();
  });
  await new Promise(setImmediate);
  browser.paint();
  await observed;
  assert.deepEqual(browser.unownedUpdates, [], "ResizeObserver layout work must remain inside the awaited act scope");
  assert.equal(browser.pendingFrames(), 0);
});
