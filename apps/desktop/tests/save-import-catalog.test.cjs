const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const root = path.resolve(__dirname, "../../..");
const guides = JSON.parse(fs.readFileSync(path.join(__dirname, "../src/save-import/guides.json"), "utf8"));

test("save migration guidance covers the current 32 game modules exactly once", () => {
  const modules = fs.readdirSync(path.join(root, "modules"), { withFileTypes: true })
    .filter(entry => entry.isDirectory() && fs.existsSync(path.join(root, "modules", entry.name, "module.toml")))
    .map(entry => entry.name).sort();
  assert.equal(modules.length, 32);
  assert.deepEqual(guides.map(guide => guide.moduleId).sort(), modules);
  for (const guide of guides) {
    for (const kind of ["server", "singlePlayer"]) {
      assert.ok(["direct", "conditional", "unsupported", "unverified", "not_applicable"].includes(guide[kind].status), guide.moduleId);
      assert.ok(guide[kind].zh.trim() && guide[kind].en.trim(), guide.moduleId);
    }
    assert.ok(guide.source.zh.trim() && guide.source.en.trim(), guide.moduleId);
    assert.ok(guide.references.length > 0, `${guide.moduleId} must retain its evidence`);
    for (const reference of guide.references) {
      const url = new URL(reference.url);
      assert.equal(url.protocol, "https:");
      assert.equal(url.username + url.password, "");
      assert.ok(reference.label.trim());
    }
  }
});

test("file import stays in maintenance and preserves the existing creation flow", () => {
  const maintenance = fs.readFileSync(path.join(__dirname, "../src/views/servers/ServerMaintenanceWorkspace.tsx"), "utf8");
  assert.match(maintenance, /id: "save-import"[\s\S]*preserveAfterVisit: true/);
  assert.equal((maintenance.match(/<SaveImportPanel\b/g) ?? []).length, 1);
  for (const file of fs.readdirSync(path.join(__dirname, "../src/views"))) {
    if (!/Create.*\.tsx$/.test(file)) continue;
    assert.doesNotMatch(fs.readFileSync(path.join(__dirname, "../src/views", file), "utf8"), /SaveImportPanel/);
  }
});
