const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");

const desktopRoot = path.resolve(__dirname, "..");

function readSource(...segments) {
  return fs.readFileSync(path.join(desktopRoot, ...segments), "utf8");
}

const moduleTypesSource = readSource("src", "views", "settings", "module-types.ts");
const registrySource = readSource("src", "views", "settings", "module-registry.ts");
const presentationSource = readSource("src", "views", "settings", "configuration-presentation.ts");
const workspaceSource = readSource("src", "views", "settings", "ConfigurationWorkspace.tsx");
const dstDefinitionSource = readSource("src", "views", "settings", "modules", "dontstarve.ts");
const sevenDaysDefinitionSource = readSource("src", "views", "settings", "modules", "sevendaystodie.ts");
const sevenDaysPanelSource = readSource("src", "views", "settings", "SevenDaysServerAdminPanel.tsx");

test("defines one patch-only specialized renderer interface and a typed registration", () => {
  assert.match(moduleTypesSource, /interface ConfigurationSpecializedRendererProps\s*\{/);
  for (const member of ["sectionId", "fieldKey", "details", "moduleDetails", "settings", "disabled", "onPatch"]) {
    assert.match(moduleTypesSource, new RegExp(`\\b${member}\\??:`), member);
  }
  assert.match(moduleTypesSource, /onPatch:\s*\(patch:\s*Readonly<SettingsObject>\)\s*=>\s*void/);
  assert.match(moduleTypesSource, /interface ConfigurationSpecializedRendererRegistration/);
  assert.match(moduleTypesSource, /Renderer:\s*ComponentType<ConfigurationSpecializedRendererProps>/);
  assert.doesNotMatch(moduleTypesSource, /SettingsModuleAddonProps|AddonPanel|operations:/);
});

test("registers only configuration editors and notices in module definitions", () => {
  assert.doesNotMatch(dstDefinitionSource, /dst-world-bootstrap|DstWorldBootstrap/);
  assert.match(dstDefinitionSource, /"dst-mastergen-preset-notice"\s*:\s*\{/);

  assert.match(sevenDaysDefinitionSource, /"seven-days-command-permissions"\s*:\s*\{/);
  assert.match(sevenDaysDefinitionSource, /sectionId:\s*"access"/);
  assert.match(sevenDaysDefinitionSource, /fieldKey:\s*"command_permissions"/);
  assert.match(sevenDaysDefinitionSource, /Renderer:\s*SevenDaysServerAdminPanel/);
  assert.match(sevenDaysDefinitionSource, /command_permissions:[\s\S]{0,220}rendererId:\s*"seven-days-command-permissions"/);
});

test("registry resolves only concrete renderers for the requested section", () => {
  assert.match(registrySource, /export function listConfigurationSpecializedRenderers/);
  assert.match(registrySource, /registration\.sectionId !== sectionId/);
  assert.match(registrySource, /typeof registration\.Renderer !== "function"/);
  assert.match(presentationSource, /renderer \$\{rendererId\} references unknown section/);
  assert.match(presentationSource, /renderer \$\{rendererId\} does not own specialized field/);
});

test("ConfigurationWorkspace renders registry entries without game or renderer IDs", () => {
  assert.match(workspaceSource, /listConfigurationSpecializedRenderers\([\s\S]{0,100}moduleDefinition,[\s\S]{0,50}selectedSectionId/);
  assert.match(workspaceSource, /specializedRenderers\.map\(\(registration\)/);
  assert.match(workspaceSource, /<registration\.Renderer/);
  assert.match(workspaceSource, /onPatch=\{\(patch\) => commitSettings\(\(current\) => applyPatch\(current, patch\)\)\}/);
  const rendererStart = workspaceSource.indexOf("const renderSpecializedRenderers =");
  const rendererEnd = workspaceSource.indexOf("\n  const workspace = (", rendererStart);
  assert.ok(rendererStart >= 0 && rendererEnd > rendererStart);
  assert.doesNotMatch(workspaceSource.slice(rendererStart, rendererEnd), /moduleId === "dontstarve"/,
    "specialized renderer hosting stays generic even when a workspace notice is game-specific");
  assert.doesNotMatch(workspaceSource, /<DstWorldBootstrapPanel/);
  assert.doesNotMatch(workspaceSource, /ModuleAddonPanel|AddonPanel/);
});

test("specialized notices render before fields while unspecified add-ons retain their position", () => {
  assert.match(moduleTypesSource, /placement\?: "before-fields" \| "after-fields"/);
  assert.match(workspaceSource, /\(registration\.placement \?\? "after-fields"\) === placement/);
  const before = workspaceSource.indexOf('{renderSpecializedRenderers("before-fields")}');
  const fields = workspaceSource.indexOf("<GuidedSettingsForm");
  const after = workspaceSource.indexOf('{renderSpecializedRenderers("after-fields")}');
  assert.ok(before >= 0 && before < fields && fields < after);
  for (const placement of ["before-fields", "after-fields"]) {
    assert.equal(workspaceSource.split(`{renderSpecializedRenderers("${placement}")}`).length - 1, 1);
  }
});

test("focused renderers retain patch boundaries without world import capabilities", () => {
  assert.doesNotMatch(workspaceSource, /ConfigurationRendererOperations|onImportDontStarveWorldData/);
  assert.match(sevenDaysPanelSource, /props\.onPatch\(\{\s*command_permissions:\s*nextEntries\s*\}\)/);
  assert.doesNotMatch(sevenDaysPanelSource, /onSettingsChange|\.\.\.props\.settings|\.\.\.settings/);
});
