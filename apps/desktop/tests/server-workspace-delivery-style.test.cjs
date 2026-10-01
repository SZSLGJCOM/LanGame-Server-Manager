const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

require.extensions[".ts"] = (module, filename) => {
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
};

const desktopRoot = path.resolve(__dirname, "..");
const serversView = fs.readFileSync(path.join(desktopRoot, "src", "views", "ServersView.tsx"), "utf8");
const serverMaintenance = fs.readFileSync(path.join(desktopRoot, "src", "views", "servers", "ServerMaintenanceWorkspace.tsx"), "utf8");
const tabSpecs = fs.readFileSync(path.join(desktopRoot, "src", "views", "servers", "server-detail-tab-specs.ts"), "utf8");
const serverDetailTabs = fs.readFileSync(path.join(desktopRoot, "src", "views", "servers", "ServerDetailTabs.tsx"), "utf8");
const runtimeSurfaceWorkbench = fs.readFileSync(
  path.join(desktopRoot, "src", "views", "servers", "RuntimeSurfaceWorkbench.tsx"),
  "utf8"
);
const modWorkbenchView = fs.readFileSync(
  path.join(desktopRoot, "src", "views", "servers", "ModWorkbench.tsx"),
  "utf8"
);
const modWorkbenchModel = fs.readFileSync(
  path.join(desktopRoot, "src", "views", "servers", "mod-workbench-model.ts"),
  "utf8"
);
const modWorkbenchPlans = fs.readFileSync(
  path.join(desktopRoot, "src", "views", "servers", "mod-workbench-plans.ts"),
  "utf8"
);
const modWorkbenchMutations = fs.readFileSync(
  path.join(desktopRoot, "src", "views", "servers", "useModWorkbenchMutations.ts"),
  "utf8"
);
const modWorkbench = [modWorkbenchView, modWorkbenchModel, modWorkbenchPlans, modWorkbenchMutations].join("\n");
const steamWorkshopStoreModel = fs.readFileSync(
  path.join(desktopRoot, "src", "views", "servers", "steam-workshop-store-model.ts"),
  "utf8"
);
const workshopStatus = fs.readFileSync(path.join(desktopRoot, "src", "views", "servers", "WorkshopStatus.tsx"), "utf8");
const manualModInventoryList = fs.readFileSync(path.join(desktopRoot, "src", "views", "servers", "ManualModInventoryList.tsx"), "utf8");
const modWorkbenchCapability = fs.readFileSync(
  path.join(desktopRoot, "src", "views", "servers", "mod-workbench-capability.ts"),
  "utf8"
);
const playerAccessWorkbench = fs.readFileSync(
  path.join(desktopRoot, "src", "views", "servers", "player-center", "use-player-access.tsx"),
  "utf8"
);
const dstWorkshopConfiguration = fs.readFileSync(
  path.join(desktopRoot, "src", "views", "servers", "DstWorkshopConfiguration.tsx"),
  "utf8"
);
const aiBroadcastWorkbench = fs.readFileSync(
  path.join(desktopRoot, "src", "views", "servers", "AiBroadcastWorkbench.tsx"),
  "utf8"
);
const workbenchEntryCssPath = path.join(desktopRoot, "src", "views", "servers", "workbench.css");
const workbenchEntryCss = fs.readFileSync(workbenchEntryCssPath, "utf8");
const operationsCss = readCssWithImports(workbenchEntryCssPath);
const modWorkbenchCss = fs.readFileSync(
  path.join(desktopRoot, "src", "views", "servers", "workbench", "mods.css"),
  "utf8"
);
const playerAccessControlCss = fs.readFileSync(
  path.join(desktopRoot, "src", "views", "servers", "workbench", "player-access-control.css"),
  "utf8"
);
const playerCenterCss = fs.readFileSync(
  path.join(desktopRoot, "src", "views", "servers", "workbench", "player-center.css"),
  "utf8"
);
const zhExtraMessages = fs.readFileSync(path.join(desktopRoot, "src", "i18n-messages-zh-extra.ts"), "utf8");
const enExtraMessages = fs.readFileSync(path.join(desktopRoot, "src", "i18n-messages-en-extra.ts"), "utf8");
const dstZhMessages = fs.readFileSync(path.join(desktopRoot, "src", "i18n", "games", "dontstarve.zh-cn.ts"), "utf8");
const dstEnMessages = fs.readFileSync(path.join(desktopRoot, "src", "i18n", "games", "dontstarve.en.ts"), "utf8");

function readCssWithImports(sourcePath, seen = new Set()) {
  if (seen.has(sourcePath)) {
    return "";
  }
  seen.add(sourcePath);

  const source = fs.readFileSync(sourcePath, "utf8");
  const sourceDir = path.dirname(sourcePath);
  const imports = Array.from(source.matchAll(/@import\s+"([^"]+)";/g), (match) =>
    readCssWithImports(path.resolve(sourceDir, match[1]), seen)
  );
  return [source, ...imports].join("\n");
}

function cssBlock(source, selector) {
  const rules = source.replace(/\/\*[\s\S]*?\*\//g, "").replace(/@import\s+[^;]+;/g, "");
  const matches = [...rules.matchAll(/([^{}]+)\{([^{}]*)\}/g)]
    .filter((match) => match[1].split(",").some((candidate) => candidate.trim() === selector.trim()));
  assert.ok(matches.length, `Missing CSS selector ${selector}`);
  return matches.map((match) => match[2]).join("\n");
}

function cssRuleSubjects(source) {
  const rules = source.replace(/\/\*[\s\S]*?\*\//g, "");
  return [...rules.matchAll(/([^{}]+)\{([^{}]*)\}/g)]
    .flatMap((match) => match[1].split(","))
    .map((selector) => selector.trim().split(/[\s>+~]+/).at(-1)).join("\n");
}

test("server detail tabs expose compact icon affordances without suffix clutter", () => {
  assert.match(serversView, /ServerDetailTabSpec/);
  assert.match(serverDetailTabs, /icon:\s*ShellIconName/);
  assert.match(serverDetailTabs, /server-detail-tab-icon/);
  assert.doesNotMatch(serverDetailTabs, /server-detail-tab-meta/);
  assert.doesNotMatch(serverDetailTabs, /title=\{`\$\{tab\.label\}\s+\$\{tab\.meta\}`\}/);
  assert.doesNotMatch(serversView, /server-detail-actionbar/);
  assert.doesNotMatch(serversView, /server-detail-identity/);
});

test("server detail mods tab stays visible but disabled when unsupported", () => {
  assert.match(serversView, /const detailTabs = buildServerDetailTabSpecs\(/);
  assert.match(
    tabSpecs,
    /id: "mods"[\s\S]*?label: t\("servers\.tabs\.mods"[\s\S]*?icon: "package"[\s\S]*?disabled: !moduleHasModWorkbench\(moduleId, moduleDetails\)/
  );
  assert.doesNotMatch(
    tabSpecs,
    /\.\.\.\(hasModWorkbench\s*\?/,
    "Unsupported modules should keep the Mods tab visible as disabled instead of removing it"
  );
});

test("server detail tabs expose all local workflows without account gates", () => {
  const orderedIds = ["runtime", "settings", "mods", "players", "maintenance", "gm"];
  let previousIndex = -1;
  for (const id of orderedIds) {
    const index = tabSpecs.indexOf(`id: "${id}"`, previousIndex + 1);
    assert.ok(index > previousIndex, `${id} should follow the configured server detail tab order`);
    previousIndex = index;
  }

  assert.match(tabSpecs, /id: "gm"[\s\S]*?icon: "zap"/);
  assert.doesNotMatch(tabSpecs, /id: "(?:broadcast|map)"/);
  assert.match(serversView, /<ServerMaintenanceWorkspace active=\{activeDetailTab === "maintenance"\}/);
  assert.match(serverMaintenance, /<AiBroadcastWorkbench[\s\S]*?active=\{props\.active\}/);
  assert.doesNotMatch(serversView, /hasGmWorkbench|\.\.\.\(hasGmWorkbench/);
  assert.match(serversView, /if \(tab\.disabled\) \{\s*return;\s*\}[\s\S]*?activateDetailTab\(tab\)/);
  assert.match(serversView, /<ServerDetailTabs[\s\S]*?onSelect=\{selectDetailTab\}/);
  assert.doesNotMatch(serversView, /auth\.|membership|vip/i);
  assert.doesNotMatch(operationsCss, /server-detail-tab-vip|is-vip/i);
});

test("not-modelled mod workflows do not enable the server mods tab", () => {
  assert.match(modWorkbenchCapability, /const catalogEntry = MOD_WORKFLOW_CATALOG\[moduleId\]/);
  assert.match(modWorkbenchCapability, /const hasModuleDefinedModWorkflow = Boolean\(/);
  assert.match(modWorkbenchCapability, /moduleDetails\?\.mods\?\.enablement/);
  assert.match(
    modWorkbenchCapability,
    /catalogEntry\?\.supportStatus === "not_modelled" && !hasModuleDefinedModWorkflow[\s\S]*?return false;/
  );
});

test("server detail Chinese tab labels stay short and user-facing", () => {
  assert.match(zhExtraMessages, /"servers\.tabs\.players":\s*"\\u73a9\\u5bb6"/);
  assert.match(zhExtraMessages, /"servers\.tabs\.gmTools":\s*"工具"/);
  assert.match(zhExtraMessages, /"servers\.tabs\.maintenance":\s*"维护"/);
  assert.match(zhExtraMessages, /"servers\.tabs\.mods":\s*"\\u6a21\\u7ec4"/);

  const { buildServerDetailTabSpecs } = require("../src/views/servers/server-detail-tab-specs.ts");
  const { ZH_CN_EXTRA_MESSAGES } = require("../src/i18n-messages-zh-extra.ts");
  const tabs = buildServerDetailTabSpecs({
    moduleId: "dontstarve",
    moduleDetails: null,
    t: (key) => {
      assert.ok(Object.hasOwn(ZH_CN_EXTRA_MESSAGES, key), `${key} must have a Chinese translation`);
      return ZH_CN_EXTRA_MESSAGES[key];
    }
  });
  assert.deepEqual(tabs.map(({ id, label }) => [id, label]), [
    ["runtime", "运行"], ["settings", "配置"], ["mods", "模组"],
    ["players", "玩家"], ["maintenance", "维护"], ["gm", "工具"]
  ]);
  assert.doesNotMatch(serversView, /AI 广播|GM"\)/);

  assert.doesNotMatch(aiBroadcastWorkbench, /bc-header/);
  assert.doesNotMatch(aiBroadcastWorkbench, /servers\.broadcast\.(eyebrow|manualTitle|supported|notSupported)/);
  assert.doesNotMatch(operationsCss, /\.bc-header|\.bc-status-chip/);
  assert.doesNotMatch(zhExtraMessages, /servers\.broadcast\.(eyebrow|manualTitle|manualBody|supported|notSupported)/);
  assert.doesNotMatch(enExtraMessages, /servers\.broadcast\.(eyebrow|manualTitle|manualBody|supported|notSupported)/);
  assert.doesNotMatch(aiBroadcastWorkbench, /AI Broadcast/);
});

test("server detail tab clicks are not reset by overview section sync", () => {
  assert.doesNotMatch(
    serversView,
    /setActiveDetailTab\(props\.section === "settings" \? "settings" : "runtime"\);\s*\}\s*,\s*\[props\.section,\s*props\.selectedInstanceId\]\)/
  );
  assert.match(serversView, /if \(props\.section === "settings"\)/);
  assert.doesNotMatch(serversView, /previousSelectedInstanceIdRef|selectedInstanceChanged/);
  assert.match(
    serversView,
    /<div key=\{props\.selectedDetails\.summary\.id\} className=\{detailScrollClassName\}\s+id=\{detailPanelId\} role="tabpanel"/,
    "Instance changes should remount detail content without resetting the selected detail tab"
  );
});

test("server detail shell stays compact for delivery", () => {
  const layoutBlock = cssBlock(operationsCss, ".servers-page .server-layout-grid");
  assert.match(layoutBlock, /grid-template-columns:\s*repeat\(4,\s*minmax\(0,\s*1fr\)\)/);
  assert.match(layoutBlock, /gap:\s*10px/);
  assert.doesNotMatch(operationsCss, /\.servers-page \.server-layout-grid\s*\{[\s\S]*?grid-template-columns:\s*(?:clamp|360px)/);

  const listPanelBlock = cssBlock(operationsCss, ".servers-page .server-list-panel");
  assert.match(listPanelBlock, /grid-column:\s*span 1/);

  const detailPanelBlock = cssBlock(operationsCss, ".servers-page .server-detail-panel");
  assert.match(detailPanelBlock, /grid-column:\s*span 3/);

  const tabsBlock = cssBlock(operationsCss, ".server-detail-tabs");
  assert.match(tabsBlock, /grid-auto-flow:\s*column/);
  assert.match(tabsBlock, /grid-auto-columns:\s*minmax\(max-content,\s*1fr\)/);
  assert.match(tabsBlock, /overflow-x:\s*auto/);
  assert.doesNotMatch(tabsBlock, /auto-fit|auto-fill/);

  const tabBlock = cssBlock(operationsCss, ".server-detail-tab");
  assert.match(tabBlock, /min-height:\s*32px/);
  assert.match(tabBlock, /border-radius:\s*8px/);
  assert.doesNotMatch(tabBlock, /999px/);

  const panelBlock = cssBlock(operationsCss, "\n.server-detail-panel");
  assert.match(panelBlock, /padding:\s*0/);
  assert.match(panelBlock, /background:\s*transparent/);
  assert.match(panelBlock, /border:\s*0/);

  assert.match(
    operationsCss,
    /(?:^|\n)\.server-list-panel\s*\{[\s\S]*?grid-template-columns:\s*minmax\(0,\s*1fr\)/
  );

  const listPanelHeadBlock = cssBlock(operationsCss, ".server-list-panel > .panel-head");
  assert.match(listPanelHeadBlock, /width:\s*100%/);
  assert.match(listPanelHeadBlock, /min-width:\s*0/);

  const listSearchBlock = cssBlock(operationsCss, ".server-list-search");
  assert.match(listSearchBlock, /flex:\s*1 1 0/);
  assert.match(listSearchBlock, /min-width:\s*0/);

  const scrollBlock = cssBlock(operationsCss, ".server-detail-scroll");
  assert.match(scrollBlock, /gap:\s*10px/);
});

test("workspace tab contents keep compact, deliberate spacing", () => {
  const listScrollBlock = cssBlock(operationsCss, ".server-list-panel > .table-list");
  assert.match(listScrollBlock, /gap:\s*8px/);
  assert.match(listScrollBlock, /overflow-y:\s*auto/);

  const listCardShellBlock = cssBlock(operationsCss, ".server-list-card-shell");
  assert.match(listCardShellBlock, /height:\s*var\(--server-list-card-height\)/);
  assert.match(listCardShellBlock, /min-height:\s*var\(--server-list-card-height\)/);
  assert.match(listCardShellBlock, /max-height:\s*var\(--server-list-card-height\)/);
  assert.match(listCardShellBlock, /aspect-ratio:\s*auto/);
  assert.match(listCardShellBlock, /border-radius:\s*8px/);
  assert.doesNotMatch(operationsCss, /height:\s*clamp\(118px,\s*9vw,\s*148px\)/);
  assert.doesNotMatch(operationsCss, /aspect-ratio:\s*21\s*\/\s*9/);

  const listCardFooterBlock = cssBlock(operationsCss, ".server-list-card-footer");
  assert.match(listCardFooterBlock, /grid-template-columns:\s*minmax\(0,\s*1fr\)\s+var\(--server-list-card-action-width\)/);

  const listCardCopyBlock = cssBlock(operationsCss, ".server-list-card-copy");
  assert.match(listCardCopyBlock, /grid-column:\s*1\s*\/\s*-1/);

  const listCardTitleBlock = cssBlock(operationsCss, ".server-list-card-copy .row-title");
  assert.match(listCardTitleBlock, /max-width:\s*100%/);
  assert.match(listCardTitleBlock, /-webkit-line-clamp:\s*2/);
  assert.match(listCardTitleBlock, /overflow-wrap:\s*anywhere/);
  assert.doesNotMatch(listCardTitleBlock, /max-width:\s*16ch/);

  const listCardActionBlock = [...operationsCss.matchAll(/\.server-list-card-primary-action\s*\{([\s\S]*?)\}/g)]
    .map((match) => match[1])
    .find((block) => block.includes("width: var(--server-list-card-action-width)"));
  assert.ok(listCardActionBlock, "Missing fixed-size server list action block");
  assert.match(listCardActionBlock, /width:\s*var\(--server-list-card-action-width\)/);
  assert.match(listCardActionBlock, /min-width:\s*var\(--server-list-card-action-width\)/);
  assert.match(listCardActionBlock, /max-width:\s*var\(--server-list-card-action-width\)/);
  assert.match(listCardActionBlock, /white-space:\s*nowrap/);

  const backupRowBlocks = [...operationsCss.matchAll(/\.server-file-backup-row[^{]*\{([\s\S]*?)\}/g)]
    .map((match) => match[1]);
  assert.ok(
    backupRowBlocks.some((block) => /grid-template-columns:\s*minmax\(0,\s*1fr\)/.test(block)),
    "Missing compact backup record layout"
  );
  assert.ok(
    backupRowBlocks.some((block) => /padding:\s*8px 0/.test(block)),
    "Missing compact backup record spacing"
  );

  assert.match(playerCenterCss, /\.player-center-list-pane,[\s\S]*?border-radius:\s*10px/);

  const modCardBlock = cssBlock(modWorkbenchCss, ".mw-entry-row");
  assert.match(modCardBlock, /min-height:\s*92px/);
  assert.match(modCardBlock, /border-radius:\s*9px/);
});

test("mod workbench keeps store and My Mods in the full-width Workshop toolbar", () => {
  assert.match(modWorkbench, /type ModWorkbenchView = "store" \| "config"/);
  assert.match(modWorkbench, /const \[activeWorkbenchView,\s*setActiveWorkbenchView\] = useState<ModWorkbenchView>\(readOnly \? "config" : "store"\)/);
  assert.match(modWorkbench, /const MOD_WORKBENCH_TABS = \[/);
  assert.match(modWorkbench, /id: "store"[\s\S]*?id: "config"/);
  assert.doesNotMatch(modWorkbench, /id: "installed"/);
  assert.doesNotMatch(modWorkbench, /className="mw-sidebar"|className="mw-primary-tabs"|aria-orientation="vertical"/);
  assert.match(modWorkbench, /function renderSteamWorkshopToolbar\(\)/);
  assert.match(modWorkbench, /browseQuery\.trim\(\) \? \["relevance"\] : \[\]\), "trend", "popular", "recent", \.\.\.\(browseKind === "item" \? \["subscribers"\] : \[\]\)\][\s\S]*?servers\.mods\.mainTabs\.config/);
  assert.match(modWorkbench, /renderSteamWorkshopWorkspace\(\)[\s\S]*?renderCommunityWorkspace\(\)/);
  assert.doesNotMatch(modWorkbench, /function renderInstalledPane\(\)/);
  assert.match(modWorkbench, /aria-selected=\{activeWorkbenchView === "store" && browseSort === sort\}/);
  assert.doesNotMatch(
    modWorkbench,
    /setSelectedBrowseWorkshopId\(null\);[\s\S]{0,120}?setActiveWorkbenchView\("config"\)/,
    "My Mods must not discard the open Workshop detail when switching views"
  );
  assert.doesNotMatch(modWorkbench, /servers\.mods\.sort\.installed/);
  assert.doesNotMatch(modWorkbench, /detailFooterExpanded|mw-detail-footer|workshopView/);

  const workbenchBlock = cssBlock(modWorkbenchCss, ".mw-workbench");
  assert.match(workbenchBlock, /display:\s*grid/);
  assert.match(workbenchBlock, /grid-template-columns:\s*minmax\(0,\s*1fr\)/);
  assert.doesNotMatch(operationsCss, /\.mw-sidebar|\.mw-primary-tabs|\.mw-primary-tab/);
  assert.match(operationsCss, /\.mw-steam-workspace,[\s\S]*?\.mw-community-workspace[\s\S]*?grid-template-rows:\s*auto minmax\(0,\s*1fr\)/);

  const workspaceSectionBlock = cssBlock(operationsCss, ".mw-workspace-section");
  assert.match(workspaceSectionBlock, /flex:\s*1 1 auto/);

  const configPaneBlock = cssBlock(operationsCss, ".mw-config-pane");
  assert.match(configPaneBlock, /height:\s*100%/);
  assert.doesNotMatch(operationsCss, /\.mw-detail-footer|\.mw-workspace-panel--installed/);
});

test("mod workbench separates view orchestration from pure model and settings planning", () => {
  assert.ok(modWorkbenchView.split(/\r?\n/).length < 3000, "ModWorkbench.tsx should stay below the complex-module limit");
  assert.ok(modWorkbenchModel.split(/\r?\n/).length < 500, "the pure workbench model should stay focused");
  assert.ok(modWorkbenchPlans.split(/\r?\n/).length < 500, "settings planning should stay focused");
  assert.match(modWorkbenchView, /from "\.\/mod-workbench-model"/);
  assert.match(modWorkbenchView, /from "\.\/mod-workbench-plans"/);
  assert.match(modWorkbenchModel, /export function buildConfiguredEntries\(/);
  assert.match(modWorkbenchPlans, /export function buildModSettingsApplyPlan\(/);
});

test("mod store reports persistent local installation status", () => {
  assert.match(modWorkbench, /readSteamWorkshopInstallationStatus/);
  assert.match(steamWorkshopStoreModel, /export type WorkshopInstallationState = "installed" \| "partial" \| "not-installed" \| "checking" \| "unknown"/);
  assert.match(steamWorkshopStoreModel, /export function resolveWorkshopInstallationState\(/);
  assert.match(modWorkbench, /workshopInstallationResult\?\.items/);
  assert.match(modWorkbench, /const machineCachedSteamIdSet = useMemo\(/);
  assert.match(modWorkbench, /const \[retainedWorkshopIds, setRetainedWorkshopIds\] = useState<string\[\]>\(\[\]\)/);
  assert.match(modWorkbench, /const workshopLookupIds = useMemo\(/);
  assert.match(modWorkbench, /uniqueEntries\(\[\.\.\.workshopIds, \.\.\.retainedWorkshopIds, \.\.\.collectionLookupIdsKey\.split/);
  assert.match(modWorkbench, /if \(readOnly \|\| !isSteamWorkshopModule\) \{/);
  assert.match(modWorkbench, /readSteamWorkshopInstallationStatus\(props\.details\.summary\.id, ids\)/);
  assert.match(modWorkbench, /workshopLookupIds\.map\(\(id\) => lookupMap\[id\]\)/);
  assert.match(modWorkbench, /resolveWorkshopStoreItemState\(item\)/);
  assert.match(modWorkbench, /itemState\.installationState === "installed"/);
  assert.match(modWorkbenchView, /<WorkshopStatus state=\{itemState\.lifecycleState\}/);
  assert.match(workshopStatus, /servers\.mods\.lifecycle\.\$\{state\}/);
  assert.match(workshopStatus, /state === "enabled" \|\| state === "downloaded" \|\| state === "not-installed"\) return null/);
});

test("mods tab uses the shared workbench without game-specific context", () => {
  const modsTabBlock = serversView.match(/\{activeDetailTab === "mods"[\s\S]*?\) : null\}/)?.[0] ?? "";
  assert.match(modsTabBlock, /<ModWorkbench/);
  assert.doesNotMatch(modsTabBlock, /Minecraft/);
  assert.doesNotMatch(operationsCss, /server-detail-scroll--minecraft-context|minecraft-mod-context/);
});

test("mod workbench lists enabled rows and installs store selections as one operation", () => {
  assert.match(modWorkbench, /interface ModEnabledRow/);
  assert.match(modWorkbench, /function buildEnabledRows\(entries: ModSourceEntry\[\]\): ModEnabledRow\[\]/);
  assert.match(modWorkbench, /const enabledRows = useMemo\(\s*\(\) => buildEnabledRows\(configurableEntries\)/);
  assert.match(modWorkbench, /const \[selectedEnabledRowKey,\s*setSelectedEnabledRowKey\]/);
  assert.match(modWorkbench, /enabledRows\.map\(\(row\)/);
  assert.match(modWorkbench, /lookupMap\[row\.id\]/);
  assert.match(modWorkbench, /handleDisableEnabledRow\(row\)/);
  assert.match(modWorkbench, /handleDeleteEnabledRow\(row\)/);
  assert.match(modWorkbench, /className="mw-entry-enabled-toggle"/);
  assert.match(modWorkbench, /className="mw-entry-remove-button"/);
  assert.match(
    modWorkbench,
    /function handleWorkshopStoreAction[\s\S]*?action === "manage"[\s\S]*?handleManageWorkshopItem\(item\);[\s\S]*?handleInstallWorkshopItems\(\[item\.id\]\);/,
    "An unconfigured Workshop item must use the instance install flow even when its package is cached"
  );
  assert.match(
    modWorkbench,
    /function handleInstallWorkshopItems[\s\S]*?runModMutation\("install"[\s\S]*?await downloadSteamWorkshopItems[\s\S]*?if \(nextSettings\) \{[\s\S]*?await persistSettings\(nextSettings\);/,
    "Store installation should await the download before persisting instance enablement"
  );
  assert.doesNotMatch(modWorkbench, /function handleApplySelectedToSettings/);
  assert.doesNotMatch(modWorkbench, /function handleDownloadSteamWorkshopItems/);
  assert.match(
    modWorkbench,
    /function handleManageWorkshopItem[\s\S]*?handleManageWorkshopContent\(workshopItemContentIds\(item\)\);/,
    "Managing an individual Workshop item should use the shared content selection handler"
  );
  assert.match(
    modWorkbench,
    /function resolveWorkshopContentSelection[\s\S]*?collectProjectZomboidLocalIds\(pzSnapshot, contentIds\)[\s\S]*?enabledRows\.find[\s\S]*?return \{ row: row \?\? null, inventoryItem: inventoryItems\[0\] \?\? null \}/,
    "Workshop content selection should resolve the configured local Mod or map entry"
  );
  assert.match(
    modWorkbench,
    /function handleManageWorkshopContent[\s\S]*?const \{ row, inventoryItem \} = resolveWorkshopContentSelection\(contentIds\);[\s\S]*?setSelectedEnabledRowKey\(row\.key\);/,
    "Managing a Workshop item should navigate using the resolved local selection"
  );
  assert.match(
    modWorkbench,
    /function handleRemoveWorkshopItems[\s\S]*?launchModMutation\("enablement"[\s\S]*?await persistSettings\(removePlan\.nextSettings, latestState\.settings, undefined,[\s\S]*?assertDstModOwnership\(current, targetIds\)[\s\S]*?assertWorkshopControlBase\(latestState\.settings, current\)[\s\S]*?setApplyMessage\(null\);/,
    "Removing an enabled Workshop row should update the list without a success banner"
  );
  assert.match(
    modWorkbench,
    /function handleDisableEnabledSettingValue[\s\S]*?launchModMutation\("enablement"[\s\S]*?await persistSettings\(nextSettings\);[\s\S]*?setApplyMessage\(null\);/,
    "Removing an enabled non-Workshop setting row should update the list without a success banner"
  );
  assert.doesNotMatch(modWorkbench, /selectedEnabledEntryKey/);
  assert.doesNotMatch(modWorkbench, /onDoubleClick=\{\(\) => handleMoveEnabledEntry\(entry\)\}/);
});

test("mod mutations serialize and rebase writes onto current instance settings", () => {
  assert.match(modWorkbenchView, /useModWorkbenchMutations\(\s*props\.details\.summary\.id, moduleId, modChangesBlockedRef, latestSettingsRef, t\s*\)/);
  assert.match(modWorkbenchMutations, /const mutationToken = useRef<symbol \| null>\(null\)/);
  assert.match(modWorkbenchMutations, /if \(mutationToken\.current\)[\s\S]*?throw new Error\(t\("servers\.mods\.mutationBusy"/);
  assert.match(modWorkbenchMutations, /await coordinator\.runOperation\(instanceId/);
  assert.match(modWorkbench, /runModMutation\("install"/);
  assert.match(modWorkbench, /const latestDetails = await readInstanceDetails\(props\.details\.summary\.id\)/);
  assert.match(modWorkbench, /instanceBlocksModChanges\(latestDetails\) \|\| modChangesBlockedRef\.current/);
  assert.match(modWorkbench, /mergeSettingPatch\(latestState\.settings, nextSettings, dirtyKeys\)/);
  assert.match(modWorkbench, /inventoryBefore = await readManualModInventory[\s\S]*?await downloadSteamWorkshopItems/);
  assert.match(modWorkbench, /enablementPlan\.unresolvedWorkshopItemIds\.length > 0/);
});

test("mod configuration does not duplicate instance settings", () => {
  assert.doesNotMatch(modWorkbench, /ModConfigurationFields|saveConfigurationFields/);
  assert.doesNotMatch(modWorkbenchCss, /\.mw-game-config/);
  assert.doesNotMatch(enExtraMessages, /servers\.mods\.gameConfiguration/);
  assert.doesNotMatch(zhExtraMessages, /servers\.mods\.gameConfiguration/);
});

test("RimWorld Workshop dependencies cannot enter the server install flow", () => {
  assert.match(modWorkbenchCapability, /rimworld:\s*\{[\s\S]*?installScope:\s*"client_only"/);
  assert.match(modWorkbench, /workflow\.installScope === "client_only"/);
  assert.match(modWorkbench, /servers\.mods\.unsupported\.clientOnlyTitle/);
  assert.match(enExtraMessages, /"servers\.mods\.unsupported\.clientOnlyBody":/);
  assert.match(zhExtraMessages, /"servers\.mods\.unsupported\.clientOnlyBody":/);
});

test("mod workbench owns DST options through its focused configuration component", () => {
  assert.match(modWorkbench, /function renderSelectedModConfigPanel\(memberId\?: string\)/);
  assert.doesNotMatch(modWorkbenchView, /renderSelectedModInfoDialog|modInfoDialogOpen|setModInfoDialogOpen/);
  assert.match(modWorkbench, /function buildConfigurableEntries\(moduleId: string, entries: ModSourceEntry\[\]\): ModSourceEntry\[\]/);
  assert.match(modWorkbench, /entry\.key === "dst-enabled"/);
  assert.match(
    modWorkbench,
    /const enabledIds = uniqueEntries\(\[\.\.\.DONTSTARVE_SHARDS\.flatMap\(\(shard\) =>\s*parseWorkshopIdList\(settings\[`\$\{shard\}_enabled_workshop_mod_ids`\]\)\)[\s\S]*?buildEntry\(\s*"dst-enabled",\s*"Mod",\s*"modoverrides.lua",\s*"game-mod",\s*enabledIds/,
    "DST should expose one deduplicated Mod row instead of one row per shard"
  );
  assert.match(
    modWorkbench,
    /function handleDisableEnabledRow[\s\S]*?moduleId === "dontstarve" && row\.entry\.key === "dst-enabled"[\s\S]*?handleSetDstModsEnabled\(\[id\], false\);/,
    "Unchecking a DST Mod should use the same enablement control as batch actions"
  );
  assert.match(
    modWorkbenchView,
    /function handleSetDstModsEnabled[\s\S]*?launchModMutation\("enablement"[\s\S]*?buildDstModEnablementPlan\(latestState\.settings, targetIds, enabled, members\)[\s\S]*?await persistSettings\(next, latestState\.settings, undefined,[\s\S]*?assertDstModOwnership\(current, targetIds\)/,
    "DST enablement must retain mutation serialization and verify ownership again before saving"
  );
  const { buildDstModEnablementPlan, buildModSettingsApplyPlan } = require("../src/views/servers/mod-workbench-plans.ts");
  const mod = { id: "2000001", status: "resolved", item_kind: "item", consumer_app_id: 322330, children: [] };
  for (const layout of ["standard", "island_adventures"]) {
    const shards = layout === "standard" ? ["master", "caves"] : ["master", "caves", "islands", "volcano"];
    const settings = { shard_layout: layout, enable_caves: false, shared_workshop_mod_ids: mod.id };
    for (const shard of shards) {
      settings[`${shard}_enabled_workshop_mod_ids`] = mod.id;
      settings[`${shard}_mod_configuration_options`] = { [mod.id]: { difficulty: 2 } };
    }
    const original = structuredClone(settings);
    const disabled = buildDstModEnablementPlan(settings, [mod.id], false);
    assert.ok(disabled, "An owned Mod can be disabled in either shard layout");
    assert.equal(disabled.shared_workshop_mod_ids, mod.id, "Disabling retains instance ownership");
    for (const shard of shards) {
      assert.equal(disabled[`${shard}_enabled_workshop_mod_ids`], "");
      assert.deepEqual(disabled[`${shard}_mod_configuration_options`], original[`${shard}_mod_configuration_options`]);
    }
    const enabled = buildModSettingsApplyPlan("dontstarve", disabled, [mod.id], { [mod.id]: mod }, 322330);
    assert.equal(enabled.canApply, true);
    for (const shard of shards) assert.equal(enabled.nextSettings[`${shard}_enabled_workshop_mod_ids`], mod.id,
      "Enabling prepares every layout shard, including caves that are not running");
    assert.deepEqual(settings, original, "Enablement planning must not mutate saved input");
  }
  assert.doesNotMatch(modWorkbench, /if \(isTruthySetting\(nextSettings\.enable_caves\)\)/);
  assert.match(modWorkbench, /className="mw-detail-config-col"/);
  assert.match(modWorkbench, /className="mw-detail-config-panel"/);
  assert.doesNotMatch(modWorkbenchView, /mw-entry-info-button|mw-entry-info-dialog|mw-selected-config-head/);
  assert.doesNotMatch(modWorkbenchView, /servers\.mods\.configPanel\.kicker|servers\.mods\.manifest\.open/);
  assert.doesNotMatch(modWorkbench, /DstModConfigPanel|DstModConfigurationRenderer|readDontStarveModConfigurationSpecs/);
  assert.match(modWorkbenchView, /<DstWorkshopConfiguration/);
  assert.match(dstWorkshopConfiguration, /useDstModConfigurationSpec/);
  assert.match(dstWorkshopConfiguration, /<DstModConfigPanel[\s\S]*?selectedModId=\{props\.selectedModId\}[\s\S]*?compact/);
  assert.match(dstWorkshopConfiguration, /master_mod_configuration_options: configuration,[\s\S]*?caves_mod_configuration_options: configuration/);
  assert.doesNotMatch(modWorkbench, /servers\.mods\.configPanel\.enableFirst/);
  assert.doesNotMatch(enExtraMessages, /servers\.mods\.configPanel\.enableFirst/);
  assert.doesNotMatch(zhExtraMessages, /servers\.mods\.configPanel\.enableFirst/);
  assert.doesNotMatch(dstWorkshopConfiguration, /downloaded locally/);
  assert.doesNotMatch(dstEnMessages, /downloaded locally/);
  assert.doesNotMatch(dstZhMessages, /尚未本地下载/);
  assert.doesNotMatch(modWorkbench, /function renderSharedDetailPanel/);
  assert.doesNotMatch(modWorkbench, /className="mw-detail-shared-col"/);
  assert.doesNotMatch(modWorkbench, /className="mw-detail-shared-panel"/);
  assert.doesNotMatch(modWorkbench, /server-mod-installed-stack/);
  assert.doesNotMatch(modWorkbenchCss, /server-mod-installed-stack/);
});

test("mod workbench supports drag sorting only for enabled rows", () => {
  assert.match(modWorkbench, /function reorderEnabledEntryValues\(\s*entry: ModSourceEntry,\s*sourceValue: string,\s*targetValue: string\s*\): string\[\] \| null/);
  assert.match(modWorkbench, /function handleReorderEnabledRow\(sourceRow: ModEnabledRow \| null,\s*targetRow: ModEnabledRow\)/);
  assert.match(modWorkbench, /const \[draggedEnabledRow,\s*setDraggedEnabledRow\] = useState<ModEnabledRow \| null>\(null\)/);
  assert.match(modWorkbench, /const \[enabledDropTargetKey,\s*setEnabledDropTargetKey\] = useState<string \| null>\(null\)/);
  assert.match(modWorkbench, /function handleEnabledRowPointerDown\(event: PointerEvent<HTMLButtonElement>,\s*row: ModEnabledRow\)/);
  assert.match(modWorkbench, /function handleEnabledRowPointerEnter\(row: ModEnabledRow\)/);
  assert.match(modWorkbench, /function handleEnabledRowPointerUp\(event: PointerEvent<HTMLButtonElement>,\s*row: ModEnabledRow\)/);
  assert.match(modWorkbench, /function handleEnabledRowPointerCancel\(\)/);
  assert.match(modWorkbench, /onPointerDown=\{\(event\) => handleEnabledRowPointerDown\(event,\s*row\)\}/);
  assert.match(modWorkbench, /onPointerEnter=\{\(\) => handleEnabledRowPointerEnter\(row\)\}/);
  assert.match(modWorkbench, /onPointerUp=\{\(event\) => handleEnabledRowPointerUp\(event,\s*row\)\}/);
  assert.match(modWorkbench, /onPointerCancel=\{handleEnabledRowPointerCancel\}/);
  assert.doesNotMatch(modWorkbench, /draggable=\{/);
  assert.doesNotMatch(modWorkbench, /onDragStart=\{/);
  assert.match(
    modWorkbench,
    /function handleReorderEnabledRow[\s\S]*?latestSettingsRef\.current[\s\S]*?reorderedValues\.join\("\\n"\)[\s\S]*?await persistSettings\(nextSettings\);[\s\S]*?setApplyMessage\(null\);/,
    "Dropping an enabled row should persist the reordered setting silently"
  );

  assert.match(modWorkbenchView, /<ManualModInventoryList[\s\S]*?onEnable=\{handleEnableManualInventoryItem\}/);
  assert.match(manualModInventoryList, /props\.onEnable\(item\)/);
  assert.doesNotMatch(manualModInventoryList, /draggable=|onDragStart=|onPointerDown=/);
});

test("mod configuration merges enablement states into one compact list", () => {
  assert.match(modWorkbench, /function renderModList\(\)/);
  assert.doesNotMatch(modWorkbench, /function renderEnabledList\(\)|function renderDisabledList\(\)/);
  assert.match(modWorkbench, /type="checkbox"[\s\S]*?checked[\s\S]*?handleDisableEnabledRow\(row\)/);
  assert.match(manualModInventoryList, /props\.canEnable \?[\s\S]*?checked=\{false\}[\s\S]*?props\.onEnable\(item\)/);
  assert.match(modWorkbenchView, /canEnable=\{isManualEnablementModule\}/);
  assert.match(modWorkbenchView, /canToggleModEnabledRow\(row, moduleId, isManualEnablementModule\) \? <input/);
  assert.match(modWorkbench, /servers\.mods\.detailTabs\.mods/);
  assert.doesNotMatch(modWorkbench, /servers\.mods\.detailTabs\.(?:enabled|disabled)/);
});

test("mod configuration uses instance entries independently of store loading and machine caches", () => {
  assert.match(modWorkbench, /const EMPTY_WORKSHOP_ITEMS: SteamWorkshopLookupItem\[\] = \[\];/);
  assert.match(modWorkbench, /const browsedWorkshopItems = browseResult\?\.items \?\? EMPTY_WORKSHOP_ITEMS;/);
  const list = modWorkbenchView.match(/function renderModList\(\)([\s\S]*?)function resolveActiveDetailSource/)?.[1];
  assert.ok(list, "Missing instance Mod list");
  assert.match(list, /enabledRows\.map\(\(row\)/);
  assert.match(list, /showsManualInventory \? <ManualModInventoryList[\s\S]*?items=\{unconfiguredInventoryItems\}/);
  assert.doesNotMatch(list, /browseLoading|browsedWorkshopItems|workshopInstallationResult|machineCachedSteamIdSet|downloadResult/);
  assert.match(modWorkbench, /const disabledCount = showsManualInventory \? unconfiguredInventoryItems\.length : 0/);
});

test("mod workbench sends Steam failures and operation feedback to the activity bar with a browse retry", () => {
  assert.match(modWorkbench, /applyMessage \? <ActivityNotice tone="success" onDismiss=/);
  assert.doesNotMatch(modWorkbench, /mw-alert-stack|mw-floating-toast/);
  assert.doesNotMatch(modWorkbench, /Workshop lookup failed:/);
  assert.match(modWorkbenchView, /lookupError \? <ActivityNotice tone="error">\{formatDesktopError\(t, lookupError\)\}/);
  assert.match(modWorkbenchView, /browseError \? <ActivityNotice tone="error" action=\{[\s\S]*?onClick=\{retryBrowse\}/);
  assert.doesNotMatch(modWorkbench, /servers\.mods\.storeBrowseFailed/);
  assert.doesNotMatch(modWorkbench, /\{ error: browseError \}/);
  assert.match(modWorkbench, /servers\.mods\.workshopUnavailable/);
  assert.match(enExtraMessages, /"servers\.mods\.workshopUnavailable":/);
  assert.match(zhExtraMessages, /"servers\.mods\.workshopUnavailable":/);
});

test("mod workbench exposes explicit unsupported state for unverified Dragonwilds mods", () => {
  assert.match(modWorkbenchCapability, /runescapedragonwilds:\s*\{/);
  assert.match(modWorkbenchCapability, /supportStatus:\s*"not_modelled"/);
  assert.match(modWorkbenchCapability, /unsupportedReason:/);
  assert.match(modWorkbenchCapability, /Shockbyte-hosted mod management/);
  assert.match(modWorkbenchCapability, /moduleHasModWorkbench[\s\S]*MOD_WORKFLOW_CATALOG\[moduleId\]/);

  assert.match(modWorkbench, /const modWorkflowUnsupported =/);
  assert.match(modWorkbench, /function renderUnsupportedModWorkflowPane\(\)/);
  assert.match(modWorkbench, /className="mw-unsupported-pane"/);
  assert.match(modWorkbench, /servers\.mods\.unsupported\.title/);
  assert.match(modWorkbench, /servers\.mods\.unsupported\.body/);
  assert.match(modWorkbench, /servers\.mods\.unsupported\.guardrail/);
  assert.match(
    modWorkbench,
    /\{modWorkflowUnsupported \? \([\s\S]*?renderUnsupportedModWorkflowPane\(\)[\s\S]*?: \(/,
    "Unsupported modules should render a guardrail panel before any empty community source workspace"
  );

  assert.match(enExtraMessages, /"servers\.mods\.unsupported\.title":/);
  assert.match(enExtraMessages, /"servers\.mods\.unsupported\.body":/);
  assert.match(enExtraMessages, /"servers\.mods\.unsupported\.guardrail":/);
  assert.match(zhExtraMessages, /"servers\.mods\.unsupported\.title":/);
  assert.match(zhExtraMessages, /"servers\.mods\.unsupported\.body":/);
  assert.match(zhExtraMessages, /"servers\.mods\.unsupported\.guardrail":/);
  assert.match(operationsCss, /\.mw-unsupported-pane\s*\{/);
});

test("mod configuration page fills the workspace with independently scrolling columns", () => {
  const configPaneBlock = cssBlock(operationsCss, ".mw-config-pane");
  assert.match(configPaneBlock, /height:\s*100%/);
  assert.match(configPaneBlock, /overflow:\s*hidden/);
  assert.match(configPaneBlock, /align-content:\s*stretch/);

  const detailLayoutBlock = cssBlock(operationsCss, ".mw-detail-layout");
  assert.match(detailLayoutBlock, /grid-template-columns:\s*minmax\(0,\s*1fr\) minmax\(280px,\s*38%\)/);
  assert.match(detailLayoutBlock, /height:\s*100%/);
  assert.match(detailLayoutBlock, /align-items:\s*stretch/);
  assert.match(detailLayoutBlock, /overflow:\s*hidden/);
  assert.doesNotMatch(detailLayoutBlock, /overflow:\s*auto/);

  const detailColBlock = cssBlock(operationsCss, ".mw-detail-list-col");
  assert.match(detailColBlock, /grid-template-rows:\s*auto minmax\(0,\s*1fr\)/);
  assert.match(detailColBlock, /overflow:\s*hidden/);

  const configColBlock = cssBlock(operationsCss, ".mw-detail-config-col");
  assert.match(configColBlock, /display:\s*grid/);
  assert.match(configColBlock, /grid-template-rows:\s*minmax\(0,\s*1fr\)/);
  assert.match(configColBlock, /overflow:\s*hidden/);

  const detailPanelBlock = cssBlock(operationsCss, ".mw-detail-panel");
  assert.match(detailPanelBlock, /min-height:\s*0/);
  assert.match(detailPanelBlock, /overflow:\s*auto/);

  const configPanelBlock = cssBlock(operationsCss, ".mw-detail-config-panel");
  assert.match(configPanelBlock, /min-height:\s*0/);
  assert.match(configPanelBlock, /overflow:\s*auto/);

  const entryListBlock = cssBlock(operationsCss, ".mw-entry-list");
  assert.doesNotMatch(entryListBlock, /overflow-y:\s*auto/);
  assert.doesNotMatch(entryListBlock, /max-height:/);
  assert.doesNotMatch(modWorkbenchCss, /@media\s*\(/);
});

test("mod configuration rows match the Workshop thumbnail-card treatment", () => {
  const detailColTitleBlock = cssBlock(operationsCss, ".mw-detail-col-title");
  assert.match(detailColTitleBlock, /min-height:\s*34px/);
  assert.match(detailColTitleBlock, /padding:\s*8px 12px/);
  assert.match(detailColTitleBlock, /font-size:\s*var\(--text-body\)/);
  assert.match(detailColTitleBlock, /color:\s*var\(--shell-muted\)/);

  const entryListBlock = cssBlock(operationsCss, ".mw-entry-list");
  assert.match(entryListBlock, /gap:\s*9px/);

  const entryRowBlock = cssBlock(operationsCss, ".mw-entry-row");
  assert.match(entryRowBlock, /grid-template-columns:\s*minmax\(0,\s*1fr\) 38px/);
  assert.match(entryRowBlock, /min-height:\s*92px/);
  assert.match(entryRowBlock, /padding:\s*0/);
  assert.match(entryRowBlock, /border:\s*1px solid color-mix/);
  assert.match(entryRowBlock, /border-radius:\s*9px/);
  assert.match(entryRowBlock, /transition:[^;]*transform/);

  const activeRowBlock = cssBlock(operationsCss, ".mw-entry-row--active");
  assert.match(activeRowBlock, /box-shadow:\s*0 0 0 1px/);
  assert.match(activeRowBlock, /background:\s*color-mix\(in srgb,\s*var\(--shell-accent\) 8%/);

  const hoverRowBlock = cssBlock(operationsCss, ".mw-entry-row:hover");
  assert.match(hoverRowBlock, /background:\s*color-mix/);

  const focusRowBlock = cssBlock(operationsCss, ".mw-entry-row:has(:focus-visible)");
  assert.match(focusRowBlock, /outline:\s*2px solid/);

  const entryTitleBlock = cssBlock(operationsCss, ".mw-entry-row-title");
  assert.match(entryTitleBlock, /font-size:\s*var\(--text-body\)/);
  assert.match(entryTitleBlock, /font-weight:\s*var\(--font-weight-semibold\)/);

  const entryIdBlock = cssBlock(operationsCss, ".mw-entry-id");
  assert.match(entryIdBlock, /font-size:\s*var\(--text-meta\)/);

  assert.match(operationsCss, /\.mw-entry-row-select\s*\{[\s\S]*?grid-template-columns:\s*92px minmax\(0,\s*1fr\)/);
  assert.match(operationsCss, /\.mw-entry-row-media\s*\{[\s\S]*?width:\s*92px[\s\S]*?min-height:\s*92px/);
  assert.match(modWorkbench, /className="mw-entry-row-media"[\s\S]*?<SteamWorkshopPreview/);
  assert.doesNotMatch(modWorkbenchView, /mw-selected-config-preview/);
  const storePreviewBlock = cssBlock(modWorkbenchCss, ".mw-store-detail-preview");
  assert.match(storePreviewBlock, /aspect-ratio:\s*1\s*;/);
  assert.doesNotMatch(storePreviewBlock, /aspect-ratio:\s*16\s*\/\s*9/);
  assert.doesNotMatch(modWorkbenchCss, /\.mw-selected-config-preview/);

  const sortableRowBlock = cssBlock(operationsCss, ".mw-entry-row--sortable .mw-entry-row-select");
  assert.match(sortableRowBlock, /cursor:\s*grab/);

  const draggingRowBlock = cssBlock(operationsCss, ".mw-entry-row--dragging");
  assert.match(draggingRowBlock, /opacity:\s*0\.54/);

  const dropTargetRowBlock = cssBlock(operationsCss, ".mw-entry-row--drop-target");
  assert.match(dropTargetRowBlock, /box-shadow:\s*inset 2px 0 0/);
});

test("runtime console accepts commands inside the managed terminal", () => {
  assert.doesNotMatch(runtimeSurfaceWorkbench, /server-runtime-command-form--terminal/);
  assert.doesNotMatch(runtimeSurfaceWorkbench, /server-runtime-command-grid/);
  assert.doesNotMatch(runtimeSurfaceWorkbench, /servers\.runtimeSurface\.primaryProcessMeta/);
  assert.doesNotMatch(runtimeSurfaceWorkbench, /<button[\s\S]*?servers\.runtimeSurface\.send/);
  assert.doesNotMatch(runtimeSurfaceWorkbench, /servers\.runtimeSurface\.openLogFolder/);
  assert.doesNotMatch(runtimeSurfaceWorkbench, /servers\.runtimeSurface\.logLineCount/);
  assert.doesNotMatch(runtimeSurfaceWorkbench, /server-runtime-diagnostics-inline/);
  assert.doesNotMatch(runtimeSurfaceWorkbench, /servers\.runtimeSurface\.runtimeSignals/);
  assert.doesNotMatch(enExtraMessages, /servers\.runtimeSurface\.openLogFolder/);
  assert.doesNotMatch(enExtraMessages, /servers\.runtimeSurface\.logLineCount/);
  assert.doesNotMatch(enExtraMessages, /servers\.runtimeSurface\.runtimeSignals/);
  assert.doesNotMatch(zhExtraMessages, /servers\.runtimeSurface\.openLogFolder/);
  assert.doesNotMatch(zhExtraMessages, /servers\.runtimeSurface\.logLineCount/);
  assert.doesNotMatch(zhExtraMessages, /servers\.runtimeSurface\.runtimeSignals/);
  assert.doesNotMatch(operationsCss, /server-runtime-diagnostics-inline/);
  assert.doesNotMatch(
    serversView,
    /minecraftDetailModel|hasMinecraftContextPanel/
  );
  assert.doesNotMatch(
    operationsCss,
    /\.server-detail-scroll--minecraft-context\.server-detail-scroll--runtime/
  );

  assert.match(runtimeSurfaceWorkbench, /<form[\s\S]*className="server-runtime-console-command-form"[\s\S]*handleRuntimeCommandSubmit/);
  assert.match(runtimeSurfaceWorkbench, /className="server-runtime-console-command-input"/);
  assert.match(runtimeSurfaceWorkbench, /className="server-runtime-console-prompt"/);
  assert.match(runtimeSurfaceWorkbench, /className=\{`server-runtime-console-copy-button/);
  assert.match(runtimeSurfaceWorkbench, /navigator\.clipboard\.writeText\(runtimeLog\)/);
  assert.match(runtimeSurfaceWorkbench, /ShellIcon name=\{consoleCopyState === "copied" \? "check" : "copy"\}/);
  assert.match(runtimeSurfaceWorkbench, /className=\{`server-runtime-health-light is-\$\{runtimeHealthTone\}`\}/);
  assert.match(runtimeSurfaceWorkbench, /props\.runtime\?\.diagnostics \?\? \[\]/);

  const runtimeScrollBlock = cssBlock(operationsCss, ".server-detail-scroll--runtime");
  assert.match(runtimeScrollBlock, /align-content:\s*stretch/);
  assert.match(runtimeScrollBlock, /overflow:\s*hidden/);
  assert.match(runtimeScrollBlock, /padding-right:\s*0/);

  const runtimeStackBlock = cssBlock(operationsCss, ".server-detail-scroll--runtime > .server-module-stack");
  assert.match(runtimeStackBlock, /height:\s*100%/);
  assert.match(runtimeStackBlock, /min-height:\s*0/);

  const consoleCardBlock = cssBlock(operationsCss, ".server-runtime-console-card");
  assert.match(consoleCardBlock, /display:\s*flex/);
  assert.match(consoleCardBlock, /flex-direction:\s*column/);
  assert.match(consoleCardBlock, /height:\s*100%/);
  assert.match(consoleCardBlock, /min-height:\s*0/);

  const terminalActionsBlock = cssBlock(operationsCss, ".server-runtime-terminal-actions");
  assert.match(terminalActionsBlock, /flex-wrap:\s*nowrap/);

  const terminalActionButtonBlock = cssBlock(operationsCss, ".server-runtime-terminal-actions .secondary-button");
  assert.match(terminalActionButtonBlock, /height:\s*26px/);
  assert.match(terminalActionButtonBlock, /white-space:\s*nowrap/);

  const consoleFrameBlock = cssBlock(operationsCss, ".server-runtime-console-frame");
  assert.match(consoleFrameBlock, /display:\s*flex/);
  assert.match(consoleFrameBlock, /flex-direction:\s*column/);
  assert.match(consoleFrameBlock, /height:\s*100%/);
  assert.match(consoleFrameBlock, /min-height:\s*0/);

  const consoleCommandBlock = cssBlock(operationsCss, ".server-runtime-console-command-form");
  assert.match(consoleCommandBlock, /grid-template-columns:\s*auto minmax\(0,\s*1fr\)/);
  assert.match(consoleCommandBlock, /font-family:\s*var\(--font-mono\)/);

  const consoleInputBlock = cssBlock(operationsCss, ".server-runtime-console-command-input");
  assert.match(consoleInputBlock, /background:\s*transparent/);
  assert.match(consoleInputBlock, /border:\s*0/);

  const runtimeLogBlock = cssBlock(operationsCss, ".server-log-preview.server-runtime-console--primary");
  assert.match(runtimeLogBlock, /flex:\s*1 1 auto/);
  assert.match(runtimeLogBlock, /min-height:\s*0/);
  assert.match(runtimeLogBlock, /max-height:\s*none/);
  assert.match(runtimeLogBlock, /overflow:\s*auto/);
  assert.doesNotMatch(runtimeLogBlock, /vh/);

  const copyButtonBlock = cssBlock(operationsCss, ".server-runtime-console-copy-button");
  assert.match(copyButtonBlock, /width:\s*26px/);
  assert.match(copyButtonBlock, /height:\s*26px/);
  assert.match(copyButtonBlock, /padding:\s*0/);
});

test("runtime diagnostics collapse into one health light in the LanGameCMD header", () => {
  assert.doesNotMatch(serversView, /runtimeDiagnostics|server-runtime-diagnostic-list|server-maintenance-health/);
  assert.match(runtimeSurfaceWorkbench, /const runtimeHealthSignals = readOnly \? \[\] : \(props\.runtime\?\.diagnostics \?\? \[\]\)\.filter/);
  assert.match(runtimeSurfaceWorkbench, /\["warning", "error"\]\.includes\(String\(signal\.severity\)\.toLowerCase\(\)\)/);
  assert.match(runtimeSurfaceWorkbench, /runtimeHealthSignals\.some/);
  assert.match(runtimeSurfaceWorkbench, /server-runtime-health-light/);
  assert.match(operationsCss, /\.server-runtime-health-light\s*\{/);
  assert.doesNotMatch(operationsCss, /\.server-runtime-diagnostic-list\s*\{/);
});

test("player center presents unavailable identity capability without development placeholders", () => {
  const livePlayerState = fs.readFileSync(
    path.join(desktopRoot, "src", "views", "servers", "player-center", "LivePlayerState.tsx"),
    "utf8"
  );
  assert.match(livePlayerState, /Player-list adapter unavailable/);
  assert.match(livePlayerState, /Player count available; names unavailable/);
  assert.doesNotMatch(livePlayerState, /game does not provide|fabricate member rows/);
  assert.doesNotMatch(playerAccessWorkbench, /player-access-adapter-pending|adapterPendingReason|adapterPendingVerification/);
  assert.doesNotMatch(`${playerAccessControlCss}\n${playerCenterCss}`, /player-access-adapter-pending/);
});

test("player, access, and mod styles have one explicit owner", () => {
  const legacyStyles = path.join(desktopRoot, "src", "views", "servers", "workbench", "players-mods.css");
  assert.equal(fs.existsSync(legacyStyles), false);
  assert.match(workbenchEntryCss, /@import "\.\/workbench\/player-center\.css";/);
  assert.match(workbenchEntryCss, /@import "\.\/workbench\/player-access-control\.css";/);
  assert.match(workbenchEntryCss, /@import "\.\/workbench\/mods\.css";/);
  assert.match(playerCenterCss, /\.player-center-player-table\s*\{/);
  assert.match(playerAccessControlCss, /\.player-access-selected-actions\s*\{/);
  assert.doesNotMatch(playerAccessControlCss, /\.player-access-roster-action-panel\s*\{/);
  assert.match(modWorkbenchCss, /\.mw-workbench\s*\{/);
  assert.doesNotMatch(playerCenterCss, /\.player-access-roster-(?:manager|nav|entry)\b/);
  assert.doesNotMatch(cssRuleSubjects(playerAccessControlCss), /\.player-center-(?:player-table|member-pane)\b|\.mw-workbench\b/);
  assert.doesNotMatch(modWorkbenchCss, /\.player-(?:center|access)-/);
  assert.doesNotMatch(
    `${playerCenterCss}\n${playerAccessControlCss}\n${modWorkbenchCss}`,
    /workbench-card--combined|player-access-command-response|player-access-adapter-pending|server-mod-/
  );
});

test("runtime console primary title is branded as LanGameCMD", () => {
  assert.match(runtimeSurfaceWorkbench, /"servers\.runtimeSurface\.managedConsole", undefined, "LanGameCMD"/);
  assert.match(runtimeSurfaceWorkbench, /"servers\.runtimeSurface\.primaryProcess", undefined, "LanGameCMD"/);
  assert.match(runtimeSurfaceWorkbench, /!selectedConsoleTab\?\.isPrimary/);

  assert.match(zhExtraMessages, /"servers\.runtimeSurface\.managedConsole":\s*"LanGameCMD"/);
  assert.match(zhExtraMessages, /"servers\.runtimeSurface\.primaryProcess":\s*"LanGameCMD"/);
  assert.match(zhExtraMessages, /"servers\.runtimeSurface\.primaryProcessMeta":\s*"[^"]*LanGameCMD[^"]*"/);
  assert.doesNotMatch(zhExtraMessages, /"servers\.runtimeSurface\.primaryProcessMeta":\s*"[^"]*主进程/);
  assert.match(enExtraMessages, /"servers\.runtimeSurface\.managedConsole":\s*"LanGameCMD"/);
  assert.match(enExtraMessages, /"servers\.runtimeSurface\.primaryProcess":\s*"LanGameCMD"/);
  assert.match(enExtraMessages, /"servers\.runtimeSurface\.primaryProcessMeta":\s*"[^"]*LanGameCMD[^"]*"/);
  assert.doesNotMatch(enExtraMessages, /"servers\.runtimeSurface\.primaryProcessMeta":\s*"[^"]*primary process/i);
});
