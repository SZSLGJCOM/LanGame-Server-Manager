const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { parseSync } = require("@swc/core");

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
    const tree = parseSync(fs.readFileSync(filename, "utf8"), {
      syntax: "typescript", tsx: filename.endsWith("x"),
    });
    function visit(node) {
      if (!node || typeof node !== "object") return;
      if (node.type === "MemberExpression" && node.object?.type === "Identifier"
        && ["window", "globalThis", "self"].includes(node.object.value)) {
        const name = node.property.type === "Computed" ? node.property.expression?.value : node.property.value;
        if (["confirm", "alert"].includes(name)) violations.push(`${path.relative(sourceRoot, filename)}: ${name}`);
      }
      for (const value of Object.values(node)) {
        if (Array.isArray(value)) value.forEach(visit);
        else if (value && typeof value === "object") visit(value);
      }
    }
    visit(tree);
  }
  assert.deepEqual(violations, [], "Use InlineConfirmAction beside the triggering action; keep complex review UI in the application.");
});
