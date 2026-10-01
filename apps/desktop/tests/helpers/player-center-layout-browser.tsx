import React, { act, StrictMode, useState } from "react";
import { createRoot } from "react-dom/client";
import { I18nProvider, useI18n, type LocaleCode } from "../../src/i18n";
import { createPlayerCenterState, derivePlayerCenterViewModel, reducePlayerCenterState } from "../../src/domain/live-player-state";
import { OnlinePlayersView } from "../../src/views/servers/player-center/OnlinePlayersView";
import { usePlayerAccess } from "../../src/views/servers/player-center/use-player-access";
import { ManualPlayerActions } from "../../src/views/servers/player-center/ManualPlayerActions";
import { readPlayerAccessRosterCapabilities } from "../../src/views/servers/player-center/player-access-roster-model";
import type { InstanceDetails, InstancePlayerAccessMutationInput, ModuleDetails, ModulePlayerActionDetails, RuntimeLivePlayerEntry, RuntimeLivePlayerSnapshot } from "../../src/types";
import "../../src/app.css";
import "../../src/views/servers/workbench.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN");
document.documentElement.dataset.theme = "dark";
const errors: string[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
window.confirm = () => { throw new Error("Player controls must confirm inline"); };
const nonce = new URLSearchParams(location.search).get("nonce");
const mutations: InstancePlayerAccessMutationInput[] = [];
const now = 1_000_000;
let checks = 0;
interface ScenarioOptions { access?: boolean; manual?: boolean; multiple?: boolean; manualCount?: number; sessionActions?: boolean }
let setScenario: (next: RuntimeLivePlayerSnapshot, options?: ScenarioOptions) => void;
let setFixtureLocale: (locale: LocaleCode) => void;
let setRoster: (entries: string[]) => void;
const sessionAction: ModulePlayerActionDetails = { id: "kick", kind: "kick", label: "Remove player from the current server session", label_zh_cn: "踢出玩家", command_template: "kick {{target}}", target_required: true, destructive: true };
const manualActions: ModulePlayerActionDetails[] = [
  { id: "manual-ban", kind: "ban", label: "Ban known account", label_zh_cn: "封禁指定账号", command_template: "ban {{target}}", target_required: true, destructive: true },
  { id: "manual-role", kind: "custom", label: "Set role", label_zh_cn: "设置权限", command_template: "role {{target}} {{role}}", target_required: true, destructive: true, role_values: ["member", "moderator"] }
];
const blocklistProperty = { type: "array", title: "黑名单", items: { type: "string" }, default: [],
  "x-lsgm-player-access-kind": "block", "x-lsgm-player-access-codec": "steam64", "x-lsgm-player-access-sync": { mode: "restart" } };
function moduleDetails(multiple: boolean): ModuleDetails {
  return {
    summary: { id: "layout-fixture", name: "Player workspace fixture", version: "1", install_state: "Installed", supported_platforms: ["windows"] },
    default_ports: [], runtime: { player_actions: [sessionAction] },
    schema_json: JSON.stringify({ type: "object", properties: {
      blocklist: blocklistProperty,
      ...(multiple ? {
        allowlist: { ...blocklistProperty, title: "白名单", "x-lsgm-player-access-kind": "allow" },
        admins: { type: "array", title: "管理员", default: [], "x-lsgm-player-access-kind": "admin",
          "x-lsgm-player-access-codec": "object_identity", "x-lsgm-player-access-identity-field": "steam_id",
          items: { type: "object", required: ["steam_id", "permission"], properties: {
            steam_id: { type: "string", title: "Steam ID" }, permission: { type: "integer", title: "权限级别", default: 1000 }
          } } }
      } : {})
    } })
  };
}
const initialDetails: InstanceDetails = {
  summary: { id: "layout-server", name: "Layout server", module_id: "layout-fixture", status: "Running", active_process_count: 1, autostart: false, bind_ip: "127.0.0.1" },
  ports: [], settings_json: JSON.stringify({ blocklist: ["76561198000000001"], allowlist: [], admins: [] }),
  config_file_path: "", saves_path: "", backup_uses_declared_saves_path: false, auto_backup_on_stop: false, backup_retention_count: 3
};
function players(count: number, actionable = true): RuntimeLivePlayerEntry[] {
  return Array.from({ length: count }, (_, index) => ({ player_key: `player-${index}`, display_name: `玩家 ${index + 1}`,
    identifiers: [{ kind: "steam_id", value: String(76561198000000001n + BigInt(index)), stable: true }],
    available_action_ids: actionable ? ["kick"] : [], ping_ms: 24 + index,
    session_started_at_unix_ms: now - 120_000, role: null, attributes: [] }));
}
function snapshot(count = 2, actionable = true, expired = false): RuntimeLivePlayerSnapshot {
  return { snapshot_id: `layout-${count}-${actionable}-${expired}`, instance_id: "layout-server", status: "ready", source: "runtime_action",
    observed_at_unix_ms: now - 1000, expires_at_unix_ms: expired ? now - 1 : now + 30_000,
    complete: true, truncated: false, stale: false, current_players: count, max_players: 100, entries: players(count, actionable), issue: null };
}
function Fixture() {
  const { locale, t, setLocale } = useI18n();
  setFixtureLocale = setLocale;
  const [state, setState] = useState(() => createPlayerCenterState({ instanceId: "layout-server", snapshot: snapshot() }));
  const [details, setDetails] = useState(initialDetails);
  const [options, setOptions] = useState<ScenarioOptions>({});
  setRoster = (entries) => setDetails((previous) => ({ ...previous, settings_json: JSON.stringify({ ...JSON.parse(previous.settings_json), blocklist: entries }) }));
  setScenario = (next, nextOptions = {}) => {
    setState((current) => reducePlayerCenterState(current, { type: "snapshot-received", snapshot: next })); setOptions(nextOptions);
  };
  const module = moduleDetails(Boolean(options.multiple));
  const presentation = derivePlayerCenterViewModel(state, now).livePlayers;
  const access = usePlayerAccess({ details, moduleDetails: module,
    onApplyPlayerAccessMutation: async (mutation) => {
      mutations.push(mutation);
      setDetails((previous) => {
        const settings = JSON.parse(previous.settings_json);
        const current = (settings[mutation.fieldKey] ?? []) as unknown[];
        return { ...previous, settings_json: JSON.stringify({ ...settings,
          [mutation.fieldKey]: mutation.operation === "add" ? [...current, mutation.value]
            : current.filter((value) => JSON.stringify(value) !== JSON.stringify(mutation.value)) }) };
      });
      return { instanceId: mutation.instanceId, fieldKey: mutation.fieldKey, operation: mutation.operation,
        persistentStatus: "updated", liveStatus: "restart_required", verificationStatus: "unavailable" };
    } });
  return <main id="fixture-workspace" style={{ width: "1000px", height: "600px", margin: "24px auto", display: "grid", minHeight: 0 }}>
    <section className="server-detail-panel"><div className="detail-stack detail-stack--server">
      <div className="server-detail-subheader" style={{ minHeight: "42px" }}>
        <nav aria-label="服务器详情" style={{ display: "flex", alignItems: "center", gap: "16px" }}><span>配置</span><strong>玩家</strong><span>运行</span></nav>
      </div>
      <div className="server-detail-scroll server-detail-scroll--players" role="tabpanel" aria-label="玩家">
        <section className="player-access-workbench">
          <OnlinePlayersView declaredActions={[sessionAction]} error={null} loading={false}
            playerActionIds={options.sessionActions === false ? [] : ["kick"]}
            locale={locale} now={now} onRefresh={async () => {}} presentation={presentation} runtime={null} runtimeAvailable snapshot={state.snapshot} t={t}
            rosterCapabilities={options.access === false ? [] : readPlayerAccessRosterCapabilities(module)}
            onSelectPlayer={(playerKey) => setState((current) => reducePlayerCenterState(current, { type: "player-selected", playerKey }))}
            manualActions={options.manual === false ? undefined : (active) => <ManualPlayerActions moduleId="fixture" actions={manualActions.slice(0, options.manualCount ?? 1)} disabled={!active} instanceId="layout-server" locale={locale} t={t} />}
            access={options.access === false ? undefined : access} />
        </section>
      </div>
    </div></section>
  </main>;
}
function check(condition: unknown, message: string): asserts condition { if (!condition) throw new Error(message); }
function select<T extends Element>(selector: string): T { const element = document.querySelector<T>(selector); check(element, `Missing element: ${selector}`); return element; }
function visible(element: Element): boolean {
  for (let parent: Element | null = element.parentElement; parent; parent = parent.parentElement) {
    if (parent instanceof HTMLDetailsElement && !parent.open && !parent.querySelector(":scope > summary")?.contains(element)) return false;
  }
  return element.getClientRects().length > 0 && getComputedStyle(element).visibility !== "hidden";
}
async function settle() { await act(async () => { await new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))); }); }
async function waitFor(condition: () => boolean, label: string) {
  const deadline = performance.now() + 5000;
  while (!condition()) { check(performance.now() < deadline, `Timed out waiting for ${label}`); await act(async () => { await new Promise<void>((resolve) => setTimeout(resolve, 10)); }); }
}
async function click(selector: string) {
  const element = select<HTMLElement>(selector); check(visible(element), `Refusing to click hidden control: ${selector}`);
  element.scrollIntoView({ block: "nearest", inline: "nearest" }); await act(async () => { element.click(); });
}
async function input(selector: string, value: string) {
  const element = select<HTMLInputElement>(selector); check(visible(element), `Refusing to edit hidden input: ${selector}`);
  await act(async () => { const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set; check(setter, "Input setter is missing"); setter.call(element, value); element.dispatchEvent(new Event("input", { bubbles: true })); });
}
async function scenario(next: RuntimeLivePlayerSnapshot, options: ScenarioOptions = {}) { await act(async () => { setScenario(next, options); }); }
async function selectPlayer(index = 1) { const selector = `.player-center-player-table tbody tr:nth-child(${index}) .player-center-player-select`; if (select<HTMLElement>(selector).getAttribute("aria-pressed") !== "true") await click(selector); }
async function disclosure(selector: string, open: boolean) { if (select<HTMLDetailsElement>(selector).open !== open) await click(`${selector} > summary`); }
async function size(width: number, height: number) { const main = select<HTMLElement>("#fixture-workspace"); main.style.width = `${width}px`; main.style.height = `${height}px`; await settle(); }
function verifyScrollOwnership() {
  const outer = select<HTMLElement>(".server-detail-scroll--players");
  check(outer.scrollHeight <= outer.clientHeight + 1, `Outer tab scrolls: ${outer.scrollHeight}/${outer.clientHeight}`);
  check(outer.scrollTop === 0, "Outer tab moved away from its allocated region");
  check(getComputedStyle(outer).overflowY === "hidden", "Outer tab owns vertical scrolling");
  for (const element of outer.querySelectorAll<HTMLElement>("*")) {
    if (visible(element) && ["auto", "scroll"].includes(getComputedStyle(element).overflowY)) check(element.matches(".player-center-table-scroll, .player-access-roster-list, .player-center-controls"), `Unexpected nested vertical scroll owner: ${element.className}`);
  }
  check(outer.scrollWidth <= outer.clientWidth + 1, "Player workspace caused horizontal overflow");
  const box = outer.getBoundingClientRect(); const work = select<HTMLElement>(".player-access-workbench").getBoundingClientRect();
  check(Math.abs(work.top - box.top) <= 1 && Math.abs(work.bottom - box.bottom) <= 1, "Workspace does not fill its tab height");
  check(Math.abs(work.left - box.left) <= 1 && Math.abs(work.right - box.right) <= 1, "Workspace does not fill its tab width");
  check(document.documentElement.scrollWidth <= innerWidth, "Player workspace overflowed the viewport");
}
function verifyColumns() {
  const list = select<HTMLElement>(".player-center-list-pane").getBoundingClientRect(); const controls = select<HTMLElement>(".player-center-controls").getBoundingClientRect();
  check(document.querySelectorAll(".player-center-controls > .player-center-member-header").length === 1
    && document.querySelectorAll(".player-center-controls .player-center-member-header").length === 1,
    "Online and persistent lists do not share one fixed player-action title");
  check(list.right <= controls.left && Math.abs(list.top - controls.top) <= 1, "Player list and controls are not side by side");
  check(Math.abs(list.bottom - controls.bottom) <= 1, "Player columns do not share the tab bottom");
  check(list.width >= 300 && controls.width >= 239, "Player columns lack usable desktop width");
}
function verifyReview() {
  const review = select<HTMLElement>(".inline-confirm-review").getBoundingClientRect(); const controls = select<HTMLElement>(".player-center-controls").getBoundingClientRect();
  check(review.width >= 150 && review.left >= controls.left && review.right <= controls.right + 1, "Confirmation escaped controls");
  for (const button of document.querySelectorAll<HTMLElement>(".inline-confirm-review button")) check(button.scrollWidth <= button.clientWidth + 1, "Confirmation button was clipped");
}
const rosterAction = (field: string, operation: "add" | "remove") => `[data-roster-field="${field}"] [data-operation="${operation}"]`;
async function listTab(key: string) { await click(`.player-center-list-tabs [data-list-key="${key}"]`); }
function visibleReview() { return Array.from(document.querySelectorAll<HTMLElement>(".inline-confirm-review")).find(visible); }
function inventory() {
  return Array.from(document.querySelectorAll<HTMLElement>(".player-center-member-action, .player-access-selected-action, .player-access-edit-selected"))
    .map((button) => button.textContent?.trim()).join("|");
}
function verifyIdle() {
  const controls = select<HTMLElement>(".player-center-total-controls");
  const actions = Array.from(controls.querySelectorAll<HTMLButtonElement>(".player-center-member-action, .player-access-selected-action, .player-access-edit-selected"));
  check(actions.length > 0 && actions.every((action) => action.disabled), "Unselected target did not retain its complete disabled inventory");
  const firstRect = actions[0].getBoundingClientRect();
  const firstStyle = getComputedStyle(actions[0]);
  const styleKeys = ["backgroundColor", "borderColor", "borderRadius", "fontSize", "fontWeight", "padding", "textAlign", "opacity"] as const;
  actions.forEach((action, index) => {
    const rect = action.getBoundingClientRect();
    const style = getComputedStyle(action);
    check(Math.abs(rect.left - firstRect.left) < 1 && Math.abs(rect.width - firstRect.width) < 1,
      "Player action buttons do not share one full-width column");
    check(index === 0 || rect.top >= actions[index - 1].getBoundingClientRect().bottom + 5,
      "Player action buttons overlap or remain side by side");
    check(styleKeys.every((key) => style[key] === firstStyle[key]), "Player action buttons use different visual styles");
  });
  check(!controls.querySelector(".player-access-selected-sync, .player-access-roster-action-timing"),
    "Player action buttons retain redundant timing descriptions");
  check(!controls.querySelector(".player-center-member-prompt, .player-center-member-heading, .inline-confirm-review, .player-access-selected-editor"),
    "Cleared selection retained a target, prompt, confirmation, or temporary form");
}
function verifyRosterLocation() {
  check(document.querySelector(".player-center-list-pane .player-access-roster-entries"), "Persistent roster is missing from the left pane");
  check(!document.querySelector(".player-center-controls .player-access-roster-entries"), "Right-side controls contain a second roster");
  check(!document.querySelector(".player-center-online-controls, .player-center-roster-controls, .player-access-roster-management"),
    "Old per-list action panes remain");
}
async function run() {
  const root = createRoot(select<HTMLDivElement>("#fixture"));
  await act(async () => { await Promise.all([import("../../src/i18n-messages"), import("../../src/i18n-messages-zh-cn")]); root.render(<StrictMode><I18nProvider><Fixture /></I18nProvider></StrictMode>); });
  await waitFor(() => Boolean(document.querySelector('.player-center-list-tabs [data-list-key="online"]')), "player workspace mount");
  verifyColumns(); verifyScrollOwnership(); verifyIdle();
  check(document.querySelectorAll('.player-center-list-pane [role="tablist"]').length === 1 && !document.querySelector('.player-center-controls [role="tablist"]'), "List navigation is duplicated"); checks++;
  const initialInventory = inventory();
  await selectPlayer();
  check(select<HTMLElement>(".player-center-member-heading").textContent?.includes("玩家 1"), "Selected online target is missing");
  check(!select<HTMLButtonElement>(".player-center-member-action").disabled && !select<HTMLButtonElement>(rosterAction("blocklist", "remove")).disabled
    && select<HTMLButtonElement>(rosterAction("blocklist", "add")).disabled, "Fresh target did not update its fixed action availability"); checks++;
  await click(rosterAction("blocklist", "remove")); verifyReview();
  await click(".inline-confirm-buttons button:first-child");
  check(mutations.length === 0 && !visibleReview(), "Cancelling removal changed data"); checks++;
  await click(rosterAction("blocklist", "remove")); await click(".inline-confirm-submit");
  check(mutations.length === 1 && mutations[0].value === "76561198000000001" && mutations[0].operation === "remove"
    && inventory() === initialInventory && !select<HTMLButtonElement>(rosterAction("blocklist", "add")).disabled, "Removal changed action inventory or used a different target"); checks++;
  await listTab("blocklist"); verifyRosterLocation(); verifyIdle();
  check(inventory() === initialInventory && document.querySelector(".player-center-manual-actions"), "Switching lists replaced the total action inventory"); checks++;
  await click(".player-access-roster-create"); await input("#player-access-roster-blocklist", "76561198000000002");
  await click(".player-access-roster-add-button"); verifyReview(); await click(".inline-confirm-buttons button:first-child");
  check(mutations.length === 1, "Manual roster cancellation changed data"); checks++;
  await click(".player-access-roster-add-button"); await click(".inline-confirm-submit");
  check(mutations.length === 2 && mutations[1].value === "76561198000000002"
    && document.querySelectorAll(".player-center-list-pane .player-access-roster-entry").length === 1 && !document.querySelector(".player-access-selected-editor"),
    "Explicit add failed to update the left roster or close its form"); checks++;
  await click(".player-access-roster-create"); await input("#player-access-roster-blocklist", "76561198000000077");
  await click(".player-access-roster-add-button"); await listTab("online"); verifyIdle();
  check(inventory() === initialInventory && mutations.length === 2, "Changing tabs retained a pending editor mutation"); checks++;
  await disclosure(".player-center-manual-actions", true); await input("#manual-player-target-layout-server", "offline-account");
  await scenario(snapshot(3));
  check(select<HTMLInputElement>("#manual-player-target-layout-server").value === "offline-account", "Snapshot refresh lost the independent manual draft");
  await disclosure(".player-center-manual-actions", false); checks++;
  await selectPlayer(3); await click(rosterAction("blocklist", "add")); await selectPlayer(1);
  check(!visibleReview(), "Changing selected players retained the prior confirmation"); checks++;
  await selectPlayer(3); await click(rosterAction("blocklist", "add")); await click(".inline-confirm-submit");
  check(mutations.length === 3 && mutations[2].value === "76561198000000003", "Quick action consumed a different draft identity"); checks++;
  await scenario(snapshot(60));
  const table = select<HTMLElement>(".player-center-table-scroll"); const controls = select<HTMLElement>(".player-center-controls");
  const controlTop = controls.getBoundingClientRect().top; const controlScroll = controls.scrollTop;
  table.scrollTop = table.scrollHeight;
  const lastRow = select<HTMLElement>(".player-center-player-table tbody tr:last-child").getBoundingClientRect();
  check(lastRow.bottom <= table.getBoundingClientRect().bottom + 1 && lastRow.bottom > table.getBoundingClientRect().top, "Player 60 is unreachable");
  await selectPlayer(60);
  check(controls.getBoundingClientRect().top === controlTop && controls.scrollTop === controlScroll, "List selection scrolled the total controls");
  verifyColumns(); verifyScrollOwnership(); checks++;
  await act(async () => { setRoster(Array.from({ length: 40 }, (_, index) => String(76561198000000100n + BigInt(index)))); });
  await listTab("blocklist"); verifyRosterLocation();
  const rosterScroll = select<HTMLElement>(".player-center-list-pane .player-access-roster-list");
  const controlPosition = controls.scrollTop; rosterScroll.scrollTop = rosterScroll.scrollHeight;
  const lastEntry = select<HTMLElement>(".player-access-roster-entry:last-child").getBoundingClientRect();
  check(lastEntry.bottom <= rosterScroll.getBoundingClientRect().bottom + 1 && lastEntry.bottom > rosterScroll.getBoundingClientRect().top
    && controls.scrollTop === controlPosition, "Left roster scrolling cannot reach its final entry independently"); checks++;
  for (const width of [1000, 850, 720]) { await size(width, 500); verifyColumns(); verifyScrollOwnership(); checks++; }
  await listTab("online"); await selectPlayer(); await size(720, 260); await click(".player-center-member-action"); verifyReview();
  const cancel = select<HTMLElement>(".inline-confirm-buttons button:first-child"); cancel.scrollIntoView({ block: "nearest" });
  check(cancel.getBoundingClientRect().bottom <= controls.getBoundingClientRect().bottom + 1, "Short viewport cannot reach confirmation");
  await click(".inline-confirm-buttons button:first-child"); checks++;
  await size(850, 500); await scenario(snapshot(2, false), { manual: false }); await selectPlayer();
  check(select<HTMLButtonElement>(".player-center-member-action").disabled && !select<HTMLButtonElement>(rosterAction("blocklist", "add")).disabled,
    "Snapshot permissions were confused with persistent roster identity"); checks++;
  const noIdentity = snapshot(2, false); noIdentity.entries = noIdentity.entries.map((entry) => ({ ...entry, identifiers: [] }));
  await scenario(noIdentity, { manual: false });
  check(!document.querySelector(".player-center-player-select")
    && Array.from(document.querySelectorAll<HTMLButtonElement>(".player-access-selected-action")).every((button) => button.disabled),
    "Missing account identity can trigger a roster operation"); checks++;
  await scenario(snapshot(2, false), { access: false, manual: false, sessionActions: false });
  check(!document.querySelector(".player-center-controls") && Math.abs(select<HTMLElement>(".player-center-list-pane").getBoundingClientRect().width
    - select<HTMLElement>(".player-center-member-layout").getBoundingClientRect().width) <= 1, "Read-only server reserves an empty controls column"); checks++;
  await scenario(snapshot(0)); verifyIdle();
  check(select<HTMLElement>('.player-center-list-tabs [data-list-key="online"]').textContent?.includes("0"), "Known zero count disappeared");
  await scenario({ ...snapshot(0), current_players: null, max_players: null, status: "unsupported", complete: false });
  check(!select<HTMLElement>(".player-center-list-pane").textContent?.includes("人数未知")
    && !select<HTMLElement>('.player-center-list-tabs [data-list-key="online"]').textContent?.match(/\d/), "Unknown count renders a placeholder or invented zero"); checks++;
  await scenario(snapshot(), { multiple: true }); await listTab("online");
  const completeInventory = inventory(); check(document.querySelectorAll(".player-access-selected-action").length === 6, "Module roster pairs are missing");
  const tabs = Array.from(document.querySelectorAll<HTMLButtonElement>('.player-center-list-tabs [role="tab"]'));
  tabs[0].focus(); await act(async () => { tabs[0].dispatchEvent(new KeyboardEvent("keydown", { key: "End", bubbles: true })); });
  check(document.activeElement === tabs.at(-1) && inventory() === completeInventory, "Keyboard list navigation changed the action inventory");
  await act(async () => { document.activeElement?.dispatchEvent(new KeyboardEvent("keydown", { key: "Home", bubbles: true })); });
  check(document.activeElement === tabs[0] && tabs.filter((tab) => tab.tabIndex === 0).length === 1, "List tabs lost roving focus"); checks++;
  await selectPlayer(); const beforeObject = mutations.length; await click(rosterAction("admins", "add"));
  check(select<HTMLInputElement>("#player-access-quick-admins-steam_id").value === "76561198000000001"
    && select<HTMLInputElement>("#player-access-quick-admins-steam_id").disabled, "Explicit roster add did not lock trusted account identity");
  await input("#player-access-quick-admins-permission", "250"); await click(".player-access-selected-editor .player-access-roster-action-pair > button");
  check(!document.querySelector(".player-access-selected-editor") && mutations.length === beforeObject, "Cancelling account parameters changed data"); checks++;
  await act(async () => { setFixtureLocale("en-US"); });
  await waitFor(() => Boolean(document.querySelector('.player-center-list-tabs [data-list-key="online"]')?.textContent?.includes("Online members")), "English catalog");
  const english = snapshot(); const longName = "Player with a long descriptive name that must remain readable during confirmation";
  english.entries[0].display_name = longName; await scenario(english); await selectPlayer(); await click(".player-center-member-action");
  const message = select<HTMLElement>(".inline-confirm-message");
  check(message.textContent?.includes(longName) && message.textContent.includes(sessionAction.label) && message.scrollWidth <= message.clientWidth + 1,
    "Long English confirmation lost its target or clipped text"); verifyReview(); await click(".inline-confirm-buttons button:first-child"); checks++;
  await act(async () => { setFixtureLocale("zh-CN"); setRoster(["76561198000000009"]); });
  await scenario(snapshot(), { multiple: true }); await listTab("blocklist"); await click(".player-access-roster-select");
  check(select<HTMLElement>(".player-center-member-heading").textContent?.includes("76561198000000009")
    && select<HTMLButtonElement>(".player-center-member-action").disabled && !select<HTMLButtonElement>(rosterAction("admins", "add")).disabled
    && !select<HTMLButtonElement>(rosterAction("blocklist", "remove")).disabled, "Offline roster target cannot use cross-roster actions or incorrectly owns a session"); checks++;
  const beforeRemoval = mutations.length; await click(rosterAction("blocklist", "remove")); verifyReview(); await click(".inline-confirm-submit");
  check(mutations.length === beforeRemoval + 1 && mutations[beforeRemoval].value === "76561198000000009"
    && !document.querySelector(".player-access-roster-entry"), "Persistent selection removal sent the wrong identity"); verifyIdle(); checks++;
  await act(async () => { setRoster(["76561198000000009"]); }); await click(".player-access-roster-select"); await click(rosterAction("blocklist", "remove"));
  await click(".player-access-roster-select"); verifyIdle();
  check(mutations.length === beforeRemoval + 1, "Deselecting a roster entry submitted the pending removal"); checks++;
  await act(async () => { setRoster(["76561198000000001"]); }); await click(".player-access-roster-select");
  check(!select<HTMLButtonElement>(".player-center-member-action").disabled, "A uniquely matched fresh roster account cannot use its real online action");
  const ambiguous = snapshot(); ambiguous.entries.push({ ...ambiguous.entries[0], player_key: "duplicate-account" });
  await scenario(ambiguous, { multiple: true });
  check(select<HTMLButtonElement>(".player-center-member-action").disabled, "Duplicate live accounts authorized a roster runtime action");
  await scenario({ ...snapshot(), stale: true }, { multiple: true });
  check(select<HTMLButtonElement>(".player-center-member-action").disabled && !select<HTMLButtonElement>(rosterAction("blocklist", "remove")).disabled,
    "Stale online authority was confused with an independently known persistent account"); checks++;
  await listTab("online"); await size(720, 550); verifyIdle(); verifyColumns(); verifyScrollOwnership(); checks++;
  check(errors.length === 0, `Browser errors: ${errors.join("; ")}`);
  return { status: "passed", checks, browser_errors: errors };
}let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => { watchdog = setTimeout(() => reject(new Error(`Player workspace fixture stalled after ${checks} checks`)), 25_000); })])
  .finally(() => clearTimeout(watchdog)).catch((error) => ({ status: "failed", checks, error: String(error), browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
