const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = function compileTypeScript(module, filename) {
    const source = fs.readFileSync(filename, "utf8");
    module._compile(transpileTypeScript(source, filename), filename);
  };
}

const desktopRoot = path.resolve(__dirname, "..");
const navigationPath = path.join(
  desktopRoot,
  "src",
  "views",
  "settings",
  "ConfigurationSectionNavigation.tsx"
);
const navigationSource = fs.readFileSync(navigationPath, "utf8");
const {
  collectConfigurationNavigationAncestors,
  reconcileConfigurationNavigationExpansion,
  toggleConfigurationNavigationExpansion
} = require(navigationPath);

function node(id, options = {}) {
  return {
    id,
    title: options.title ?? id,
    order: options.order ?? 0,
    breadcrumb: options.breadcrumb ?? [options.title ?? id],
    items: [],
    children: options.children ?? [],
    actionable: options.actionable ?? false
  };
}

const roots = [
  node("room", { actionable: true }),
  node("world", {
    children: [
      node("rules", { actionable: true }),
      node("breeding", {
        children: [node("imprinting", { actionable: true })]
      })
    ]
  })
];

test("expands every ancestor of the selected configuration section", () => {
  assert.deepEqual(
    [...collectConfigurationNavigationAncestors(roots, "imprinting")],
    ["world", "breeding"]
  );
  assert.deepEqual(
    [...reconcileConfigurationNavigationExpansion(roots, new Set(), "imprinting")],
    ["world", "breeding"]
  );
});

test("toggles a branch without changing unrelated expansion state", () => {
  const expanded = new Set(["world", "breeding"]);
  assert.deepEqual(
    [...toggleConfigurationNavigationExpansion(expanded, "world")],
    ["breeding"]
  );
  assert.deepEqual(
    [...toggleConfigurationNavigationExpansion(expanded, "imprinting")],
    ["world", "breeding", "imprinting"]
  );
  assert.deepEqual([...expanded], ["world", "breeding"], "toggle must be immutable");
});

test("drops stale branch ids when a different game hierarchy loads", () => {
  assert.deepEqual(
    [...reconcileConfigurationNavigationExpansion(
      [node("cluster", { children: [node("master", { actionable: true })] })],
      new Set(["world", "breeding"]),
      "master"
    )],
    ["cluster"]
  );
});

test("wires parent branches as real disclosure buttons", () => {
  assert.match(navigationSource, /aria-expanded=\{expanded\}/);
  assert.match(navigationSource, /aria-controls=\{childListId\}/);
  assert.match(navigationSource, /onClick=\{toggleNode\}/);
  assert.match(navigationSource, /expanded\s*\?\s*\(\s*<ul/);
  assert.doesNotMatch(
    navigationSource,
    /hasChildren\s*\?\s*\(\s*<ul/,
    "children must not remain permanently expanded"
  );
});

test("keeps selection and disclosure as separate actions for future actionable parents", () => {
  assert.match(navigationSource, /onClick=\{selectNode\}/);
  assert.match(navigationSource, /configuration-section-navigation__disclosure/);
  assert.match(navigationSource, /activeAncestor\s*\?\s*"is-active-ancestor"/);
});
