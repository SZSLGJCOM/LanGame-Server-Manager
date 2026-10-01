const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");

const desktopRoot = path.resolve(__dirname, "..");
const moduleStyles = fs.readFileSync(
  path.join(desktopRoot, "src", "styles", "module-settings.css"),
  "utf8"
);
const legacyModalStylesPath = path.join(desktopRoot, "src", "styles", "modal", "base.css");

test("Configuration owns focused 7DTD layouts while DST import styles belong to Maintenance", () => {
  for (const selector of [
    ".configuration-workspace .sevendays-admin-rows",
    ".configuration-workspace .sevendays-admin-row",
    ".configuration-workspace .sevendays-admin-alert"
  ]) {
    assert.ok(moduleStyles.includes(selector), `missing ${selector}`);
  }
  assert.doesNotMatch(moduleStyles, /dst-bootstrap|dst-world-import/);
  const maintenanceStyles = fs.readFileSync(path.join(desktopRoot,
    "src/views/servers/workbench/operations/maintenance.css"), "utf8");
  assert.match(maintenanceStyles, /\.dst-world-import-feedback\.is-error/);
});

test("legacy modal shell styles are removed instead of retained as a parallel design system", () => {
  assert.equal(fs.existsSync(legacyModalStylesPath), false);
  assert.doesNotMatch(moduleStyles, /\.modal-content/);
});
