import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import React, { act, useState } from "react";
import { createRoot } from "react-dom/client";
import schema from "../../../../modules/enshrouded/schema.json";
import nativeHashFixture from "../../../../modules/enshrouded/reference-fixtures/2026-10-16-native_account_hash.json";
import { I18nProvider, useI18n } from "../../src/i18n";
import { createPlayerCenterState, derivePlayerCenterViewModel, reducePlayerCenterState } from "../../src/domain/live-player-state";
import { OnlinePlayersView } from "../../src/views/servers/player-center/OnlinePlayersView";
import { usePlayerAccess } from "../../src/views/servers/player-center/use-player-access";
import type { ExecuteInstancePlayerActionInput, InstanceDetails, ModuleDetails, ModulePlayerActionDetails, RuntimeLivePlayerEntry, RuntimeLivePlayerSnapshot } from "../../src/types";
import "../../src/app.css";
import "../../src/views/servers/workbench.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN");
document.documentElement.dataset.theme = "dark";
const nonce = new URLSearchParams(location.search).get("nonce");
const fixture = document.getElementById("fixture");
if (!fixture) throw new Error("Fixture root is missing");
const root = createRoot(fixture);
const checks: string[] = [];
const errors: string[] = [];
let commands = 0;
let mutations = 0;
const nativeRequests: ExecuteInstancePlayerActionInput[] = [];
const nativeSteamCase = nativeHashFixture.cases.find((entry) => entry.platform === 3 && entry.synthetic_platform_id === "76561197960265729");
if (!nativeSteamCase) throw new Error("Synthetic native Steam hash evidence is missing");
const nativeAccountId = nativeSteamCase.native_hash;
const nativeSteamId = nativeSteamCase.synthetic_platform_id;
const now = 1_000_000;
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
const player: RuntimeLivePlayerEntry = { player_key: "observed-player", display_name: "测试玩家 <PAMD>",
  identifiers: [], available_action_ids: [], ping_ms: null, session_started_at_unix_ms: null, role: null, attributes: [] };
const action: ModulePlayerActionDetails = { id: "kick", kind: "kick", label: "Kick", label_zh_cn: "踢出玩家",
  target_required: true, destructive: true, command_template: "kick {{target}}" };
const nativeActions: ModulePlayerActionDetails[] = [
  { ...action, id: "kick_player" },
  { ...action, id: "ban_player", kind: "ban", label: "Ban player", label_zh_cn: "封禁玩家" }
];
type Scenario = "enshrouded" | "read-only" | "workspace-read-only" | "expired" | "unavailable" | "metadata"
  | "enshrouded-native" | "enshrouded-native-banned" | "enshrouded-native-unknown" | "enshrouded-steam-only" | "enshrouded-native-stale" | "enshrouded-native-protected";

function Fixture({ scenario }: { scenario: Scenario }) {
  const { locale, t } = useI18n();
  const enshrouded = scenario.startsWith("enshrouded");
  const nativeScenario = enshrouded && scenario !== "enshrouded";
  const unknownExtension = scenario === "enshrouded-native-unknown";
  const steamOnly = scenario === "enshrouded-steam-only";
  const expired = scenario === "expired" || scenario === "enshrouded-native-stale";
  const workspaceReadOnly = scenario === "workspace-read-only";
  const sessionControls = expired || workspaceReadOnly || scenario === "unavailable";
  const moduleId = enshrouded ? "enshrouded" : "fixture";
  const row: RuntimeLivePlayerEntry = nativeScenario ? { ...player, player_key: "native-player",
    identifiers: unknownExtension ? [] : steamOnly ? [{ kind: "steam_id", value: nativeSteamId, stable: true }]
      : [{ kind: "steam_id", value: nativeSteamId, stable: true }, { kind: "enshrouded_account_id", value: nativeAccountId, stable: true }],
    available_action_ids: ["kick_player", "ban_player"], role: scenario === "enshrouded-native" ? "Admin" : null,
    attributes: unknownExtension || steamOnly ? [] : [{ key: "moderation_immune", value: scenario === "enshrouded-native-protected" ? "true" : "false" }] }
    : scenario === "metadata" ? { ...player,
    identifiers: [{ kind: "steam_id", value: "76561198000000001", stable: true }], ping_ms: 34,
    session_started_at_unix_ms: now - 3_660_000, role: "Guest", attributes: [{ key: "World", value: "测试世界" }] }
    : expired || workspaceReadOnly ? { ...player, available_action_ids: ["kick"] } : player;
  const snapshot: RuntimeLivePlayerSnapshot = { instance_id: `details-${scenario}`, snapshot_id: `snapshot-${scenario}`,
    source: nativeScenario && !unknownExtension ? "file_ipc" : "server_query", status: "ready", observed_at_unix_ms: now, expires_at_unix_ms: expired ? now - 1 : now + 30_000,
    stale: false, complete: true, truncated: false, current_players: 1, max_players: 16, entries: [row], issue: null };
  const [state, setState] = useState(() => createPlayerCenterState({ instanceId: snapshot.instance_id, snapshot }));
  const [bannedIds, setBannedIds] = useState(scenario === "enshrouded-native-banned" ? nativeAccountId : "");
  const details: InstanceDetails = { summary: { id: snapshot.instance_id, name: "Player details", module_id: moduleId,
    status: "Running", active_process_count: 1, autostart: false, bind_ip: "127.0.0.1" }, ports: [],
    settings_json: JSON.stringify({ banned_player_ids: bannedIds }), config_file_path: "", saves_path: "",
    backup_uses_declared_saves_path: false, auto_backup_on_stop: false, backup_retention_count: 3 };
  const moduleDetails: ModuleDetails = { summary: { id: moduleId, name: moduleId, version: "1", install_state: "Installed", supported_platforms: ["windows"] },
    default_ports: [], runtime: {}, schema_json: JSON.stringify(schema) };
  const access = usePlayerAccess({ details, moduleDetails, onApplyPlayerAccessMutation: async (input) => {
    mutations++;
    return { instanceId: input.instanceId, fieldKey: input.fieldKey, operation: input.operation,
      persistentStatus: "updated", liveStatus: "restart_required", verificationStatus: "unavailable" };
  } });
  return <main style={{ width: "1000px", height: "650px", margin: "32px auto", display: "grid", minHeight: 0 }}>
    <section className="player-access-workbench">
      <OnlinePlayersView moduleId={moduleId} readOnly={workspaceReadOnly} locale={locale} now={now} declaredActions={nativeScenario ? nativeActions : sessionControls ? [action] : []}
        playerActionIds={nativeScenario ? ["kick_player", "ban_player"] : sessionControls ? ["kick"] : []} error={null} loading={false} runtime={null} runtimeAvailable
        snapshot={state.snapshot} presentation={derivePlayerCenterViewModel(state, now).livePlayers} t={t}
        access={enshrouded ? access : undefined} onRefresh={async () => {}}
        onExecutePlayerAction={async (input) => {
          commands++;
          if (nativeScenario) {
            nativeRequests.push(input);
            if (input.action_id === "ban_player") setBannedIds(nativeAccountId);
          }
          return { action_id: input.action_id, status: "sent", executed_at_unix_ms: now, summary: "Sent" };
        }}
        onSelectPlayer={(playerKey) => setState((current) => reducePlayerCenterState(current, { type: "player-selected", playerKey }))} />
    </section>
  </main>;
}
function check(condition: unknown, description: string): asserts condition {
  if (!condition) throw new Error(`${description}: ${fixture?.textContent}`);
  checks.push(description);
}
function element<T extends Element>(selector: string): T {
  const found = fixture?.querySelector<T>(selector);
  if (!found) throw new Error(`Missing ${selector}`);
  return found;
}
async function draw(scenario: Scenario) {
  await act(async () => { root.render(<I18nProvider key={scenario}><Fixture scenario={scenario} /></I18nProvider>); });
  const deadline = performance.now() + 5000;
  while (!fixture?.querySelector(".player-center-player-select")) {
    if (performance.now() >= deadline) throw new Error(`Player workspace did not load: ${fixture?.textContent}`);
    await act(async () => { await new Promise<void>((resolve) => requestAnimationFrame(() => resolve())); });
  }
}
async function key(value: string) {
  await act(async () => {
    const response = await fetch(`/__reliability_key/${nonce}/${value}`, { method: "POST" });
    check(response.ok, `Browser dispatches ${value}`);
  });
}
async function selectWithKeyboard() {
  element<HTMLButtonElement>(".player-center-player-select").focus();
  await key("Enter");
  check(element<HTMLButtonElement>(".player-center-player-select").getAttribute("aria-pressed") === "true", "Keyboard selection is announced");
}
function verifyDisabledDescriptions() {
  const buttons = fixture?.querySelectorAll<HTMLButtonElement>(".player-center-member-action, .player-access-selected-action, .player-access-edit-selected") ?? [];
  for (const button of buttons) {
    check(button.disabled, "Unavailable operation stays disabled");
    const id = button.getAttribute("aria-describedby");
    check(id && document.getElementById(id)?.textContent?.trim(), "Disabled operation exposes a readable reason");
  }
}
function nativeAction(label: string): HTMLButtonElement {
  const found = Array.from(fixture?.querySelectorAll<HTMLButtonElement>(".player-center-member-action") ?? [])
    .find((button) => button.textContent?.trim() === label);
  if (!found) throw new Error(`Missing native action ${label}`);
  return found;
}
function describedReason(button: HTMLButtonElement): string {
  const id = button.getAttribute("aria-describedby");
  return id ? document.getElementById(id)?.textContent ?? "" : "";
}
// The right panel no longer repeats the selected identity; the highlighted left row is the target.
function selectedRowText() {
  const pressed = document.querySelector('.player-center-list-pane [aria-pressed="true"]');
  return (pressed?.closest('tr, [role="listitem"]') ?? pressed)?.textContent ?? "";
}
async function run() {
  await act(prepareBrowserLocaleCatalogs);
  await draw("enshrouded"); await selectWithKeyboard(); verifyDisabledDescriptions();
  check(fixture?.textContent?.includes("匿名查询记录的时长无法可靠对应到玩家姓名"), "Unknown duration explains anonymous player records");
  check(fixture?.textContent?.includes("Steam ID 不能替代") && fixture.textContent.includes("停服编辑，启动后生效"), "Native bans explain their exact identity and lifecycle");
  check(!fixture?.querySelector(".player-center-member-action") && mutations === 0,
    "Keyboard selects an Enshrouded name without granting native account authority");
  await draw("read-only"); await selectWithKeyboard();
  check(selectedRowText() && !fixture?.querySelector(".player-access-selected-action, .player-center-member-action"),
    "A game with no player actions still exposes selected details");
  await draw("workspace-read-only"); await selectWithKeyboard(); verifyDisabledDescriptions();
  await act(async () => { element<HTMLButtonElement>(".player-center-member-action").click(); });
  check(commands === 0 && !fixture?.querySelector(".inline-confirm-review") && fixture?.textContent?.includes("当前处于只读模式"),
    "Read-only inspection rejects a live command even when the supplied snapshot is actionable");
  await draw("expired"); await selectWithKeyboard(); verifyDisabledDescriptions();
  element<HTMLButtonElement>(".player-center-member-action").click();
  check(commands === 0 && fixture?.textContent?.includes("快照尚未确认完整且有效"), "Inspecting expired players never sends live commands");
  await draw("unavailable"); await selectWithKeyboard(); verifyDisabledDescriptions();
  check(fixture?.textContent?.includes("未提供此操作所需的身份") && !fixture.textContent.includes("快照尚未确认完整且有效"),
    "A fresh read-only player explains missing action identity without requesting a refresh");
  await draw("metadata"); await selectWithKeyboard();
  check(["76561198000000001", "1 小时 1 分", "34 ms", "Guest", "测试世界"].every((value) => selectedRowText().includes(value)),
    "Selected details preserve every supplied player field");
  await draw("enshrouded-native-unknown"); await selectWithKeyboard(); verifyDisabledDescriptions();
  check(fixture?.textContent?.includes("扩展未提供有效玩家身份")
    && !fixture.textContent.includes("未连接原生管理接口") && commands === 0,
    "Unknown native extension preserves read-only fallback despite declared actions");
  await draw("enshrouded-steam-only"); await selectWithKeyboard(); verifyDisabledDescriptions();
  await act(async () => { nativeAction("封禁玩家").click(); });
  check(nativeRequests.length === 0 && !fixture?.querySelector(".inline-confirm-review"), "Steam identity alone never authorizes a native account action");
  await draw("enshrouded-native-protected"); await selectWithKeyboard(); verifyDisabledDescriptions();
  check(element<HTMLElement>(".server-player-profile-copy").textContent === player.display_name
    && fixture?.querySelector(".player-center-details")?.textContent?.includes("受服务器保护是"),
    "Native protection uses labeled details without a raw true subtitle");
  check(["踢出玩家", "封禁玩家"].every((label) => describedReason(nativeAction(label)) === "服务器保护此玩家，原生权限不允许踢出或封禁。"),
    "Native protection explains disabled kick and ban without inferring permissions from a role name");
  await act(async () => { nativeAction("踢出玩家").click(); nativeAction("封禁玩家").click(); });
  check(nativeRequests.length === 0 && !fixture?.querySelector(".inline-confirm-review"), "Protected native players cannot dispatch moderation commands");
  await draw("enshrouded-native-stale"); await selectWithKeyboard();
  check(nativeAction("封禁玩家").disabled && describedReason(nativeAction("封禁玩家")).includes("快照尚未确认完整且有效"),
    "Expired native identity remains inspectable without live authority");
  await draw("enshrouded-native-banned"); await selectWithKeyboard();
  check(nativeAction("封禁玩家").disabled && describedReason(nativeAction("封禁玩家")) === "此玩家已在封禁名单中。"
    && !nativeAction("踢出玩家").disabled, "An existing native ban explains only its disabled ban action");
  await draw("enshrouded-native"); await selectWithKeyboard();
  check(element<HTMLElement>(".server-player-profile-copy").textContent === player.display_name
    && fixture?.querySelector(".player-center-details")?.textContent?.includes("受服务器保护否"),
    "Unprotected native players use labeled details without a raw false subtitle");
  check(fixture?.querySelector(".player-center-details")?.textContent?.includes(`原生 accountId${nativeAccountId}`)
    && !fixture.textContent?.includes("未连接原生管理接口")
    && !fixture.textContent?.includes("匿名查询记录的时长无法可靠对应到玩家姓名")
    // Connected state needs no standing note; no read-only explanation remains.
    && !fixture.textContent?.includes("扩展未提供有效玩家身份"),
    "Verified native identity replaces unavailable-adapter and anonymous-duration copy");
  check(!fixture?.querySelector('[data-roster-field="banned_player_ids"] [data-operation="add"]')
    && !nativeAction("封禁玩家").disabled, "Native live ban replaces the duplicate persistent add operation");
  await act(async () => { nativeAction("封禁玩家").click(); });
  await act(async () => { element<HTMLButtonElement>(".inline-confirm-submit").click(); });
  check(nativeRequests.length === 1 && nativeRequests[0].action_id === "ban_player" && nativeRequests[0].player_key === "native-player"
    && Object.keys(nativeRequests[0]).sort().join(",") === "action_id,instance_id,player_key,snapshot_id" && mutations === 0,
    "Native ban dispatch retains snapshot references without substituting Steam or persistent mutations");
  check(nativeAction("封禁玩家").disabled && describedReason(nativeAction("封禁玩家")) === "此玩家已在封禁名单中。",
    "The saved native ban disables repeated live bans after authoritative details refresh");
  await draw("enshrouded-native-banned"); await selectWithKeyboard();
  const controls = element<HTMLElement>(".player-center-controls");
  check(controls.scrollWidth <= controls.clientWidth + 1, "Player details fit the controls column");
  check(errors.length === 0, "Browser and React report no errors");
  return { status: "passed", checks, browser_errors: errors };
}
void run().catch((error) => ({ status: "failed", error: String(error?.stack ?? error), checks, browser_errors: errors }))
  .then((result) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(result) }));
