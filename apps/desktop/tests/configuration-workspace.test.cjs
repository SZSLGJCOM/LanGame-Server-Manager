const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const desktopRoot = path.resolve(__dirname, "..");
const serversViewPath = path.join(desktopRoot, "src", "views", "ServersView.tsx");
const workspacePath = path.join(
  desktopRoot,
  "src",
  "views",
  "settings",
  "ConfigurationWorkspace.tsx"
);
const workspaceCssPath = path.join(desktopRoot, "src", "styles", "configuration-workspace.css");
const guidedSettingsCssPath = path.join(desktopRoot, "src", "styles", "settings-guided.css");
const serverLayoutCssPath = path.join(
  desktopRoot,
  "src",
  "views",
  "servers",
  "workbench",
  "operations",
  "server-layout.css"
);
const serversViewSource = fs.readFileSync(serversViewPath, "utf8");
const serverMaintenanceSource = fs.readFileSync(path.join(desktopRoot, "src", "views", "servers", "ServerMaintenanceWorkspace.tsx"), "utf8");
const archiveWorkspaceSource = fs.readFileSync(path.join(desktopRoot, "src", "views", "servers", "ArchivedInstanceWorkspace.tsx"), "utf8");
const workspaceSource = fs.readFileSync(workspacePath, "utf8");
const workspaceCssSource = fs.readFileSync(workspaceCssPath, "utf8");
const guidedSettingsCssSource = fs.readFileSync(guidedSettingsCssPath, "utf8");
const serverLayoutCssSource = fs.readFileSync(serverLayoutCssPath, "utf8");

for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = function compileTypeScript(module, filename) {
    const source = fs.readFileSync(filename, "utf8");
    module._compile(transpileTypeScript(source, filename), filename);
  };
}
require.extensions[".css"] = function compileEmptyCss(module) {
  module._compile("module.exports = {};", module.filename);
};
require.extensions[".png"] = function compileImageAsset(module, filename) {
  module._compile(`module.exports = ${JSON.stringify(filename)};`, filename);
};

function readRequired(filename, label) {
  assert.ok(fs.existsSync(filename), `${label} must exist`);
  return fs.readFileSync(filename, "utf8");
}

function loadWorkspaceApi() {
  readRequired(workspacePath, "ConfigurationWorkspace.tsx");
  return require(workspacePath);
}

function settingsMountBlock(source) {
  const start = source.indexOf('{activeDetailTab === "settings"');
  const end = source.indexOf("<ServerMaintenanceWorkspace", start);
  assert.ok(start >= 0 && end > start, "ServersView must keep a bounded Configuration tab block");
  return source.slice(start, end);
}

function cssRule(source, selector) {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return source.match(new RegExp(`${escaped}\\s*\\{([\\s\\S]*?)\\}`))?.[1] ?? "";
}

function assertMatches(source, pattern, message) {
  assert.ok(pattern.test(source), message);
}

function assertInteractiveMarker(source, marker) {
  const markerIndex = source.indexOf(marker);
  const buttonStart = source.lastIndexOf("<button", markerIndex);
  const buttonEnd = source.indexOf("</button>", markerIndex);
  assert.ok(markerIndex >= 0, `missing ${marker}`);
  assert.ok(buttonStart >= 0 && buttonEnd > markerIndex, `${marker} must belong to a button`);
  assert.ok(
    source.slice(buttonStart, buttonEnd).includes("onClick="),
    `${marker} must trigger navigation`
  );
}

function modelFixture() {
  const makeField = (key, sectionId, title) => ({
    key,
    title,
    sectionId,
    presentation: {
      state: "editable",
      owner: "configuration",
      sectionId
    }
  });
  const identityField = makeField("server_name", "identity", "Server name");
  const gameplayField = makeField("difficulty", "gameplay", "Difficulty");
  return {
    roots: [
      {
        id: "identity",
        title: "Identity",
        order: 10,
        breadcrumb: ["Identity"],
        actionable: true,
        children: [],
        items: [{
          sectionId: "identity",
          fieldKey: identityField.key,
          breadcrumb: ["Identity"],
          owner: "configuration",
          state: "editable",
          field: identityField
        }]
      },
      {
        id: "gameplay",
        title: "Gameplay",
        order: 20,
        breadcrumb: ["Gameplay"],
        actionable: true,
        children: [],
        items: [{
          sectionId: "gameplay",
          fieldKey: gameplayField.key,
          breadcrumb: ["Gameplay"],
          owner: "configuration",
          state: "editable",
          field: gameplayField
        }]
      }
    ],
    actionableSectionIds: ["identity", "gameplay"],
    items: [
      {
        sectionId: "identity",
        fieldKey: identityField.key,
        breadcrumb: ["Identity"],
        owner: "configuration",
        state: "editable",
        field: identityField
      },
      {
        sectionId: "gameplay",
        fieldKey: gameplayField.key,
        breadcrumb: ["Gameplay"],
        owner: "configuration",
        state: "editable",
        field: gameplayField
      }
    ]
  };
}

test("keeps the six instance tabs in their established order", () => {
  const { buildServerDetailTabSpecs } = require(path.join(desktopRoot, "src", "views", "servers", "server-detail-tab-specs.ts"));
  const tabs = buildServerDetailTabSpecs({ moduleId: "corekeeper", moduleDetails: null, t: (key) => key });
  const ids = tabs.map((tab) => tab.id);
  assert.deepEqual(ids, [
    "runtime",
    "settings",
    "mods",
    "players",
    "maintenance",
    "gm"
  ]);
  assert.deepEqual(tabs.map((tab) => tab.icon), ["terminal", "settings", "package", "users", "shield", "zap"]);
  assert.deepEqual(tabs.map((tab) => tab.label), [
    "servers.tabs.runtime", "servers.tabs.settings", "servers.tabs.mods",
    "servers.tabs.players", "servers.tabs.maintenance", "servers.tabs.gmTools"
  ]);
  assert.match(serversViewSource, /const detailTabs = buildServerDetailTabSpecs\(/);
});

test("archive tabs preserve module capabilities and require restoration for tools", () => {
  const { buildServerDetailTabSpecs } = require(path.join(desktopRoot, "src", "views", "servers", "server-detail-tab-specs.ts"));
  const t = (key) => key;
  const normal = buildServerDetailTabSpecs({ moduleId: "dontstarve", moduleDetails: null, t });
  const archived = buildServerDetailTabSpecs({ moduleId: "dontstarve", moduleDetails: null, t, archived: true });
  assert.equal(normal.find((tab) => tab.id === "mods").disabled, false);
  assert.equal(normal.find((tab) => tab.id === "gm").disabled, false);
  assert.deepEqual(archived.slice(0, 5), normal.slice(0, 5));
  assert.equal(archived.find((tab) => tab.id === "gm").disabled, true);
  assert.equal(archived.find((tab) => tab.id === "gm").disabledReason, "servers.archives.workspace.restoreForTools");
  const unsupported = buildServerDetailTabSpecs({ moduleId: "unsupported", moduleDetails: null, t, archived: true });
  assert.equal(unsupported.length, 6);
  assert.equal(unsupported.find((tab) => tab.id === "mods").disabled, true);
  assert.equal(unsupported.find((tab) => tab.id === "gm").disabledReason, "servers.gmTools.unavailable");
});

test("mounts the existing broadcast workbench at the bottom of Maintenance", () => {
  assert.match(serversViewSource, /<ServerMaintenanceWorkspace active=\{activeDetailTab === "maintenance"\}/);
  assert.match(serverMaintenanceSource, /<MaintenanceWorkspace active=\{props\.active\}/);
  assert.match(serverMaintenanceSource, /id: "storage"[\s\S]*broadcast=\{[\s\S]*<AiBroadcastWorkbench[\s\S]*active=\{props\.active\}/,
    "the shared broadcast workbench must follow Maintenance sections and share their activation");
  assert.equal((serverMaintenanceSource.match(/<AiBroadcastWorkbench/g) ?? []).length, 1);
  assert.doesNotMatch(serversViewSource, /id:\s*"(?:broadcast|map)"/);
  assert.doesNotMatch(serversViewSource, /WorldMapWorkbench|servers\.tabs\.worldMap/);
});

test("archives mount the same instance workspaces with saved data and read-only operation boundaries", () => {
  for (const component of ["ConfigurationWorkspace", "RuntimeSurfaceWorkbench", "ModWorkbench", "PlayerCenterWorkbench", "ServerMaintenanceWorkspace"]) {
    assert.match(serversViewSource, new RegExp(`<${component}\\b`), `normal instances must mount ${component}`);
    assert.match(archiveWorkspaceSource, new RegExp(`<${component}\\b`), `archives must reuse ${component}`);
  }
  assert.doesNotMatch(archiveWorkspaceSource, /ArchiveConfigurationPreview|ArchivedRuntimeData|ArchivedSavedSettings|ArchivedMaintenanceData|SavedConfigurationValue/);
  assert.doesNotMatch(archiveWorkspaceSource, /<header|archived-instance-workspace__header/);
  assert.match(serversViewSource, /requestedTab=\{requestedArchiveDetailTab\} onTabChange=\{setArchiveDetailTab\}/);
  assert.match(archiveWorkspaceSource, /requestedTab: ServerDetailTab/);
  assert.doesNotMatch(archiveWorkspaceSource, /useState|selection\.archiveId/);
  assert.match(archiveWorkspaceSource, /<ConfigurationWorkspace[\s\S]*?details=\{state\.details\.instance\} archive=\{state\.details\}/);
  assert.match(archiveWorkspaceSource, /<RuntimeSurfaceWorkbench[\s\S]*?archive=\{state\.details\}[\s\S]*?runtime=\{null\}/);
  assert.match(archiveWorkspaceSource, /<PlayerCenterWorkbench[\s\S]*?runtime=\{null\} readOnly/);
  assert.doesNotMatch(archiveWorkspaceSource, /onSave=|onSaveSettings=|onApplyPlayerAccessMutation=|onSendRuntimeCommand=|onCreateBackup=|onRestoreBackup=/);
  assert.match(workspaceSource, /enabled: !readOnly/);
  assert.match(workspaceSource, /function commitSettings[\s\S]*?if \(readOnly\) return;/);
  assert.match(workspaceSource, /if \(readOnly \|\| !parsed\.value \|\| !moduleDefinition\?\.initializeSettings\)/);
});

test("mounts one embedded ConfigurationWorkspace directly in the detail region", () => {
  const block = settingsMountBlock(serversViewSource);
  assertMatches(
    serversViewSource,
    /import\s*\{[^}]*\bConfigurationWorkspace\b[^}]*\}\s*from\s*["']\.\/settings\/ConfigurationWorkspace["']/,
    "ServersView must import ConfigurationWorkspace directly"
  );
  assertMatches(block, /<ConfigurationWorkspace\b/, "Configuration tab must mount ConfigurationWorkspace");
  for (const prop of [
    "details",
    "moduleDetails",
    "bindAddressCandidates",
    "runtime",
    "launchPlan",
    "launchPlanError",
    "onSave"
  ]) {
    assertMatches(block, new RegExp(`\\b${prop}=`), `missing ConfigurationWorkspace prop ${prop}`);
  }
  assert.doesNotMatch(serversViewSource, /SettingsModalRouter/);
  assert.doesNotMatch(block, /onPickDirectory|onImportDontStarveWorldData/);
  assert.doesNotMatch(block, /server-settings-inline|server-settings-shell|modal-overlay|runtime-box|\bonClose=/);
  assert.doesNotMatch(serverLayoutCssSource, /\.server-settings-shell\b/);
});

test("uses neutral layout sections instead of nesting a card around configuration details", () => {
  const sectionRule = cssRule(guidedSettingsCssSource, ".settings-schema-section");
  for (const property of ["padding", "border", "border-radius", "background", "box-shadow"]) {
    assert.doesNotMatch(
      sectionRule,
      new RegExp(`(?:^|\\n)\\s*${property.replace("-", "\\-")}\\s*:`),
      `configuration detail sections must not add decorative ${property}`
    );
  }
  assert.match(sectionRule, /display:\s*grid/, "semantic sections must retain their layout role");
});

test("keeps navigation and configuration details in independent scroll regions", () => {
  const workspaceSource = readRequired(workspacePath, "ConfigurationWorkspace.tsx");
  const workspaceCss = readRequired(workspaceCssPath, "configuration-workspace.css");
  const serverLayoutCss = readRequired(serverLayoutCssPath, "server-layout.css");
  assert.equal(
    workspaceSource.match(/data-configuration-scroll-owner/g)?.length ?? 0,
    1,
    "ConfigurationWorkspace must identify its right-hand content scroll owner"
  );
  assert.match(
    workspaceSource,
    /<main[^>]*data-configuration-scroll-owner/,
    "the right-hand details pane must own content scrolling"
  );
  assert.doesNotMatch(
    workspaceSource,
    /configuration-workspace__body[^>]*data-configuration-scroll-owner/,
    "the two-column body must not scroll both panes together"
  );
  assertMatches(
    cssRule(workspaceCss, "[data-configuration-scroll-owner]"),
    /overflow-y:\s*auto/,
    "the details pane must scroll vertically"
  );
  assertMatches(
    cssRule(workspaceCss, ".configuration-workspace__sidebar"),
    /overflow-y:\s*auto/,
    "long game navigation must have its own vertical scrolling"
  );
  assertMatches(
    cssRule(workspaceCss, ".configuration-workspace__body"),
    /overflow:\s*hidden/,
    "the two-column body must only lay out the two scroll panes"
  );
  assertMatches(
    cssRule(serverLayoutCss, ".server-detail-scroll--settings"),
    /overflow:\s*hidden/,
    "outer Configuration tab shell must not own scrolling"
  );
});

test("starts directly with categories and settings without a nested workspace header", () => {
  assert.doesNotMatch(workspaceSource, /<header\s+className="configuration-workspace__header"/);
  assert.doesNotMatch(workspaceSource, /settings\.configuration\.workspace\.(?:eyebrow|subtitle|searchPlaceholder)/);
  assert.doesNotMatch(
    workspaceSource,
    /configuration-workspace__(?:section-head|section-title|eyebrow)|activeNode\.(?:breadcrumb|title)/,
    "the selected sidebar label must not be repeated in the details pane"
  );
  assert.doesNotMatch(
    workspaceCssSource,
    /\.configuration-workspace__(?:section-head|section-title|eyebrow)\b/,
    "removed details-pane headings must not leave orphaned styles"
  );
  assert.match(workspaceSource, /<ConfigurationSearchNavigation\s+model=\{model\}/);
  assert.doesNotMatch(workspaceSource, /settings\.configuration\.save\.(?:saved|dirty|saving)/);
  assertMatches(
    cssRule(workspaceCssSource, ".configuration-workspace"),
    /grid-template-rows:\s*minmax\(0,\s*1fr\)/,
    "header-free workspace must give its only row to the configuration body"
  );
  assert.doesNotMatch(
    cssRule(workspaceCssSource, ".configuration-workspace"),
    /grid-template-rows:\s*auto/,
    "removed header must not leave an empty auto row"
  );
});

test("selects through the first-actionable resolver instead of a room default", () => {
  const source = readRequired(workspacePath, "ConfigurationWorkspace.tsx");
  assertMatches(
    source,
    /resolveConfigurationSectionId\(/,
    "ConfigurationWorkspace must use the first-actionable selection resolver"
  );
  assert.doesNotMatch(source, /useState(?:<[^>]+>)?\(\s*["']room["']\s*\)/);
});

test("keeps internal configuration provenance out of the player workspace", () => {
  const guidedSettingsSource = readRequired(
    path.join(desktopRoot, "src", "views", "settings", "GuidedSettingsForm.tsx"),
    "GuidedSettingsForm.tsx"
  );
  const englishMessages = readRequired(
    path.join(desktopRoot, "src", "i18n-messages.ts"),
    "English messages"
  );
  const chineseMessages = readRequired(
    path.join(desktopRoot, "src", "i18n-messages-zh-settings.ts"),
    "Chinese settings messages"
  );

  assert.doesNotMatch(workspaceSource, /item\.field\.sourceKey/);
  assert.doesNotMatch(workspaceSource, /<code>\{item\.field\.sourceKey\}<\/code>/);
  assert.doesNotMatch(workspaceSource, /item\.field\.presentation\.reason/);
  assert.doesNotMatch(workspaceSource, /activeNode\.description/);
  assert.doesNotMatch(guidedSettingsSource, /group\.description/);
  assert.doesNotMatch(guidedSettingsSource, /section\.description/);
  assert.doesNotMatch(workspaceSource, /settings\.configuration\.workspace\.unavailable\./);
  assert.doesNotMatch(workspaceSource, /configuration-workspace__advanced-help/);
  assert.doesNotMatch(workspaceSource, /settings\.configuration\.workspace\.generatedHelp/);
  assert.doesNotMatch(englishMessages, /settings\.configuration\.workspace\.generatedHelp/);
  assert.doesNotMatch(chineseMessages, /settings\.configuration\.workspace\.generatedHelp/);
});

test("resolves validation targets to a section and stable focus control", () => {
  const {
    focusConfigurationControl,
    resolveConfigurationFieldNavigation
  } = loadWorkspaceApi();
  const model = modelFixture();
  const validationIssue = { fieldKey: "server_name", message: "Required" };
  assert.deepEqual(
    resolveConfigurationFieldNavigation(model, validationIssue.fieldKey, "configuration-demo"),
    {
      sectionId: "identity",
      inputId: "configuration-demo-server-name-input"
    }
  );
  assert.equal(resolveConfigurationFieldNavigation(model, "missing", "configuration-demo"), null);

  let focused = 0;
  const root = {
    getElementById(inputId) {
      return inputId === "configuration-demo-server-name-input"
        ? { focus: () => { focused += 1; } }
        : null;
    }
  };
  assert.equal(
    focusConfigurationControl("configuration-demo-server-name-input", root),
    true
  );
  assert.equal(focused, 1);
  assert.equal(focusConfigurationControl("configuration-demo-missing-input", root), false);
});

test("wires validation issues into focusable navigation", () => {
  const source = readRequired(workspacePath, "ConfigurationWorkspace.tsx");
  assertInteractiveMarker(source, "data-configuration-validation-field");
  assert.ok(
    (source.match(/resolveConfigurationFieldNavigation\(/g)?.length ?? 0) >= 1,
    "workspace must call the shared field-navigation resolver"
  );
  assert.ok(
    (source.match(/focusConfigurationControl\(/g)?.length ?? 0) >= 1,
    "workspace must focus a control after navigation"
  );
});

test("merges patches into the current draft without dropping unrelated native keys", () => {
  const { mergeConfigurationPatch } = loadWorkspaceApi();
  const nested = { preserve: true };
  const current = {
    server_name: "Lan room",
    max_players: 8,
    native_extension: "keep-me",
    nested
  };
  const next = mergeConfigurationPatch(current, { max_players: 16, motd: "Welcome" });
  assert.deepEqual(next, {
    server_name: "Lan room",
    max_players: 16,
    native_extension: "keep-me",
    nested,
    motd: "Welcome"
  });
  assert.notStrictEqual(next, current);
  assert.strictEqual(next.nested, nested);
});

test("keeps network in Configuration and autostart in Maintenance", () => {
  const source = readRequired(workspacePath, "ConfigurationWorkspace.tsx");
  assertMatches(source, /InstanceConnectionSettingsPanel/, "missing built-in network editor");
  assert.doesNotMatch(source, /InstanceRuntimeSettingsPanel/, "autostart belongs to Maintenance");
  assertMatches(source, /instance-network/, "network model node is not handled");
  assert.doesNotMatch(source, /instance-runtime/, "native runtime settings remain schema-owned");
});

test("connection address select has one global arrow style owner", () => {
  const connectionSelect = readRequired(
    path.join(desktopRoot, "src", "views", "settings", "PlayerJoinAddressSelect.tsx"),
    "PlayerJoinAddressSelect.tsx"
  );
  const appCss = readRequired(path.join(desktopRoot, "src", "app.css"), "app.css");
  const selectCss = readRequired(path.join(desktopRoot, "src", "styles", "select-control.css"), "select-control.css");
  const guidedCss = readRequired(path.join(desktopRoot, "src", "styles", "settings-guided.css"), "settings-guided.css");
  const overlaysCss = readRequired(path.join(desktopRoot, "src", "styles", "overlays.css"), "overlays.css");

  assertMatches(connectionSelect, /settings-schema-select player-join-address__select/, "connection address must use the global select style");
  assertMatches(appCss, /@import "\.\/styles\/select-control\.css";/, "global select controls must be imported once");
  assertMatches(selectCss, /background-image: var\(--select-control-arrow\) !important;/, "global select control must retain its arrow image");
  assertMatches(selectCss, /background-repeat: no-repeat !important;/, "global select control must prevent page backgrounds from repeating its arrow");
  assert.doesNotMatch(guidedCss, /select\.settings-schema-input\s*\{/, "guided settings must not redraw the global select arrow");
  assert.doesNotMatch(overlaysCss, /select\.settings-schema-select/, "overlays must not redraw the global select arrow");
});

test("owns loading, disabled, empty, and error states locally", () => {
  const source = readRequired(workspacePath, "ConfigurationWorkspace.tsx");
  for (const state of ["loading", "disabled", "empty"]) {
    assertMatches(
      source,
      new RegExp(`data-configuration-state=[{]?["']${state}["']`),
      `missing local ${state} state`
    );
  }
  assertMatches(source, /if \(workspaceError\)\s*\{\s*return <ConfigurationLoadError error=\{workspaceError\}/,
    "workspace must route its actual error to its local recovery component");
  const errorSource = readRequired(path.join(path.dirname(workspacePath), "ConfigurationLoadError.tsx"), "ConfigurationLoadError.tsx");
  assertMatches(errorSource, /data-configuration-state="error"/, "missing local error state");
  assertMatches(errorSource, /<pre[^>]*>\{error\}<\/pre>/, "recovery must preserve the actual diagnostic");
  assertMatches(source, /onRetry=\{props\.moduleDetailsError \? props\.onRetryModuleDetails : undefined\}/,
    "metadata read failures must retain an explicit retry");
  const block = settingsMountBlock(serversViewSource);
  assert.doesNotMatch(block, /data-configuration-state=/);
});

test("keeps autosave active with progress feedback and retryable failures", () => {
  const source = readRequired(workspacePath, "ConfigurationWorkspace.tsx");
  assertMatches(source, /useAutoSaveInstanceSettings\(/, "workspace must use shared autosave");
  for (const value of [
    "details: props.details",
    "bindIp",
    "settingsJson",
    "ports",
    "disabled: saveBlocked",
    "onSave: props.onSave"
  ]) {
    assertMatches(source, new RegExp(value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")), `missing autosave input ${value}`);
  }
  assertMatches(source, /saveStatus\.state === "failed"/, "save failures must remain visible");
  assertMatches(source, /saveStatus\.state === "conflict"/, "save conflicts must remain visible");
  assertMatches(source, /onClick=\{retrySave\}/, "save failures must remain retryable");
  assertMatches(source, /<ConfigurationSaveStatus status=\{saveStatus\} validationBlocked=\{validationBlocked\}/,
    "saved, pending and invalid settings must remain distinguishable");
});

test("feeds module-level validation into the blocking summary and autosave gate", () => {
  assertMatches(
    workspaceSource,
    /getSettingsValidationIssues\?\.\(settingsParseResult\.value,\s*moduleContext\)/,
    "module-level settings validation must execute"
  );
  assertMatches(
    workspaceSource,
    /uniqueIssues\(\[\s*\.\.\.schemaIssues,\s*\.\.\.moduleIssues\s*\]\)/,
    "schema and module issues must share one summary"
  );
  assertMatches(
    workspaceSource,
    /const validationBlocked = validationIssues\.length > 0 \|\| networkValidationBlocked/,
    "module validation must block autosave"
  );
  assertMatches(workspaceSource, /const saveBlocked = editorDisabled \|\| validationBlocked/,
    "schema and local port validation must share the autosave gate");
  assertMatches(workspaceSource, /onValidationBlockedChange=\{setNetworkValidationBlocked\}/,
    "unfinished network drafts must reach the workspace validation gate");
  assertMatches(
    workspaceSource,
    /validationIssues\.map\([\s\S]{0,260}data-configuration-validation-field/,
    "the visible summary must enumerate the combined validation issues"
  );
});

test("returns the mismatch state before any foreign schema controls render", () => {
  assertMatches(
    workspaceSource,
    /localizedModuleDetails\.summary\.id !== moduleId/,
    "module mismatch must compare the loaded and selected module IDs"
  );
  const mismatchBranch = workspaceSource.indexOf("if (moduleMismatch)");
  const guidedForm = workspaceSource.indexOf("<GuidedSettingsForm");
  assert.ok(mismatchBranch >= 0 && guidedForm > mismatchBranch);
  assertMatches(
    workspaceSource.slice(mismatchBranch, guidedForm),
    /return[\s\S]*data-configuration-state="disabled"/,
    "mismatch must return locally before game fields"
  );
});

test("does not render the empty guided-schema notice over built-in sections", () => {
  assertMatches(
    workspaceSource,
    /activeGuidedFields\.length\s*>\s*0\s*\?\s*\(\s*<GuidedSettingsForm/,
    "the guided form must only render when the active section owns guided fields"
  );
});

test("removes modal-inline CSS without coupling the two workspace scroll panes", () => {
  assert.doesNotMatch(workspaceCssSource, /server-settings-inline|\.modal-(?:overlay|content)/);
  assert.doesNotMatch(serverLayoutCssSource, /server-settings-inline/);
  assert.match(
    cssRule(workspaceCssSource, ".configuration-workspace__sidebar"),
    /overflow-y:\s*auto/,
    "sidebar must remain usable for long ARK and SCUM navigation trees"
  );
});
