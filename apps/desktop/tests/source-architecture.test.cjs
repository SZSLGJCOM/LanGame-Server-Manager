const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");

const repoRoot = path.resolve(__dirname, "..", "..", "..");
const serverWorkbenchCssFacadePath = path.join(
  repoRoot,
  "apps",
  "desktop",
  "src",
  "views",
  "servers",
  "workbench",
  "operations.css"
);
const serverWorkbenchCssModuleDir = path.join(
  repoRoot,
  "apps",
  "desktop",
  "src",
  "views",
  "servers",
  "workbench",
  "operations"
);
const serverWorkbenchRootFacadePath = path.join(
  repoRoot,
  "apps",
  "desktop",
  "src",
  "views",
  "servers",
  "workbench.css"
);
const apiMockFacadePath = path.join(repoRoot, "apps", "desktop", "src", "api-mock.ts");
const apiMockModuleDir = path.join(repoRoot, "apps", "desktop", "src", "api-mock");
const instanceThemeDir = path.join(repoRoot, "apps", "desktop", "src", "views", "servers");
const instanceThemeFacadePath = path.join(instanceThemeDir, "instance-themes.css");
const desktopSourceDir = path.join(repoRoot, "apps", "desktop", "src");

function lineCount(filePath) {
  return fs.readFileSync(filePath, "utf8").split(/\r?\n/).length;
}

function cssSourceFiles(dirPath) {
  if (!fs.existsSync(dirPath)) {
    return [];
  }

  return fs
    .readdirSync(dirPath, { withFileTypes: true })
    .flatMap((entry) => {
      const entryPath = path.join(dirPath, entry.name);
      if (entry.isDirectory()) {
        return cssSourceFiles(entryPath);
      }
      return entry.isFile() && entry.name.endsWith(".css") ? [entryPath] : [];
    });
}

function typescriptSourceFiles(dirPath) {
  if (!fs.existsSync(dirPath)) {
    return [];
  }

  return fs
    .readdirSync(dirPath, { withFileTypes: true })
    .flatMap((entry) => {
      const entryPath = path.join(dirPath, entry.name);
      if (entry.isDirectory()) {
        return typescriptSourceFiles(entryPath);
      }
      return entry.isFile() && entry.name.endsWith(".ts") ? [entryPath] : [];
    });
}

test("server workbench CSS is split into maintainable source files", () => {
  assert.ok(fs.existsSync(serverWorkbenchCssFacadePath), "server workbench CSS facade is missing");
  assert.ok(fs.existsSync(serverWorkbenchCssModuleDir), "server workbench CSS modules are missing");
  assert.ok(fs.existsSync(serverWorkbenchRootFacadePath), "server workbench root CSS facade is missing");

  const maxFacadeLines = 80;
  const maxModuleLines = 2000;
  const expectedImports = [
    "./operations/base.css",
    "./operations/maintenance.css",
    "./operations/server-layout.css",
    "./operations/server-runtime.css",
    "./operations/gm-tools.css"
  ];
  const facadeSource = fs.readFileSync(serverWorkbenchCssFacadePath, "utf8");
  const rootFacadeSource = fs.readFileSync(serverWorkbenchRootFacadePath, "utf8");
  const violations = [];

  const facadeLines = lineCount(serverWorkbenchCssFacadePath);
  if (facadeLines > maxFacadeLines) {
    violations.push(`operations.css has ${facadeLines} lines; expected <= ${maxFacadeLines}`);
  }

  for (const importPath of expectedImports) {
    if (!facadeSource.includes(`@import "${importPath}";`)) {
      violations.push(`operations.css is missing ${importPath}`);
    }
  }

  const maintenanceSource = fs.readFileSync(path.join(serverWorkbenchCssModuleDir, "maintenance.css"), "utf8");
  assert.match(maintenanceSource, /@import "\.\/maintenance-broadcast\.css";/);
  assert.ok(fs.existsSync(path.join(serverWorkbenchCssModuleDir, "maintenance-broadcast.css")), "maintenance broadcast stylesheet is missing");

  if (!rootFacadeSource.includes('@import "./workbench/mods.css";')) {
    violations.push("workbench.css is missing ./workbench/mods.css");
  }

  for (const filePath of cssSourceFiles(serverWorkbenchCssModuleDir)) {
    const lines = lineCount(filePath);
    if (lines > maxModuleLines) {
      violations.push(`${path.relative(repoRoot, filePath)} has ${lines} lines; expected <= ${maxModuleLines}`);
    }
  }

  assert.deepEqual(violations, []);
});

test("mock API is split into maintainable source files", () => {
  assert.ok(fs.existsSync(apiMockFacadePath), "mock API facade is missing");
  assert.ok(fs.existsSync(apiMockModuleDir), "mock API modules are missing");

  const maxFileLines = 3000;
  const violations = [];
  const files = [apiMockFacadePath, ...typescriptSourceFiles(apiMockModuleDir)];

  for (const filePath of files) {
    const lines = lineCount(filePath);
    if (lines > maxFileLines) {
      violations.push(`${path.relative(repoRoot, filePath)} has ${lines} lines; expected <= ${maxFileLines}`);
    }
  }

  assert.deepEqual(violations, []);
});

test("the instance-theme facade imports only implemented stylesheets", () => {
  const source = fs.readFileSync(instanceThemeFacadePath, "utf8");
  const retiredThemes = [
    "arksurvivalevolved",
    "enshrouded",
    "projectzomboid",
    "sevendaystodie",
    "terraria",
    "vrising"
  ];

  for (const theme of retiredThemes) {
    assert.ok(!source.includes(`./${theme}.css`), `${theme}.css is still imported`);
    assert.ok(!fs.existsSync(path.join(instanceThemeDir, `${theme}.css`)), `${theme}.css is still a placeholder file`);
  }

  for (const match of source.matchAll(/@import "([^"]+)";/g)) {
    assert.ok(fs.existsSync(path.resolve(instanceThemeDir, match[1])), `${match[1]} does not exist`);
  }
});

test("desktop styles do not restore retired shell selectors", () => {
  const retiredSelectorPattern =
    /\.shell-(?:sidebar|search--compact|search)(?![-_A-Za-z0-9])|\.shell-frame\.is-collapsed(?![-_A-Za-z0-9])/g;
  const violations = [];

  for (const filePath of cssSourceFiles(desktopSourceDir)) {
    const source = fs.readFileSync(filePath, "utf8");
    for (const match of source.matchAll(retiredSelectorPattern)) {
      const line = source.slice(0, match.index).split(/\r?\n/).length;
      violations.push(`${path.relative(repoRoot, filePath)}:${line}: ${match[0]}`);
    }
  }

  assert.deepEqual(violations, []);
});
