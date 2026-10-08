const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const ts = require("@typescript/typescript6");
const { parseSource, visitSyntax } = require("../scripts/typescript_source_tools.cjs");

const sourceRoot = path.resolve(__dirname, "../src");

function sources(directory) {
  return fs.readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const filename = path.join(directory, entry.name);
    return entry.isDirectory() ? sources(filename) : /\.[jt]sx?$/.test(entry.name) ? [filename] : [];
  });
}

test("UI confirmations and alerts stay in the application instead of browser dialogs", () => {
  const violations = [];
  for (const filename of sources(sourceRoot)) {
    const tree = parseSource(fs.readFileSync(filename, "utf8"), filename);
    visitSyntax(tree, (node) => {
      if ((ts.isPropertyAccessExpression(node) || ts.isElementAccessExpression(node))
        && ts.isIdentifier(node.expression) && ["window", "globalThis", "self"].includes(node.expression.text)) {
        const name = ts.isPropertyAccessExpression(node) ? node.name.text : node.argumentExpression?.text;
        if (["confirm", "alert"].includes(name)) violations.push(`${path.relative(sourceRoot, filename)}: ${name}`);
      }
    });
  }
  assert.deepEqual(violations, [], "Use InlineConfirmAction beside the triggering action; keep complex review UI in the application.");
});
