const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const React = require("react");
const { renderToStaticMarkup } = require("react-dom/server");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (module, filename) => {
    module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  };
}
const playerCenterRoot = path.resolve(__dirname, "../src/views/servers/player-center");
const { LivePlayerTable } = require(path.join(playerCenterRoot, "LivePlayerTable.tsx"));
const { LivePlayerState } = require(path.join(playerCenterRoot, "LivePlayerState.tsx"));
const { deriveLivePlayerPresentation } = require(path.resolve(__dirname, "../src/domain/live-player-state.ts"));

const player = {
  player_key: "query:1", display_name: "Alice <script>", identifiers: [], available_action_ids: [],
  ping_ms: null, session_started_at_unix_ms: null, role: null, attributes: []
};

test("query names render escaped and read-only without an account identity column", () => {
  const html = renderToStaticMarkup(React.createElement(LivePlayerTable, {
    locale: "zh-CN", now: 1_000, onSelect() {}, rows: [player], selectedPlayerKey: null
  }));
  assert.match(html, /Alice &lt;script&gt;/);
  assert.doesNotMatch(html, /<script>|<button|账号身份/);
  assert.match(html, /在线/);
});

test("actionable players remain selectable and retained rows identify their old status", () => {
  const html = renderToStaticMarkup(React.createElement(LivePlayerTable, {
    locale: "zh-CN", now: 1_000, onSelect() {}, rows: [{ ...player, available_action_ids: ["kick"], identifiers: [{ kind: "steam_id", value: "76561198000000001", stable: true }] }], selectedPlayerKey: "query:1", stale: true
  }));
  assert.match(html, /<button/);
  assert.match(html, /aria-pressed="true"/);
  assert.match(html, /账号身份/);
  assert.match(html, /上次在线/);
});

test("capability and credential states provide distinct actionable Chinese copy", () => {
  const cases = [
    ["adapter-unavailable", "adapter_unavailable", /在线玩家读取尚未接入/],
    ["count-only", "names_unavailable", /已取得在线人数，暂无玩家名称/],
    ["failed-without-rows", "authentication_failed", /玩家查询认证失败/],
    ["failed-without-rows", "query_unavailable", /玩家查询端口未响应/],
    ["failed-without-rows", "extension_unavailable", /Ficsit Remote Monitoring/]
  ];
  for (const [kind, code, expected] of cases) {
    const html = renderToStaticMarkup(React.createElement(LivePlayerState, {
      error: null, locale: "zh-CN", onOpenSettings() {}, moduleId: code === "extension_unavailable" ? "satisfactory" : undefined,
      presentation: { kind, tableVisible: false }, snapshot: { issue: { code, setting_keys: [], summary: "" } }
    }));
    assert.match(html, expected);
    assert.doesNotMatch(html, /游戏不提供|伪造|fabricate/);
    if (code === "authentication_failed") assert.match(html, /打开配置/);
    if (code === "extension_unavailable") assert.match(html, /HTTP 自动启动/);
  }
});

test("console response closure offers settings without claiming proven authentication failure", () => {
  const html = renderToStaticMarkup(React.createElement(LivePlayerState, {
    error: null, locale: "zh-CN", onOpenSettings() {},
    presentation: { kind: "failed-without-rows", tableVisible: false },
    snapshot: { issue: { code: "io_failed", setting_keys: ["console_password"], summary: "" } }
  }));
  assert.match(html, /控制台在返回名单前关闭了连接/);
  assert.match(html, /打开配置/);
  assert.doesNotMatch(html, /认证失败/);
});

test("private Steam queries explain each native visibility limit without reporting an empty server", () => {
  for (const [moduleId, settingKey, expectedZh, expectedEn] of [
    ["valheim", "public_server", /私密服务器不提供 Steam 玩家查询/, /Private Valheim servers do not expose/],
    ["vrising", "list_on_steam", /关闭“列入 Steam 列表”后不提供 Steam 玩家查询/, /V Rising does not expose Steam player queries/],
    ["abioticfactor", "lan_only", /“仅限局域网”模式不提供 Steam 玩家查询/, /Abiotic Factor does not expose Steam player queries/]
  ]) {
    const snapshot = {
      status: "unsupported", source: "server_query", complete: false, entries: [],
      current_players: null, max_players: null, stale: false,
      issue: { code: "query_unavailable", setting_keys: [settingKey], summary: "" }
    };
    const presentation = deriveLivePlayerPresentation(snapshot, 1_000, null);
    assert.equal(presentation.kind, "unsupported");
    assert.equal(presentation.authoritativeEmpty, false);
    for (const [locale, expected] of [["zh-CN", expectedZh], ["en-US", expectedEn]]) {
      const html = renderToStaticMarkup(React.createElement(LivePlayerState, {
        error: null, locale, moduleId, onOpenSettings() {}, presentation, snapshot
      }));
      assert.match(html, expected);
      assert.match(html, /打开配置|Open settings/);
      assert.doesNotMatch(html, /玩家快照刷新失败|Player snapshot refresh failed|当前没有在线玩家|No players are online/);
    }
  }
});

test("untracked running servers explain recovery without suggesting settings or enabling actions", () => {
  const snapshot = {
    instance_id: "surviving-server", snapshot_id: "untracked-query", source: "http_api", status: "failed",
    observed_at_unix_ms: null, expires_at_unix_ms: null, complete: false, truncated: false,
    stale: false, current_players: null, max_players: null, entries: [],
    issue: { code: "process_untracked", setting_keys: [], summary: "" }
  };
  const presentation = deriveLivePlayerPresentation(snapshot, 1_000, null);
  assert.equal(presentation.kind, "failed-without-rows");
  assert.equal(presentation.authoritativeEmpty, false);
  assert.equal(presentation.actionsEnabled, false);
  for (const [locale, expected] of [
    ["zh-CN", /服务器仍在运行.*正常停服后重新启动/],
    ["en-US", /The server is running.*Stop it normally in LanGame/]
  ]) {
    const html = renderToStaticMarkup(React.createElement(LivePlayerState, {
      error: null, locale, onOpenSettings() {}, presentation, snapshot
    }));
    assert.match(html, expected);
    assert.doesNotMatch(html, /<button|服务器尚未运行|当前没有在线玩家|No players are online|Server is not running/);
  }
});

test("Moria native-console labels remain intact and read-only without inferred account identities", () => {
  const rows = ["Dwarf (opaque-one)", "Dwarf (opaque-two)", "矮人 (矿工) (<unverified-token>)"].map((display_name, index) => ({
    ...player, player_key: `returntomoria:fixture:${index}`, display_name
  }));
  const snapshot = {
    instance_id: "moria-ui", snapshot_id: "moria-ready", source: "native_console", status: "ready",
    observed_at_unix_ms: 1_000, expires_at_unix_ms: 31_000, complete: true, truncated: false,
    stale: false, current_players: 3, max_players: 8, entries: rows, issue: null
  };
  const presentation = deriveLivePlayerPresentation(snapshot, 1_000, rows[0].player_key);
  assert.equal(presentation.kind, "ready");
  assert.equal(presentation.tableVisible, true);
  assert.equal(presentation.actionPanelVisible, false);
  assert.equal(presentation.actionsEnabled, false);
  const html = renderToStaticMarkup(React.createElement(LivePlayerTable, {
    locale: "zh-CN", now: 1_000, onSelect() {}, rows: presentation.rows, selectedPlayerKey: null
  }));
  assert.match(html, /Dwarf \(opaque-one\)/);
  assert.match(html, /Dwarf \(opaque-two\)/);
  assert.match(html, /矮人 \(矿工\) \(&lt;unverified-token&gt;\)/);
  assert.doesNotMatch(html, /<button|<unverified-token>|账号身份/);
});

test("Moria disabled console exposes settings instead of an unsupported or empty roster", () => {
  const snapshot = {
    instance_id: "moria-ui", snapshot_id: "moria-config", source: "native_console", status: "misconfigured",
    observed_at_unix_ms: null, expires_at_unix_ms: null, complete: false, truncated: false,
    stale: false, current_players: 2, max_players: 8, entries: [],
    issue: { code: "runtime_action_unavailable", setting_keys: ["console_enabled"], summary: "" }
  };
  const presentation = deriveLivePlayerPresentation(snapshot, 1_000, null);
  assert.equal(presentation.kind, "misconfigured");
  assert.equal(presentation.tableVisible, false);
  assert.equal(presentation.authoritativeEmpty, false);
  const html = renderToStaticMarkup(React.createElement(LivePlayerState, {
    error: null, locale: "zh-CN", onOpenSettings() {}, presentation, snapshot
  }));
  assert.match(html, /玩家查询配置不完整/);
  assert.match(html, /<button[^>]*>打开配置<\/button>/);
  assert.doesNotMatch(html, /当前没有在线玩家|尚未接入|不提供玩家身份列表/);
});

for (const moduleId of ["windrose", "runescapedragonwilds"]) {
test(`${moduleId} missing extension names its loader and query mod without FRM instructions`, () => {
  const html = renderToStaticMarkup(React.createElement(LivePlayerState, {
    error: null, locale: "zh-CN", moduleId,
    presentation: { kind: "failed-without-rows", tableVisible: false },
    snapshot: { issue: { code: "extension_unavailable", setting_keys: [], summary: "" } }
  }));
  assert.match(html, /UE4SS/);
  assert.match(html, /LgsmPlayerQuery/);
  assert.doesNotMatch(html, /Ficsit|Satisfactory|当前没有在线玩家/);
  if (moduleId === "runescapedragonwilds") assert.match(html, /Dragonwilds.*专服代理/);
});
}
