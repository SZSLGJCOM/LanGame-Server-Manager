const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const Module = require("node:module");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const filename = path.join(__dirname, "helpers/inline-preview-readiness.ts");
const loaded = new Module(filename, module);
loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
const { readInlinePreviewStatus } = loaded.exports;

test("iframe navigation waits for its document root before reading preview completion", () => {
  const frame = { contentDocument: null };
  assert.equal(readInlinePreviewStatus(frame), undefined);
  frame.contentDocument = { documentElement: null };
  assert.equal(readInlinePreviewStatus(frame), undefined);
  frame.contentDocument.documentElement = { dataset: {} };
  assert.equal(readInlinePreviewStatus(frame), undefined);
  frame.contentDocument.documentElement.dataset.preview = "passed";
  assert.equal(readInlinePreviewStatus(frame), "passed");
});

test("an unfinished or failed preview never counts as a successful preview", () => {
  for (const preview of [undefined, "loading", "Error: preview assertion failed"]) {
    assert.notEqual(readInlinePreviewStatus({ contentDocument: { documentElement: { dataset: { preview } } } }), "passed");
  }
});

test("unexpected cross-origin access errors remain visible", () => {
  const failure = new Error("Fixture origin changed");
  const frame = { get contentDocument() { throw failure; } };
  assert.throws(() => readInlinePreviewStatus(frame), (error) => error === failure);
});
