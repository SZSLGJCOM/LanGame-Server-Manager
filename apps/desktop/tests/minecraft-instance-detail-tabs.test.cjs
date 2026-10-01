const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { parseSource, sourceText, visitSyntax } = require("../scripts/typescript_source_tools.cjs");

const repoRoot = path.resolve(__dirname, "../../..");
const desktopRoot = path.join(repoRoot, "apps", "desktop");
const serversViewPath = path.join(desktopRoot, "src", "views", "ServersView.tsx");
const configurationWorkspacePath = path.join(desktopRoot, "src", "views", "settings", "ConfigurationWorkspace.tsx");
const settingsRegistryPath = path.join(desktopRoot, "src", "views", "settings", "module-registry.ts");
const minecraftPanelsPath = path.join(desktopRoot, "src", "views", "servers", "MinecraftDetailPanels.tsx");
const minecraftModelPath = path.join(desktopRoot, "src", "views", "servers", "minecraft-detail-model.ts");
const minecraftManifestPath = path.join(repoRoot, "modules", "minecraft", "module.toml");
const maintenanceWorkspacePath = path.join(desktopRoot, "src", "views", "servers", "ServerMaintenanceWorkspace.tsx");

const serversView = fs.readFileSync(serversViewPath, "utf8");
const configurationWorkspace = fs.readFileSync(configurationWorkspacePath, "utf8");
const settingsRegistry = fs.readFileSync(settingsRegistryPath, "utf8");
const minecraftManifest = fs.readFileSync(minecraftManifestPath, "utf8");
const maintenanceWorkspace = fs.readFileSync(maintenanceWorkspacePath, "utf8");

function importedComponentProps(source, filename, owner, component, importPath) {
  const syntax = parseSource(source, filename);
  const imported = syntax.body.find((node) => node.type === "ImportDeclaration" && node.source.value === importPath);
  assert.ok(imported?.specifiers.some((specifier) => specifier.type === "ImportSpecifier"
    && specifier.local.value === component && (specifier.imported?.value ?? component) === component),
  `${owner} imports the actual ${component} implementation`);
  const declaration = syntax.body.find((node) => node.type === "ExportDeclaration"
    && node.declaration.type === "FunctionDeclaration" && node.declaration.identifier.value === owner)?.declaration;
  assert.ok(declaration, `${owner} remains the rendered exported component`);
  const calls = [];
  visitSyntax(declaration.body, (node) => {
    if (node.type === "JSXOpeningElement" && node.name.type === "Identifier" && node.name.value === component) calls.push(node);
  });
  assert.equal(calls.length, 1, `${owner} renders exactly one ${component}`);
  return Object.fromEntries(calls[0].attributes.map((attribute) => {
    assert.equal(attribute.type, "JSXAttribute", `${component} keeps explicit prop ownership`);
    assert.equal(attribute.value?.type, "JSXExpressionContainer");
    return [attribute.name.value, sourceText(source, attribute.value.expression).replace(/\s+/g, "")];
  }));
}

test("Minecraft instance tabs use the same shared workbenches as other games", () => {
  const sharedTabs = [
    ["runtime", "RuntimeSurfaceWorkbench"],
    ["settings", "ConfigurationWorkspace"],
    ["mods", "ModWorkbench"],
    ["players", "PlayerCenterWorkbench"],
    ["gm", "GMToolsWorkbench"],
  ];

  for (const [tab, workbench] of sharedTabs) {
    assert.match(serversView, new RegExp(`activeDetailTab === "${tab}"[\\s\\S]*?<${workbench}`));
  }

  const maintenanceProps = importedComponentProps(serversView, serversViewPath,
    "ServersView", "ServerMaintenanceWorkspace", "./servers/ServerMaintenanceWorkspace");
  for (const [key, expression] of Object.entries({ active: 'activeDetailTab==="maintenance"',
    details: "props.selectedDetails", moduleDetails: "props.selectedModuleDetails",
    aiSettings: "props.aiSettings", assistantCanRun: "props.assistantCanRun", runtime: "props.runtime" })) {
    assert.equal(maintenanceProps[key], expression, `ServersView forwards ${key} to Maintenance`);
  }
  const broadcastProps = importedComponentProps(maintenanceWorkspace, maintenanceWorkspacePath,
    "ServerMaintenanceWorkspace", "AiBroadcastWorkbench", "./AiBroadcastWorkbench");
  for (const [key, expression] of Object.entries({ active: "props.active", details: "props.details",
    moduleDetails: "props.moduleDetails", aiSettings: "props.aiSettings",
    assistantCanRun: "Boolean(props.assistantCanRun)", runtime: "props.runtime??null" })) {
    assert.equal(broadcastProps[key], expression, `Maintenance forwards ${key} to Broadcast`);
  }
  assert.doesNotMatch(
    serversView,
    /minecraftDetailModel|MinecraftDetailPanels|Minecraft[A-Za-z]+(?:Context)?Panel|server-detail-scroll--minecraft-context/
  );
});

test("Minecraft no longer carries a parallel instance-detail implementation", () => {
  assert.equal(fs.existsSync(minecraftPanelsPath), false);
  assert.equal(fs.existsSync(minecraftModelPath), false);
});

test("Minecraft configuration uses the shared schema workspace", () => {
  assert.match(settingsRegistry, /minecraftSettingsDefinition/);
  assert.match(configurationWorkspace, /parseGuidedSettingsSchema\(localizedModuleDetails, locale, t, \{ fieldIcons: configurationIcons\.icons \}\)/);
  assert.match(configurationWorkspace, /resolveSettingsModuleDefinition\(moduleId\)/);
  assert.doesNotMatch(configurationWorkspace, /SettingsModal|onClose/);
});

test("Minecraft-specific behavior remains declarative in the module manifest", () => {
  assert.match(minecraftManifest, /source = "minecraft_java"/);
  assert.match(minecraftManifest, /host_surface = "managed_terminal"/);
  assert.match(minecraftManifest, /\[runtime\.player_query\][\s\S]*?protocol = "minecraft_query"/);
  assert.match(minecraftManifest, /\[runtime\.shutdown\][\s\S]*?grace_period_ms = 8000/);
  assert.match(minecraftManifest, /\[storage\][\s\S]*?saves_path_template = "\{\{paths\.instance_root\}\}\/\{\{settings\.level_name\}\}"/);
});
