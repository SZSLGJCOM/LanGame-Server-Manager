const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const desktopRoot = path.resolve(__dirname, "..");
require.extensions[".ts"] = function compileTypeScript(module, filename) {
  const source = fs.readFileSync(filename, "utf8");
  const outputText = transpileTypeScript(source, filename);
  module._compile(outputText, filename);
};

function readSource(...segments) {
  return fs.readFileSync(path.join(desktopRoot, "src", ...segments), "utf8");
}

const serversView = readSource("views", "ServersView.tsx");
const playerCenter = readSource("views", "servers", "PlayerCenterWorkbench.tsx");
const useLivePlayers = readSource("views", "servers", "player-center", "use-live-players.ts");
const onlinePlayers = readSource("views", "servers", "player-center", "OnlinePlayersView.tsx");
const liveTable = readSource("views", "servers", "player-center", "LivePlayerTable.tsx");
const actionPanel = readSource("views", "servers", "player-center", "LivePlayerActionPanel.tsx");
const manualActions = readSource("views", "servers", "player-center", "ManualPlayerActions.tsx");
const manualModel = readSource("views", "servers", "player-center", "manual-player-action-model.ts");
const accessControl = readSource("views", "servers", "player-center", "use-player-access.tsx");
const rosterManager = readSource("views", "servers", "player-center", "PlayerAccessRosterEditor.tsx");
const rosterActions = readSource("views", "servers", "player-center", "SelectedPlayerRosterActions.tsx");
const rosterList = readSource("views", "servers", "player-center", "PlayerAccessRosterList.tsx");
const playerCenterCss = readSource("views", "servers", "workbench", "player-center.css");
const playerAccessControlCss = readSource("views", "servers", "workbench", "player-access-control.css");
const modsCss = readSource("views", "servers", "workbench", "mods.css");
const workbenchCss = readSource("views", "servers", "workbench.css");
const { EN_US_EXTRA_MESSAGES } = require(path.join(desktopRoot, "src", "i18n-messages-en-extra.ts"));
const { ZH_CN_EXTRA_MESSAGES } = require(path.join(desktopRoot, "src", "i18n-messages-zh-extra.ts"));

function translate(catalog, key, params = {}) {
  return String(catalog[key]).replace(/\{\s*([\w.]+)\s*\}/g, (match, paramKey) =>
    params[paramKey] == null ? match : String(params[paramKey])
  );
}

test("server detail mounts the online-first Player Center and removes the mixed workbench", () => {
  assert.match(serversView, /import \{ PlayerCenterWorkbench \} from "\.\/servers\/PlayerCenterWorkbench"/);
  assert.match(serversView, /activeDetailTab === "players"[\s\S]*?<PlayerCenterWorkbench/);
  assert.equal(fs.existsSync(path.join(desktopRoot, "src", "views", "servers", "PlayerAccessWorkbench.tsx")), false);
  assert.doesNotMatch(serversView, /PlayerManagementWorkbench/);
});

test("Player Center keeps players and capability-gated controls in one workspace", () => {
  assert.match(playerCenter, /createPlayerCenterState\(\{/);
  assert.match(playerCenter, /const access = usePlayerAccess\(props\)/);
  assert.match(playerCenter, /access=\{access\}/);
  assert.doesNotMatch(playerCenter, /activeSubview|role="tablist"|role="tab"|role="tabpanel"/);
  assert.doesNotMatch(playerCenter, /history|开发中|development placeholder/i);
  assert.match(onlinePlayers, /className="player-center-controls"/);
  assert.match(onlinePlayers, /props\.access\?\.fields/);
  assert.match(onlinePlayers, /className="player-center-list-tabs" role="tablist"/);
  assert.match(onlinePlayers, /data-list-key=\{tab\.key\} aria-selected=/);
  assert.doesNotMatch(onlinePlayers, /countUnavailable|AccessControlView|player-access-roster-management/);
  assert.match(onlinePlayers, /className="player-center-total-controls"/);
  assert.doesNotMatch(onlinePlayers, /player-center-online-controls|player-center-roster-controls|hidden=\{/);
  assert.match(rosterActions, /\["add", "remove"\]/);
});

test("live player lifecycle uses typed read, refresh, and expiry scheduling", () => {
  assert.match(useLivePlayers, /readInstanceLivePlayers/);
  assert.match(useLivePlayers, /refreshInstanceLivePlayers/);
  assert.match(useLivePlayers, /new LivePlayerRefreshController/);
  assert.match(useLivePlayers, /snapshot\.status === "refreshing" \|\| snapshot\.status === "failed"/);
  assert.match(useLivePlayers, /visibilitychange/);
  assert.doesNotMatch(useLivePlayers, /setInterval|sendInstanceRuntimeCommand|log_tail/);
  assert.doesNotMatch(`${playerCenter}\n${onlinePlayers}`, /onSendRuntimeCommand|runtimeActionTarget/);
});

test("online members render as a real selectable table and a separate action pane", () => {
  assert.match(liveTable, /<table className="player-center-player-table">/);
  assert.match(liveTable, /<button[\s\S]*?className="player-center-player-select"/);
  assert.match(liveTable, /player\.player_key === props\.selectedPlayerKey/);
  assert.match(liveTable, /props\.onSelect\(player\.player_key\)/);
  assert.doesNotMatch(liveTable, /<div[^>]*role="button"/);
  assert.match(onlinePlayers, /<LivePlayerActionPanel/);
  assert.match(onlinePlayers, /props\.manualActions/);
  assert.match(actionPanel, /<aside className="player-center-member-pane"/);
  assert.match(onlinePlayers, /className="player-center-controls">\s*<header className="player-center-member-header"/);
  assert.doesNotMatch(actionPanel, /player-center-member-header|player-center-member-heading/);
  assert.match(playerCenter, /playerActionIds=\{props\.moduleDetails\?\.runtime\?\.player_management\?\.status === "pending_adapter"/);
  assert.match(onlinePlayers, /actionIds=\{actionIds\}/);
  assert.match(actionPanel, /props\.actionIds\.map\(\(actionId\)/);
  assert.doesNotMatch(actionPanel, /player-center-member-prompt|props\.player\.available_action_ids\.map/);
  assert.match(playerCenterCss, /\.player-center-member-layout[\s\S]*?grid-template-columns:\s*minmax\(0, 1fr\)\s+minmax\(/);
  assert.doesNotMatch(playerCenterCss, /auto-fit|auto-fill/);
});

test("snapshot actions submit only instance, snapshot, player, and action references", () => {
  const executeInput = actionPanel.slice(
    actionPanel.indexOf("(props.onExecute ?? executeInstancePlayerAction)({"),
    actionPanel.indexOf("});", actionPanel.indexOf("(props.onExecute ?? executeInstancePlayerAction)({")) + 3
  );
  for (const key of ["instance_id", "snapshot_id", "player_key", "action_id"]) {
    assert.match(executeInput, new RegExp(`${key}:`));
  }
  assert.doesNotMatch(executeInput, /target|transport|command|password|port/);
  assert.match(readSource("App.tsx"), /onExecutePlayerAction=\{handleExecutePlayerAction\}/);
  for (const routedView of [readSource("views", "AppViewRouter.tsx"), readSource("views", "ServerWorkspaceView.tsx"), serversView, playerCenter]) {
    assert.match(routedView, /onExecutePlayerAction=\{props\.onExecutePlayerAction\}/);
  }
  assert.match(onlinePlayers, /onExecute=\{props\.onExecutePlayerAction\}/);
  assert.match(actionPanel, /!props\.actionIds\.includes\(actionId\) \|\| !player\.available_action_ids\.includes\(actionId\)/);
  assert.match(actionPanel, /const disabled = !props\.actionsEnabled \|\| hasPendingRequest \|\| !props\.player\?\.available_action_ids\.includes\(actionId\)/);
  assert.match(actionPanel, /return danger \? \([\s\S]*?<InlineConfirmAction/);
  assert.match(actionPanel, /scopeKey=\{JSON\.stringify\(\[props\.snapshot\?\.instance_id, props\.snapshot\?\.snapshot_id, props\.player\?\.player_key, actionId\]\)\}/);
  assert.doesNotMatch(actionPanel, /window\.confirm/);
});

test("manual ID fallback stays declared, secondary, and command-free", () => {
  assert.match(manualModel, /moduleDetails\.runtime\.player_list/);
  assert.match(manualModel, /filterConsumedPlayerAccessActions/);
  assert.match(manualModel, /\.filter\(actionNeedsManualTarget\)/);
  assert.match(manualActions, /executeDeclaredRuntimePlayerAction/);
  assert.match(manualActions, /props\.actions\.filter\(actionNeedsManualTarget\)/);
  assert.match(manualActions, /value=\{target\}/);
  assert.match(manualActions, /roleValues = action\?\.role_values/);
  assert.doesNotMatch(manualActions, /sendInstanceRuntimeCommand|command_template|transport|password_setting_key|response_text/);
  assert.doesNotMatch(`${manualActions}\n${manualModel}`, /\.split\("\{\{target\}\}"\)|renderRuntimeActionCommand/);
  assert.match(manualActions, /action\.destructive \? \([\s\S]*?<InlineConfirmAction/);
  assert.match(manualActions, /scopeKey=\{JSON\.stringify\(\[props\.instanceId, action\.id, target\.trim\(\), selectedRole\]\)\}/);
  assert.doesNotMatch(manualActions, /window\.confirm/);
});

test("Player Center static copy resolves through complete en-US and zh-CN catalogs", () => {
  const sources = [playerCenter, onlinePlayers, actionPanel, manualActions].join("\n");
  const keys = [...new Set(Array.from(
    sources.matchAll(/\b(?:props\.)?t\("([^"]+)"/g),
    (match) => match[1]
  ))];

  assert.ok(keys.length > 20, "Player Center should keep its static UI copy in the shared catalog");
  for (const key of keys) {
    assert.equal(typeof EN_US_EXTRA_MESSAGES[key], "string", `missing en-US ${key}`);
    assert.equal(typeof ZH_CN_EXTRA_MESSAGES[key], "string", `missing zh-CN ${key}`);
    assert.notEqual(EN_US_EXTRA_MESSAGES[key], ZH_CN_EXTRA_MESSAGES[key], `${key} should be localized`);
  }

  assert.equal(
    translate(EN_US_EXTRA_MESSAGES, "servers.playerCenter.member.confirm", { action: "Kick", player: "Alex" }),
    "Run “Kick” for “Alex”?"
  );
  assert.equal(
    translate(ZH_CN_EXTRA_MESSAGES, "servers.playerCenter.member.confirm", { action: "踢出", player: "Alex" }),
    "确认对「Alex」执行「踢出」？"
  );
  assert.doesNotMatch(sources, /selectLocaleText/);
});

test("Player Center keeps locale selection only at dynamic metadata and date boundaries", () => {
  assert.match(manualActions, /locale === "zh-CN" && action\.label_zh_cn/);
  assert.match(manualActions, /locale === "zh-CN" && action\.target_label_zh_cn/);
  assert.match(manualActions, /locale === "zh-CN" && action\.target_placeholder_zh_cn/);
  assert.match(actionPanel, /locale === "zh-CN" && action\?\.label_zh_cn/);
  assert.match(onlinePlayers, /Intl\.DateTimeFormat\(locale === "zh-CN" \? "zh-CN" : "en-US"/);
});

test("access control has one isolated owner and preserves CAS mutation semantics", () => {
  assert.match(playerCenter, /usePlayerAccess\(props\)/);
  assert.match(accessControl, /export function usePlayerAccess/);
  assert.match(accessControl, /onApplyPlayerAccessMutation/);
  assert.match(accessControl, /expectedValue: field\.kind === "string-scalar" \? field\.currentValue : undefined/);
  assert.match(onlinePlayers, /<SelectedPlayerRosterActions/);
  assert.match(rosterActions, /<PlayerAccessRosterEditor/);
  assert.equal(fs.existsSync(path.join(desktopRoot, "src", "views", "servers", "player-center", "PlayerAccessRosterManager.tsx")), false);
  assert.match(rosterList, /className="player-access-roster-entries"[\s\S]*?role="list"/);
  assert.match(rosterList, /className=\{`player-access-roster-entry[\s\S]*?role="listitem"/);
  assert.doesNotMatch(rosterManager, /role="list"|role="listitem"|player-access-roster-nav/);
  assert.doesNotMatch(rosterList, /onMutate|onRemove|InlineConfirmAction/);
  assert.doesNotMatch(accessControl, /OnlinePlayersView|readInstanceLivePlayers|executeInstancePlayerAction/);
  assert.doesNotMatch(`${accessControl}\n${rosterManager}`, /window\.confirm/);
  assert.match(rosterActions, /operation === "remove" \? entry\?\.rawValue : target\?\.rawValue/);
  assert.match(rosterActions, /scopeKey=\{JSON\.stringify\(\[field\.key, field\.currentValue, operation, rawValue\]\)\}/);
  assert.match(rosterManager, /JSON\.stringify\(\[field\.key, field\.currentValue, entry\?\.rawValue \?\? null, value\]\)/);
  assert.match(rosterManager, /onConfirm=\{saveDraft\}/);
  assert.match(rosterManager, /onConfirm=\{addDraft\}/);
  assert.equal((rosterManager.match(/onSubmit=\{\(event\) => event\.preventDefault\(\)\}/g) ?? []).length, 2,
    "form submission must not bypass roster confirmation");
});

test("frontend contains no DST player parser, raw Lua builder, or blind command poll", () => {
  const playerFiles = [playerCenter, useLivePlayers, onlinePlayers, liveTable, actionPanel, manualActions, accessControl].join("\n");
  assert.doesNotMatch(playerFiles, /LGM-DST-PLAYERS|TheNet:GetClientTable|TheNet:Kick|parseDstLivePlayerSnapshot/);
  assert.doesNotMatch(playerFiles, /setInterval\s*\(/);
  assert.doesNotMatch(playerFiles, /Array\.from\(\{ length: 6 \}/);
  assert.doesNotMatch(playerFiles, /adapterPendingReason|adapterPendingVerification/);
});

test("player center styles are split by feature ownership", () => {
  assert.equal(fs.existsSync(path.join(desktopRoot, "src", "views", "servers", "workbench", "players-mods.css")), false);
  assert.match(workbenchCss, /player-center\.css[\s\S]*player-access-control\.css[\s\S]*mods\.css/);
  assert.match(playerCenterCss, /\.player-center-member-layout\s*\{/);
  assert.match(playerAccessControlCss, /\.player-access-selected-actions\s*\{/);
  assert.doesNotMatch(playerAccessControlCss, /\.player-access-roster-action-panel\s*\{/);
  assert.match(modsCss, /\.mw-workbench\s*\{/);
  assert.doesNotMatch(playerAccessControlCss, /player-access-command-response|player-access-adapter-pending/);
});
