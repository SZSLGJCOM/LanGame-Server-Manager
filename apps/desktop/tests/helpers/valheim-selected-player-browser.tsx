import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import schema from "../../../../modules/valheim/schema.json";
import { applyMockPlayerAccessMutation } from "../../src/api-mock/player-access";
import { buildMockModuleDetails } from "../../src/api-mock/module-details";
import { createPlayerCenterState, derivePlayerCenterViewModel, reducePlayerCenterState } from "../../src/domain/live-player-state";
import { I18nProvider, useI18n } from "../../src/i18n";
import { ActivityNoticeTarget } from "../../src/components/ActivityNotice";
import { OnlinePlayersView } from "../../src/views/servers/player-center/OnlinePlayersView";
import { readPlayerAccessRosterCapabilities } from "../../src/views/servers/player-center/player-access-roster-model";
import { usePlayerAccess } from "../../src/views/servers/player-center/use-player-access";
import type { InstanceDetails, InstancePlayerAccessMutationInput, RuntimeLivePlayerEntry, RuntimeLivePlayerSnapshot } from "../../src/types";
import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import "../../src/app.css";
import "../../src/views/servers/workbench.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
document.documentElement.dataset.theme = "dark";
const nonce = new URLSearchParams(location.search).get("nonce");
const fixture = document.getElementById("fixture");
if (!fixture) throw new Error("Fixture root is missing");
const root = createRoot(fixture);
const checks: string[] = [];
const errors: string[] = [];
const mutations: InstancePlayerAccessMutationInput[] = [];
const now = 1_000_000;
const platformBan = "xBoX_Banned_7";
const nativeAccounts = [{ name: "Rowan", id: "Steam_76561198000000001" }, { name: "Birch", id: "Xbox_NativeAccount_A7" }];
const initial = { admin_list: "Steam_Admin", permitted_list: "Steam_Permitted", banned_list: platformBan, future_setting: "retained",
  world_preset: "", world_modifiers: "", world_set_keys: "", world_name: "Existing World" };
let saved = JSON.stringify(initial);
let nativeActionCalls = 0;
let scenario: "players" | "setup-running" | "setup-stopped" | "setup-connected-empty" = "players";
const preparationCalls: string[] = [];
let preparationFails = false;
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
window.alert = window.confirm = window.prompt = () => { throw new Error("Native dialogs are not allowed"); };
const moduleDetails = { ...buildMockModuleDetails("valheim"), schema_json: JSON.stringify(schema) };
const capabilities = readPlayerAccessRosterCapabilities(moduleDetails);
const instance: InstanceDetails = {
  summary: { id: "valheim-character-ban", module_id: "valheim", name: "Valheim character bans", status: "Running",
    active_process_count: 1, autostart: false, bind_ip: "127.0.0.1" }, active_run: { run_id: 1 },
  ports: [], settings_json: saved, config_file_path: "", saves_path: "", backup_uses_declared_saves_path: false,
  auto_backup_on_stop: false, backup_retention_count: 3
};
const players: RuntimeLivePlayerEntry[] = [
  { player_key: "native-raven", display_name: "Raven", identifiers: [{ kind: "player_name" as const, value: "Raven", stable: false }] },
  { player_key: "label-only", display_name: "Displayed Alias", identifiers: [{ kind: "session_id" as const, value: "not-a-character", stable: false }] },
  ...nativeAccounts.map((account) => ({ player_key: `native-${account.name}`, display_name: account.name, identifiers: [
    { kind: "valheim_platform_id" as const, value: account.id, stable: true },
    { kind: "player_name" as const, value: account.name, stable: false }
  ] })),
  { player_key: "conflicting-native", display_name: "Conflicted", identifiers: [
    { kind: "valheim_platform_id" as const, value: "Xbox_Conflicted_A", stable: true },
    { kind: "valheim_platform_id" as const, value: "Xbox_Conflicted_B", stable: true },
    { kind: "player_name" as const, value: "Conflicted", stable: false }
  ] }
].map((player) => ({ ...player, available_action_ids: [], attributes: [], ping_ms: null, role: null, session_started_at_unix_ms: null }));
const snapshot: RuntimeLivePlayerSnapshot = {
  snapshot_id: "valheim-name-snapshot", instance_id: instance.summary.id, status: "ready", source: "structured_log",
  observed_at_unix_ms: now, expires_at_unix_ms: now + 60_000, complete: true, truncated: false, stale: false,
  current_players: players.length, max_players: 10, entries: players, issue: null
};
function Fixture() {
  const { locale, t } = useI18n();
  const stopped = scenario === "setup-stopped";
  const initialSnapshot: RuntimeLivePlayerSnapshot = scenario === "players" ? snapshot : { ...snapshot,
    source: scenario === "setup-connected-empty" ? "file_ipc" : "structured_log",
    status: stopped ? "stopped" : "ready", complete: !stopped,
    entries: scenario === "setup-running" ? players.slice(0, 1) : [], current_players: scenario === "setup-running" ? 1 : 0 };
  const [state, setState] = useState(() => createPlayerCenterState({ instanceId: instance.summary.id, snapshot: initialSnapshot }));
  const [details, setDetails] = useState<InstanceDetails>({ ...instance, settings_json: saved,
    summary: { ...instance.summary, status: stopped ? "Stopped" : "Running", active_process_count: stopped ? 0 : 1 },
    active_run: stopped ? null : instance.active_run });
  const [activityTarget, setActivityTarget] = useState<HTMLDivElement | null>(null);
  const access = usePlayerAccess({ details, moduleDetails, onApplyPlayerAccessMutation: async (input) => {
    if (input.instanceId !== instance.summary.id || !["admin_list", "permitted_list", "banned_list"].includes(input.fieldKey)) throw new Error("Unexpected roster mutation target");
    const settings = JSON.parse(saved);
    const property = schema.properties[input.fieldKey as "admin_list" | "permitted_list" | "banned_list"];
    const result = applyMockPlayerAccessMutation(property, settings[input.fieldKey], input, true);
    mutations.push(structuredClone(input));
    saved = JSON.stringify({ ...settings, [input.fieldKey]: result.value });
    setDetails((current) => ({ ...current, settings_json: saved }));
    // This response models a verified native reload. It does not claim this
    // browser fixture has connected to a real game process.
    const verified = input.fieldKey !== "banned_list" && nativeAccounts.some((account) => account.id === input.value);
    return { instanceId: input.instanceId, fieldKey: input.fieldKey, operation: input.operation,
      persistentStatus: result.changed ? "updated" : "unchanged", liveStatus: verified ? "applied" : "restart_required",
      verificationStatus: verified ? "verified" : result.verificationStatus };
  } });
  return <ActivityNoticeTarget.Provider value={{ element: activityTarget, dismissLabel: t("common.close") }}><main style={{ height: "calc(100vh - 96px)", margin: 24, display: "grid", minHeight: 0 }}>
    <section className="server-detail-panel"><div className="detail-stack detail-stack--server">
      <div className="server-detail-subheader">Valheim · {locale === "zh-CN" ? "玩家" : "Players"}</div>
      <div className="server-detail-scroll server-detail-scroll--players"><section className="player-access-workbench">
        <OnlinePlayersView moduleId="valheim" access={access} declaredActions={moduleDetails.runtime.player_actions ?? []}
          instanceDetails={details} onPrepareValheimControl={async (instanceId) => {
            if (!stopped || instanceId !== instance.summary.id) throw new Error("Invalid preparation scope");
            preparationCalls.push(instanceId);
            if (preparationFails) throw new Error("Synthetic preparation failure");
            return structuredClone(details);
          }}
          playerActionIds={moduleDetails.runtime.player_list?.player_action_ids ?? []} locale={locale} t={t}
          loading={false} error={null} runtime={null} runtimeAvailable={!stopped} snapshot={state.snapshot} now={now}
          rosterCapabilities={capabilities} presentation={derivePlayerCenterViewModel(state, now).livePlayers}
          onRefresh={async () => { setState((current) => current.snapshot ? reducePlayerCenterState(current, { type: "snapshot-received",
            snapshot: { ...current.snapshot, snapshot_id: `${current.snapshot.snapshot_id}-refreshed`, entries: current.snapshot.entries.map((player) => ({ ...player })) } }) : current); }}
          onSelectPlayer={(playerKey) => setState((current) => reducePlayerCenterState(current, { type: "player-selected", playerKey }))}
          onExecutePlayerAction={async () => { nativeActionCalls++; throw new Error("Valheim has no verified live management action"); }} />
      </section></div>
    </div></section>
  </main><div data-valheim-test-activity ref={setActivityTarget} style={{ position: "fixed", left: 24, right: 24, bottom: 12 }} /></ActivityNoticeTarget.Provider>;
}
function check(value: unknown, description: string): asserts value {
  if (!value) throw new Error(`${description}: ${fixture?.textContent}`);
  checks.push(description);
}
function element<T extends Element>(selector: string): T {
  const found = fixture?.querySelector<T>(selector);
  if (!found) throw new Error(`Missing ${selector}`);
  return found;
}
const action = (field: string, operation: string) => `[data-roster-field="${field}"] [data-operation="${operation}"]`;
async function click(selector: string) {
  const button = element<HTMLButtonElement>(selector);
  check(!button.disabled, `Enabled action ${selector}`);
  button.scrollIntoView({ block: "nearest" });
  await act(async () => { button.click(); });
}
function platformDisabled(locale: string) {
  for (const field of ["admin_list", "permitted_list"]) {
    for (const operation of ["add", "remove"]) {
      const button = element<HTMLButtonElement>(action(field, operation));
      check(button.disabled, `${locale}: character selection cannot mutate ${field} ${operation}`);
      const reasons = button.getAttribute("aria-describedby")?.split(" ").map((id) => document.getElementById(id)?.textContent).join(" ") ?? "";
      check(reasons.includes("F2") && reasons.includes("Platform User ID") && reasons.includes(locale === "zh-CN" ? "手动添加" : "manually"),
        `${locale}: ${field} explains the actual missing platform identity and manual entry route`);
      check(reasons.includes(locale === "zh-CN" ? "自动取得" : "Automatic collection"),
        `${locale}: ${field} describes unavailable automatic collection instead of a name-only protocol limitation`);
    }
  }
}
async function actionBubble(locale: string) {
  const button = element<HTMLButtonElement>(action("admin_list", "add"));
  const anchor = button.closest<HTMLElement>(".player-access-total-action");
  check(anchor?.tabIndex === 0 && button.disabled, `${locale}: disabled action keeps a keyboard-focusable help anchor`);
  const description = document.getElementById(button.getAttribute("aria-describedby") ?? "");
  check(description?.classList.contains("configuration-field-help-a11y"), `${locale}: disabled reason remains available as a hidden accessible description`);
  const waitForBubble = async (text: string) => {
    const deadline = performance.now() + 4000;
    for (;;) {
      const bubble = document.querySelector<HTMLElement>(".configuration-field-help-tooltip.is-visible");
      if (bubble?.textContent?.includes(text) && getComputedStyle(bubble).opacity === "1") return bubble;
      if (performance.now() >= deadline) throw new Error(`Action help did not become visible: ${text}`);
      await act(async () => { await new Promise<void>((resolve) => requestAnimationFrame(() => resolve())); });
    }
  };
  const bounds = anchor.getBoundingClientRect();
  const point = { clientX: bounds.left + bounds.width / 2, clientY: bounds.top + bounds.height / 2 };
  const hit = document.elementFromPoint(point.clientX, point.clientY);
  check(hit !== button && Boolean(hit && anchor.contains(hit)), `${locale}: a disabled button delegates pointer hit testing to its help anchor`);
  await act(async () => { anchor.dispatchEvent(new PointerEvent("pointerover", { bubbles: true, pointerType: "mouse", ...point })); });
  const hovered = await waitForBubble("Platform User ID");
  check(hovered.textContent?.includes("F2") && hovered.textContent?.includes(locale === "zh-CN" ? "手动添加" : "manually"),
    `${locale}: hovering a disabled action shows its accurate platform identity reason`);
  await act(async () => {
    anchor.dispatchEvent(new PointerEvent("pointerout", { bubbles: true, pointerType: "mouse", relatedTarget: document.body, ...point }));
    anchor.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    anchor.focus();
  });
  const focused = await waitForBubble("Platform User ID");
  check(document.activeElement === anchor && focused.textContent?.includes("F2"),
    `${locale}: keyboard focus shows the disabled action reason without enabling the button`);
  await act(async () => {
    anchor.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    anchor.blur();
  });
  check(!document.querySelector(".configuration-field-help-tooltip.is-visible"), `${locale}: Escape dismisses action help`);
  check(!fixture?.querySelector(".player-center-action-reason"), `${locale}: the action pane contains no visible stacked reason paragraphs`);
}
async function draw(locale: string, suffix: string) {
  await act(async () => {
    localStorage.setItem("langame.locale", locale);
    root.render(<I18nProvider key={`${locale}-${suffix}`}><Fixture /></I18nProvider>);
  });
  const selector = scenario === "players"
    ? `.player-center-player-select[aria-label="${locale === "zh-CN" ? "选择玩家 Raven" : "Select player Raven"}"]` : ".player-center-controls";
  const deadline = performance.now() + 5000;
  while (!fixture?.querySelector(selector)) {
    if (performance.now() >= deadline) throw new Error(`Localized player workspace did not load: ${fixture?.textContent}`);
    await act(async () => { await new Promise<void>((resolve) => requestAnimationFrame(() => resolve())); });
  }
}
async function accountControlSetup(locale: string) {
  scenario = "setup-running";
  await draw(locale, "setup-running");
  const disabled = element<HTMLButtonElement>("[data-valheim-control-setup] button");
  const anchor = disabled.closest<HTMLElement>("[data-valheim-control-setup]");
  check(disabled.disabled && anchor?.tabIndex === 0, `${locale}: enabling account management requires an actually stopped instance`);
  const reason = document.getElementById(disabled.getAttribute("aria-describedby") ?? "")?.textContent;
  check(reason?.includes(locale === "zh-CN" ? "先正常停止" : "Stop the server normally"), `${locale}: running setup explains the stop requirement only through accessible help`);
  await act(async () => { anchor.focus(); });
  const deadline = performance.now() + 4000;
  while (!document.querySelector(".configuration-field-help-tooltip.is-visible")) {
    if (performance.now() >= deadline) throw new Error("Account setup help did not become visible");
    await act(async () => { await new Promise<void>((resolve) => requestAnimationFrame(() => resolve())); });
  }
  check(document.querySelector(".configuration-field-help-tooltip.is-visible")?.textContent === reason,
    `${locale}: disabled setup exposes its stop requirement in a keyboard-focus help bubble`);
  await act(async () => { anchor.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })); anchor.blur(); });
  scenario = "setup-stopped";
  await draw(locale, "setup-stopped");
  const before = preparationCalls.length;
  await click("[data-valheim-control-setup] button");
  const confirmation = element<HTMLElement>(".inline-confirm-message").textContent ?? "";
  check(confirmation.includes("BepInEx") && confirmation.includes(locale === "zh-CN" ? "独立程序目录" : "independent program directory")
    && confirmation.includes(locale === "zh-CN" ? "保留已有存档和世界规则" : "preserves existing saves and world rules")
    && confirmation.includes(locale === "zh-CN" ? "启动后将自动采集平台身份" : "Platform identities will be collected automatically after startup"),
    `${locale}: explicit setup reviews the runtime change data preservation and automatic identity collection`);
  await click(".inline-confirm-buttons > button:first-child");
  check(preparationCalls.length === before, `${locale}: cancelling setup does not install the extension`);
  preparationFails = true;
  await click("[data-valheim-control-setup] button");
  await click(".inline-confirm-submit");
  check(element<HTMLElement>("[data-valheim-test-activity] .shell-activity-notice.is-error").textContent?.includes("Synthetic preparation failure")
    && !fixture?.querySelector("[data-valheim-test-activity] .shell-activity-notice.is-success")
    && !element<HTMLButtonElement>(".inline-confirm-submit").disabled,
    `${locale}: failed preparation remains retryable and reports no false readiness`);
  preparationFails = false;
  await click(".inline-confirm-submit");
  check(preparationCalls.length === before + 2 && preparationCalls.at(-1) === instance.summary.id,
    `${locale}: confirmed setup prepares only the selected stopped instance`);
  check(element<HTMLElement>("[data-valheim-test-activity] .shell-activity-notice.is-success").textContent?.includes(locale === "zh-CN" ? "启动服务器后" : "Start the server"),
    `${locale}: setup reports preparation in the dynamic bar without claiming a connected native identity`);
  check(!fixture?.querySelector(".player-center-controls .shell-activity-notice, .player-center-action-reason"),
    `${locale}: setup adds one action without stacking explanatory paragraphs in the right pane`);
  check(saved === JSON.stringify(initial), `${locale}: preparing account management preserves world rules and all rosters`);
  scenario = "setup-connected-empty";
  await draw(locale, "setup-connected-empty");
  check(!fixture?.querySelector("[data-valheim-control-setup]"), `${locale}: an authoritative empty native roster does not offer redundant setup`);
  scenario = "players";
}
const selectPlayer = (locale: string, name: string) => `.player-center-player-select[aria-label="${locale === "zh-CN" ? "选择玩家 " : "Select player "}${name}"]`;
async function nativeAccountActions(locale: string) {
  for (const account of nativeAccounts) {
    await click(selectPlayer(locale, account.name));
    const ban = element<HTMLButtonElement>(action("banned_list", "add"));
    check(!ban.disabled && !ban.textContent?.includes(locale === "zh-CN" ? "角色名" : "character name"),
      `${locale}: verified ${account.name} uses an account ban instead of a character-name fallback`);
    for (const field of ["admin_list", "permitted_list"]) {
      check(!element<HTMLButtonElement>(action(field, "add")).disabled && element<HTMLButtonElement>(action(field, "remove")).disabled,
        `${locale}: verified ${account.name} enables adding the exact native identity to ${field}`);
      for (const operation of ["add", "remove"]) {
        const before = mutations.length;
        const allowlistPolicy = locale === "zh-CN" ? "名单非空时，仅名单内账号可进入；移除最后一项会取消白名单限制，封禁规则继续有效。"
          : "When the allowlist is non-empty, only listed accounts can enter. Removing its last entry disables the allowlist restriction; bans still apply.";
        if (field === "permitted_list") {
          const button = element<HTMLButtonElement>(action(field, operation));
          const help = document.getElementById(button.getAttribute("aria-describedby") ?? "")?.textContent;
          check(help?.includes(allowlistPolicy), `${locale}: allowlist ${operation} help explains entry restrictions last-entry removal and retained bans`);
        }
        await click(action(field, operation));
        check(element<HTMLElement>(".inline-confirm-message").textContent?.includes(account.id),
          `${locale}: ${field} ${operation} reviews the exact native ${account.name} account`);
        if (field === "permitted_list") check(element<HTMLElement>(".inline-confirm-message").textContent?.includes(allowlistPolicy),
          `${locale}: allowlist ${operation} confirmation reviews the native restriction scope`);
        await click(".inline-confirm-submit");
        const mutation = mutations.at(-1);
        check(mutations.length === before + 1 && mutation?.instanceId === instance.summary.id && mutation.fieldKey === field
          && mutation.operation === operation && mutation.value === account.id,
          `${locale}: ${field} ${operation} dispatches the exact native ${account.name} account`);
        const entries = String(JSON.parse(saved)[field]).split("\n");
        check(entries.includes(account.id) === (operation === "add"), `${locale}: ${field} ${operation} updates the selected account membership`);
        const notice = element<HTMLElement>("[data-valheim-test-activity] .shell-activity-notice.is-success");
        check(notice.textContent?.includes(locale === "zh-CN" ? "已即时生效" : "applied live"),
          `${locale}: verified account changes report actual application in the dynamic activity bar`);
        check(!fixture?.querySelector(".player-center-controls .shell-activity-notice, .player-center-action-reason"),
          `${locale}: account actions keep feedback and disabled reasons out of the right action area`);
      }
      await click(".player-center-refresh-button");
      check(element<HTMLButtonElement>(selectPlayer(locale, account.name)).getAttribute("aria-pressed") === "true",
        `${locale}: native ${account.name} remains selected across a refreshed snapshot`);
    }
    await click(action("banned_list", "add"));
    check(element<HTMLElement>(".inline-confirm-message").textContent?.includes(account.id),
      `${locale}: account bans review the authoritative platform ID`);
    await click(".inline-confirm-submit");
    check(mutations.at(-1)?.fieldKey === "banned_list" && mutations.at(-1)?.value === account.id
      && String(JSON.parse(saved).banned_list).split("\n").includes(account.id),
      `${locale}: account bans preserve exact native platform case`);
    await click(action("banned_list", "remove"));
    await click(".inline-confirm-submit");
    check(mutations.at(-1)?.operation === "remove" && mutations.at(-1)?.value === account.id
      && JSON.parse(saved).banned_list === platformBan, `${locale}: removing an account ban preserves pre-existing bans`);
  }
  for (const key of ["admin_list", "permitted_list", "banned_list", "future_setting", "world_preset", "world_modifiers", "world_set_keys", "world_name"]) {
    check(JSON.parse(saved)[key] === initial[key as keyof typeof initial], `${locale}: account mutations preserve or restore ${key}`);
  }
  await click(selectPlayer(locale, "Conflicted"));
  for (const field of ["admin_list", "permitted_list", "banned_list"]) {
    check(element<HTMLButtonElement>(action(field, "add")).disabled, `${locale}: conflicting native identities cannot mutate ${field} or fall back to a name`);
  }
  check(nativeActionCalls === 0, `${locale}: persistent roster operations cannot fabricate session command authority`);
}
async function run() {
  await act(prepareBrowserLocaleCatalogs);
  for (const locale of ["zh-CN", "en-US"]) {
    scenario = "players";
    saved = JSON.stringify(initial);
    const before = mutations.length;
    await draw(locale, "initial");
    await click(`.player-center-player-select[aria-label="${locale === "zh-CN" ? "选择玩家 Raven" : "Select player Raven"}"]`);
    platformDisabled(locale);
    await actionBubble(locale);
    check(!Array.from(fixture?.querySelectorAll<HTMLButtonElement>(".player-center-member-action") ?? []).some((button) => !button.disabled),
      `${locale}: name-only players acquire no fabricated Valheim live action authority`);
    const ban = element<HTMLButtonElement>(action("banned_list", "add"));
    check(!ban.disabled && ban.textContent === (locale === "zh-CN" ? "封禁角色名" : "Ban character name"), `${locale}: only the explicit native character name enables the ban action`);
    check(element<HTMLButtonElement>(action("banned_list", "remove")).disabled, `${locale}: an unlisted character cannot be unbanned`);
    await click(action("banned_list", "add"));
    const confirmation = element<HTMLElement>(".inline-confirm-message").textContent ?? "";
    check(confirmation.includes("Raven") && confirmation.includes(locale === "zh-CN" ? "角色名可重名或更改" : "Character names can be shared or changed") &&
      confirmation.includes(locale === "zh-CN" ? "可能自动重读" : "may reload the roster automatically") &&
      confirmation.includes(locale === "zh-CN" ? "未核验在线踢出结果" : "does not verify whether an online player was kicked"),
      `${locale}: inline confirmation explains name collisions changes and unverified native enforcement`);
    check(mutations.length === before, `${locale}: opening confirmation does not persist a ban`);
    await click(".inline-confirm-buttons > button:first-child");
    check(mutations.length === before && JSON.parse(saved).banned_list === platformBan, `${locale}: cancelling leaves the ban roster intact`);
    await click(action("banned_list", "add"));
    await click(".inline-confirm-submit");
    check(mutations.length === before + 1 && mutations.at(-1)?.value === "Raven" && JSON.parse(saved).banned_list === `${platformBan}\nRaven`,
      `${locale}: confirmed character ban preserves exact native spelling and existing platform entries`);
    check(JSON.parse(saved).admin_list === initial.admin_list && JSON.parse(saved).permitted_list === initial.permitted_list &&
      JSON.parse(saved).future_setting === "retained", `${locale}: character bans preserve platform rosters and unrelated settings`);
    check(fixture?.textContent?.includes(locale === "zh-CN" ? "下次启动" : "next server start")
      && fixture?.textContent?.includes(locale === "zh-CN" ? "即时状态未核验" : "Live state was not verified"),
      `${locale}: persistence feedback distinguishes the saved roster from unverified live state`);
    await draw(locale, "reopen");
    await click('[data-list-key="banned_list"]');
    await click(`.player-access-roster-select[aria-label="${locale === "zh-CN" ? "选择条目 Raven" : "Select entry Raven"}"]`);
    platformDisabled(locale);
    check(!element<HTMLButtonElement>(action("banned_list", "remove")).disabled &&
      element<HTMLButtonElement>(action("banned_list", "remove")).textContent === (locale === "zh-CN" ? "解除角色名封禁" : "Unban character name"),
      `${locale}: a stored character-name ban can be removed without platform escalation`);
    await click(action("banned_list", "remove"));
    await click(".inline-confirm-submit");
    check(mutations.length === before + 2 && mutations.at(-1)?.operation === "remove" && mutations.at(-1)?.value === "Raven" &&
      JSON.parse(saved).banned_list === platformBan, `${locale}: removing the character ban preserves the platform ban`);
    await click(`.player-access-roster-select[aria-label="${locale === "zh-CN" ? "选择条目 " : "Select entry "}${platformBan}"]`);
    check(!element<HTMLButtonElement>(action("admin_list", "add")).disabled
      && !element<HTMLButtonElement>(action("permitted_list", "add")).disabled,
      `${locale}: a stored platform roster identity can fill other platform rosters without guessing a binding`);
    check(element<HTMLButtonElement>(action("banned_list", "remove")).textContent === (locale === "zh-CN" ? "解除封禁" : "Unban"),
      `${locale}: a stored platform ban is not mislabeled as a character-name ban`);
    await click('[data-list-key="online"]');
    await click(`.player-center-player-select[aria-label="${locale === "zh-CN" ? "选择玩家 Displayed Alias" : "Select player Displayed Alias"}"]`);
    check(element<HTMLButtonElement>(action("banned_list", "add")).disabled,
      `${locale}: display names and session identifiers never become a ban target`);
    check(nativeActionCalls === 0, `${locale}: roster operations never send a live command`);
    await nativeAccountActions(locale);
    await accountControlSetup(locale);
  }
  await draw("zh-CN", "final-preview");
  await click('.player-center-player-select[aria-label="选择玩家 Rowan"]');
  check(errors.length === 0, "Browser and React report no errors");
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false });
  return { status: "passed", checks, browser_errors: errors };
}
Object.assign(window, { __reliabilityFixtureCleanup: async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  await act(async () => { root.unmount(); });
  console.error = originalError;
  return { browser_errors: errors, native_dialogs: 0 };
} });
void run().catch((error) => ({ status: "failed", error: String(error?.stack ?? error), checks, browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
