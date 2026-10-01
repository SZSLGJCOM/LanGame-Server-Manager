import React, { act, StrictMode, useState } from "react";
import { createRoot } from "react-dom/client";
import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { I18nProvider } from "../../src/i18n";
import { PlayerCenterWorkbench } from "../../src/views/servers/PlayerCenterWorkbench";
import type { InstanceDetails, ModuleDetails, RuntimeLivePlayerEntry, RuntimeLivePlayerSnapshot } from "../../src/types";
import "../../src/app.css";
import "../../src/views/servers/workbench.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true, isTauri: true });
localStorage.setItem("langame.locale", "zh-CN");
document.documentElement.dataset.theme = "dark";
const errors: string[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
const nonce = new URLSearchParams(location.search).get("nonce");
let checks = 0;
let propsRefreshes = 0;
let snapshotRefreshes = 0;
let reads = 0;
let onlinePlayers: RuntimeLivePlayerEntry[] = [];
let updateProps: () => void;
let switchInstance: (instanceId: string) => void;
const player: RuntimeLivePlayerEntry = { player_key: "alice", display_name: "Alice",
  identifiers: [{ kind: "steam_id", value: "76561198000000001", stable: true }],
  available_action_ids: ["kick"], ping_ms: 20, session_started_at_unix_ms: null, role: null, attributes: [] };
const initialDetails: InstanceDetails = {
  summary: { id: "refresh-one", name: "Refresh server", module_id: "refresh-fixture", status: "Running",
    active_process_count: 1, autostart: false, bind_ip: "127.0.0.1" },
  ports: [], settings_json: JSON.stringify({ blocklist: [] }), config_file_path: "", saves_path: "",
  backup_uses_declared_saves_path: false, auto_backup_on_stop: false, backup_retention_count: 3
};
function moduleDetails(): ModuleDetails {
  return { summary: { id: "refresh-fixture", name: "Refresh fixture", version: "1", install_state: "Installed", supported_platforms: ["windows"] },
    default_ports: [], runtime: { player_list: {
      scope: "online", source: "runtime_action", action_id: null, player_action_ids: ["kick"],
      response_codec: "rust_player_list", identity_kind: "steam_id", refresh_interval_ms: 60_000
    }, player_actions: [
      { id: "kick", kind: "kick", label: "Kick", command_template: "kick {{target}}", target_required: true, destructive: true },
      { id: "lookup", kind: "custom", label: "Look up player", command_template: "lookup {{target}}", target_required: true }
    ] },
    schema_json: JSON.stringify({ type: "object", properties: {
      blocklist: { type: "array", title: "黑名单", items: { type: "string" }, default: [],
        "x-lsgm-player-access-kind": "block", "x-lsgm-player-access-codec": "steam64", "x-lsgm-player-access-sync": { mode: "restart" } }
    } }) };
}
function check(condition: unknown, message: string): asserts condition { if (!condition) throw new Error(message); }
mockIPC((command, args) => {
  check(command === "read_instance_live_players" || command === "refresh_instance_live_players", `Unexpected native command: ${command}`);
  check(args && typeof args.instanceId === "string", "Live player request has no instance ID");
  if (command === "refresh_instance_live_players") snapshotRefreshes++;
  else reads++;
  const now = Date.now();
  const snapshot: RuntimeLivePlayerSnapshot = { snapshot_id: `snapshot-${reads}-${snapshotRefreshes}`, instance_id: args.instanceId,
    status: "ready", source: "runtime_action", observed_at_unix_ms: now, expires_at_unix_ms: now + 60_000,
    complete: true, truncated: false, stale: false, current_players: onlinePlayers.length, max_players: 20,
    entries: structuredClone(onlinePlayers), issue: null };
  return snapshot;
});
function Fixture() {
  const [instanceId, setInstanceId] = useState("refresh-one");
  const [revision, setRevision] = useState(0);
  switchInstance = setInstanceId;
  updateProps = () => { propsRefreshes++; setRevision((value) => value + 1); };
  const details = { ...initialDetails, summary: { ...initialDetails.summary, id: instanceId },
    settings_json: JSON.stringify({ blocklist: [], fixture_revision: revision }) };
  return <main style={{ width: "720px", height: "500px", margin: "24px auto", display: "grid", minHeight: 0 }}>
    <section className="server-detail-panel"><div className="detail-stack detail-stack--server">
      <div className="server-detail-subheader" style={{ minHeight: "42px" }}>玩家</div>
      <div className="server-detail-scroll server-detail-scroll--players">
        <PlayerCenterWorkbench details={details} moduleDetails={moduleDetails()} runtime={null}
          onApplyPlayerAccessMutation={async () => { throw new Error("Refresh fixture must never mutate player access"); }} />
      </div>
    </div></section>
  </main>;
}
function select<T extends Element>(selector: string): T {
  const element = document.querySelector<T>(selector);
  check(element, `Missing element: ${selector}`); return element;
}
async function waitFor(condition: () => boolean, label: string) {
  const deadline = performance.now() + 5000;
  while (!condition()) {
    check(performance.now() < deadline, `Timed out waiting for ${label}`);
    await act(async () => { await new Promise<void>((resolve) => setTimeout(resolve, 10)); });
  }
}
async function click(selector: string) {
  const element = select<HTMLElement>(selector);
  check(element.getClientRects().length > 0, `Cannot click hidden control: ${selector}`);
  await act(async () => { element.click(); });
}
async function input(selector: string, value: string) {
  const element = select<HTMLInputElement>(selector);
  check(element.getClientRects().length > 0, `Cannot edit hidden control: ${selector}`);
  await act(async () => {
    const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
    check(setter, "Input setter is missing"); setter.call(element, value);
    element.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
function verifyOneControlOfEachKind(label: string) {
  check(document.querySelectorAll('.player-center-list-pane [role="tablist"]').length === 1, `${label}: duplicate list switcher`);
  check(document.querySelectorAll('.player-center-list-tabs [data-list-key="blocklist"]').length === 1, `${label}: duplicate roster tab`);
  check(document.querySelectorAll("#player-access-roster-blocklist").length <= 1, `${label}: duplicate roster editor`);
  check(document.querySelectorAll(".player-center-manual-actions").length === 1, `${label}: duplicate manual actions`);
  check(!document.querySelector(".player-center-controls .player-access-roster-entries"), `${label}: roster rows remain in controls`);
}
function verifyOnlineActionsDisabled() {
  const controls = select<HTMLElement>(".player-center-total-controls");
  const actions = Array.from(controls.querySelectorAll<HTMLButtonElement>(".player-center-member-action"));
  const rosterActions = Array.from(controls.querySelectorAll<HTMLButtonElement>(".player-access-selected-action"));
  check(actions.length === 1 && actions[0].disabled, "Empty online list lost its declared action or kept it enabled");
  check(rosterActions.length === 2 && rosterActions.every((action) => action.disabled), "Missing target lost its fixed roster pair or kept it enabled");
  check(!controls.querySelector(".player-center-member-prompt, .player-center-member-heading strong[title], .player-center-member-heading > span, .inline-confirm-review"),
    "Unselected online controls retained a prompt, selected target, or confirmation");
}
async function listTab(key: string) { await click(`.player-center-list-tabs [data-list-key="${key}"]`); }
async function refresh() {
  const currentList = select<HTMLElement>('.player-center-list-tabs [aria-selected="true"]').dataset.listKey;
  await listTab("online");
  const previous = snapshotRefreshes;
  await click(".player-center-refresh-button");
  await waitFor(() => snapshotRefreshes === previous + 1 && !select<HTMLButtonElement>(".player-center-refresh-button").disabled, "snapshot refresh completion");
  if (currentList && currentList !== "online") await listTab(currentList);
}
function verifyDrafts() {
  check(select<HTMLDetailsElement>(".player-center-manual-actions").open, "Manual disclosure closed during refresh");
  check(select<HTMLInputElement>(".player-access-roster-input").value === "76561198000000009", "Roster draft was lost");
  check(select<HTMLInputElement>("input[id^='manual-player-target-']").value === "manual-draft", "Manual draft was lost");
}
async function run() {
  const root = createRoot(select<HTMLDivElement>("#fixture"));
  await act(async () => {
    await Promise.all([import("../../src/i18n-messages"), import("../../src/i18n-messages-zh-cn")]);
    root.render(<StrictMode><I18nProvider><Fixture /></I18nProvider></StrictMode>);
  });
  await waitFor(() => reads > 0 && Boolean(document.querySelector('.player-center-list-tabs [data-list-key="blocklist"]')), "real workbench mount");
  verifyOneControlOfEachKind("initial empty player list");
  verifyOnlineActionsDisabled(); checks++;

  for (let index = 0; index < 20; index++) {
    await act(async () => { updateProps(); }); verifyOneControlOfEachKind(`props refresh ${index}`);
    await refresh(); verifyOneControlOfEachKind(`snapshot refresh ${index}`);
  }
  const controls = select<HTMLElement>(".player-center-controls");
  check(controls.scrollHeight <= controls.clientHeight + 1, "Closed controls acquired unnecessary scroll height"); checks++;

  await click(".player-center-manual-actions > summary");
  await input("input[id^='manual-player-target-']", "manual-draft");
  await listTab("blocklist");
  await click(".player-access-roster-create");
  await input(".player-access-roster-input", "76561198000000009");
  for (let index = 0; index < 20; index++) {
    await act(async () => { updateProps(); }); verifyOneControlOfEachKind(`draft refresh ${index}`); verifyDrafts();
  }
  checks++;

  onlinePlayers = [player]; await refresh();
  await listTab("online");
  await waitFor(() => Boolean(document.querySelector(".player-center-player-select")), "online player");
  await click(".player-center-player-select");
  check(document.querySelector(".player-center-member-heading")?.textContent?.includes("Alice"), "Selected player's identity is missing");
  check(!select<HTMLButtonElement>(".player-center-member-action").disabled,
    "The snapshot-authorized action was not enabled after selecting its player");
  verifyOneControlOfEachKind("player panel shown");
  check(!document.querySelector(".player-access-roster-editor"), "Changing list retained an obsolete roster form");
  check(select<HTMLInputElement>("input[id^='manual-player-target-']").value === "manual-draft", "Authoritative refresh lost the independent manual draft"); checks++;

  onlinePlayers = []; await refresh();
  verifyOnlineActionsDisabled();
  verifyOneControlOfEachKind("player panel unselected again");
  check(!document.querySelector(".player-access-roster-editor"), "Removing the online target reopened a stale roster form"); checks++;

  await listTab("blocklist"); await click(".player-access-roster-create");
  await input(".player-access-roster-input", "76561198000000009");
  const oldRoster = select<HTMLInputElement>(".player-access-roster-input");
  const oldManual = select<HTMLDetailsElement>(".player-center-manual-actions");
  await act(async () => { switchInstance("refresh-two"); });
  await waitFor(() => Boolean(document.querySelector("#manual-player-target-refresh-two")), "new instance controls");
  verifyOneControlOfEachKind("instance switch");
  check(!oldRoster.isConnected && !oldManual.isConnected, "Old instance controls were retained");
  check(select<HTMLElement>('.player-center-list-tabs [data-list-key="online"]').getAttribute("aria-selected") === "true"
    && !select<HTMLDetailsElement>(".player-center-manual-actions").open, "New instance view was not reset");
  verifyOnlineActionsDisabled();
  await listTab("blocklist"); await click(".player-access-roster-create");
  check(select<HTMLInputElement>(".player-access-roster-input").value === "" && select<HTMLInputElement>("#manual-player-target-refresh-two").value === "", "Drafts leaked across instances"); checks++;

  check(errors.length === 0, `Browser errors: ${errors.join("; ")}`);
  await act(async () => { root.unmount(); }); clearMocks();
  return { status: "passed", checks, props_refreshes: propsRefreshes, snapshot_refreshes: snapshotRefreshes, browser_errors: errors };
}
let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => { watchdog = setTimeout(() => reject(new Error(`Player refresh fixture stalled after ${checks} checks`)), 25_000); })])
  .finally(() => clearTimeout(watchdog)).catch((error) => ({ status: "failed", checks, error: String(error), browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
