import React, { act, StrictMode, useState, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { bootstrapApp } from "../../src/api";
import { createDefaultAiSettings } from "../../src/ai-settings";
import { createInitialAppUpdateState } from "../../src/app-update-model";
import { AppShell } from "../../src/components/AppShell";
import { I18nProvider } from "../../src/i18n";
import { SystemView } from "../../src/views/SystemView";
import type { SystemSnapshot } from "../../src/types";
import "../../src/app.css";

const query = new URLSearchParams(location.search);
const nonce = query.get("nonce");
if (nonce) Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const browserErrors: string[] = [];
addEventListener("error", (event) => browserErrors.push(event.message));
addEventListener("unhandledrejection", (event) => browserErrors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { browserErrors.push(args.map(String).join(" ")); originalError(...args); };
Object.assign(window, { systemPlayerCountsErrors: browserErrors });

const noOperation = () => {};
const scenarios = {
  unknown: { running_instances: 2, player_count_queried_instances: 0, total_online_players: 0 },
  partial: { running_instances: 2, player_count_queried_instances: 1, total_online_players: 7 },
  complete: { running_instances: 2, player_count_queried_instances: 2, total_online_players: 0 },
  idle: { running_instances: 0, player_count_queried_instances: 0, total_online_players: 0 }
} satisfies Record<string, Partial<SystemSnapshot>>;
type Scenario = keyof typeof scenarios;
type FixtureConfig = { locale: "zh-CN" | "en-US"; theme: "dark" | "light" };
let checks = 0;

function Fixture({ props, automatedScenario }: { props: ComponentProps<typeof SystemView>; automatedScenario?: Scenario }) {
  const [manualScenario, setScenario] = useState<Scenario>("unknown");
  const scenario = automatedScenario ?? manualScenario;
  return <>
    {!automatedScenario && <nav aria-label="Player count scenarios" style={{ display: "flex", gap: 12, padding: 16 }}>
      {(Object.keys(scenarios) as Scenario[]).map((name) =>
        <button key={name} type="button" aria-pressed={name === scenario} onClick={() => setScenario(name)}>
          {name}
        </button>)}
    </nav>}
    <SystemView {...props} instances={props.instances.map((instance) => ({ ...instance,
      status: scenario === "idle" ? "Stopped" : "Running" }))}
      snapshot={{ ...props.snapshot, player_count_queryable_instances: scenario === "idle" ? 0 : 2,
      total_player_capacity: 32, ...scenarios[scenario] }} />
  </>;
}

function check(condition: unknown, description: string): asserts condition {
  if (!condition) throw new Error(description);
}
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const selected = document.querySelector<T>(selector);
  check(selected, `Missing ${selector}`);
  return selected;
}
async function settleUntil(predicate: () => boolean, description: string) {
  const deadline = performance.now() + 5000;
  while (!predicate()) {
    check(performance.now() < deadline, description);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
function assertHeaderLayout() {
  const header = element(".system-instance-panel .system-panel-head");
  const heading = element(".system-instance-panel h2");
  const summary = element(".system-instance-summary-bar");
  const items = [...summary.querySelectorAll<HTMLElement>(".system-instance-summary-item")];
  const outer = header.getBoundingClientRect();
  const titleBox = heading.getBoundingClientRect();
  check(header.contains(summary) && items.length === 4, "The instance header must contain all four summary counts");
  check(outer.width > 0 && outer.left >= -1 && outer.right <= innerWidth + 1
    && outer.top >= -1 && outer.bottom <= innerHeight + 1, "Instance header must be visible inside the viewport");
  check(header.scrollWidth <= header.clientWidth + 1, "Instance header must not overflow horizontally");
  const values = items.map((item) => item.querySelector<HTMLElement>("strong")!);
  const valueTop = values[0].getBoundingClientRect().top;
  for (const item of items) {
    const box = item.getBoundingClientRect();
    check(titleBox.right <= box.left + 1 && Math.max(titleBox.top, box.top) < Math.min(titleBox.bottom, box.bottom),
      "Title and each summary count must occupy the same header row");
  }
  for (const target of [heading, summary, ...items, ...items.flatMap((item) => [...item.querySelectorAll<HTMLElement>("span, strong")])]) {
    const box = target.getBoundingClientRect();
    check(box.width > 0 && box.height > 0 && box.left >= outer.left - 1 && box.right <= outer.right + 1
      && box.top >= outer.top - 1 && box.bottom <= outer.bottom + 1,
    `Header content must fit: ${target.textContent}`);
    check(target.scrollWidth <= target.clientWidth + 1, `Header content must not clip horizontally: ${target.textContent}`);
  }
  check(values.every((value) => Math.abs(value.getBoundingClientRect().top - valueTop) <= 1),
    "All four count values must stay on the same row");
  check(document.documentElement.scrollWidth <= innerWidth + 1, "System dashboard must not overflow the window horizontally");
  return { header_width: outer.width, header_height: outer.height, title_width: titleBox.width };
}

async function mount() {
  const config: FixtureConfig = nonce
    ? await fetch("/__system_player_counts_config").then((response) => {
      check(response.ok, "System player count configuration must load");
      return response.json() as Promise<FixtureConfig>;
    })
    : { locale: query.get("locale") === "en-US" ? "en-US" : "zh-CN", theme: query.get("theme") === "light" ? "light" : "dark" };
  localStorage.setItem("langame.locale", config.locale);
  document.documentElement.dataset.theme = config.theme;
  const bootstrap = await bootstrapApp({ includeSystemSnapshot: true });
  if (!bootstrap.state.snapshot) throw new Error("System browser fixture requires a snapshot");
  const root = createRoot(element("#fixture"));
  // The synthetic snapshot tests presentation states, not live game query success.
  // Real AppShell and SystemView geometry are used unchanged in automation.
  const props: ComponentProps<typeof SystemView> = {
    snapshot: bootstrap.state.snapshot,
    instances: bootstrap.state.instances.slice(0, 2).map((instance, index) => ({ ...instance, autostart: index === 0 })),
    bindAddressCandidates: [],
    appSettings: bootstrap.state.settings,
    steamCmdStatus: null, steamCmdBusy: false, steamCmdProgress: null, steamCmdMessage: "",
    onOpenInstance: noOperation, onPickDirectory: async () => null, onSaveAppSettings: noOperation,
    onEnsureSteamCmd: noOperation, onUninstallSteamCmd: noOperation
  };
  if (!nonce) {
    root.render(<I18nProvider><Fixture props={props} /></I18nProvider>);
    return;
  }
  check(props.instances.length === 2, "The browser fixture requires two synthetic instances");
  const aiSettings = createDefaultAiSettings();
  const shellProps: Omit<ComponentProps<typeof AppShell>, "children"> = {
    activeView: "system", theme: config.theme, serverCount: 2, aiSettings, activityText: "",
    appUpdatesEnabled: true, appUpdateState: createInitialAppUpdateState("0.1.0"), assistant: { panelTitle: "Assistant", tone: "info" }, assistantDraft: "",
    assistantExecution: { status: "idle", promptLabel: null, result: null, error: null },
    assistantMessages: [], assistantConversations: [], assistantActiveConversationId: null,
    assistantInput: { aiSettings, locale: config.locale, activeJobsCount: 0, activeView: "system", bootstrap,
      storageReady: true, libraryPage: "catalog", overlayNames: [], runtimeAutoRefreshPaused: false, runtimeRefreshIssue: null,
      selectedInstanceDetails: null, selectedInstanceId: null, selectedModuleId: null, selectedInstanceModuleDetails: null,
      selectedLaunchPlan: null, selectedLaunchPlanError: null, selectedLogDocument: null,
      selectedModuleDetails: null, selectedRuntime: null, serverWorkspaceSection: "overview", steamCmdStatus: null },
    jobs: [], steamCmdProgress: null, steamCmdMessage: "", steamCmdStopPending: false, steamCmdStopError: null,
    installationStopPendingIds: [], installationStopErrors: {}, onCancelInstallation: noOperation, onCancelSteamCmd: noOperation,
    runtimeRefreshIssue: null, runtimeAutoRefreshPaused: false, runtimePollIntervalMs: 5000, runtimeRefreshFailureLimit: 3,
    onResumeRuntimeAutoRefresh: noOperation, onAssistantAction: noOperation, onAssistantDeleteConversation: noOperation,
    onAssistantDraftChange: noOperation, onAssistantNewConversation: noOperation, onAssistantSelectConversation: noOperation,
    onAssistantRunPrompt: noOperation, onAssistantSendMessage: noOperation, onCheckAppUpdate: noOperation,
    onClearAiSecret: async (next) => next, onInstallAppUpdate: noOperation, onSaveAiSettings: async (next) => next,
    onSelectView: noOperation, onThemeChange: noOperation
  };
  const render = async (scenario: Scenario) => {
    await act(async () => { root.render(<StrictMode><I18nProvider><AppShell {...shellProps}>
      <Fixture props={props} automatedScenario={scenario} />
    </AppShell></I18nProvider></StrictMode>); });
  };
  await render("unknown");
  await settleUntil(() => Boolean(document.querySelector(".system-instance-panel")), "System dashboard did not mount");
  await settleUntil(() => document.querySelector(".shell-locale-button")?.textContent === (config.locale === "en-US" ? "ZH" : "EN"),
    "Requested interface language did not load");
  await document.fonts.ready;
  await settleUntil(() => [...document.querySelectorAll(".system-instance-item-address")]
    .every((address) => !/Loading|正在读取/.test(address.textContent ?? "")), "Instance connection reads did not settle");
  const labels = config.locale === "zh-CN"
    ? { title: "实例概览", running: "运行", players: "人数", unknown: "未知", partial: "部分数据", errors: "异常", autostart: "自启动", coverage: "人数查询覆盖" }
    : { title: "Instance Overview", running: "Running", players: "Players", unknown: "Unknown", partial: "Partial data", errors: "Errors", autostart: "Autostart", coverage: "Player queries" };
  const expectedValues = { unknown: "— / 32", partial: "≥7 / 32", complete: "0 / 32", idle: "0" };
  const observed: Partial<Record<Scenario, { value: string; coverage: string }>> = {};
  for (const scenario of Object.keys(scenarios) as Scenario[]) {
    await render(scenario);
    element(".system-instance-panel").scrollIntoView({ block: "nearest", inline: "nearest" });
    const items = [...document.querySelectorAll<HTMLElement>(".system-instance-summary-item")];
    const values = items.map((item) => item.querySelector("strong")?.textContent?.trim());
    const note = scenario === "unknown" ? labels.unknown : scenario === "partial" ? labels.partial : null;
    check(element(".system-instance-panel h2").textContent === labels.title, "Instance overview must use the selected language");
    check(JSON.stringify(items.map((item) => item.querySelector("span")?.textContent))
      === JSON.stringify([labels.running, note ? `${labels.players} · ${note}` : labels.players, labels.errors, labels.autostart]),
    `${scenario} must show all four localized count labels and the player data state`);
    check(JSON.stringify(values) === JSON.stringify([scenario === "idle" ? "0 / 2" : "2 / 2", expectedValues[scenario], "0", "1"]),
      `${scenario} count values are incorrect: ${JSON.stringify(values)}`);
    const players = items[1].querySelector<HTMLElement>("strong")!;
    const coverage = `${labels.coverage} ${scenarios[scenario].player_count_queried_instances} / ${scenarios[scenario].running_instances}`;
    players.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    check(players.title === coverage, `${scenario} hover title must report successful queries / running instances`);
    observed[scenario] = { value: players.textContent!.trim(), coverage: players.title };
    assertHeaderLayout();
    checks++;
  }
  // Capture the longest player label after verifying all four transitions.
  await render("partial");
  const layout = assertHeaderLayout();
  check(browserErrors.length === 0, `Unexpected browser errors: ${browserErrors.join("; ")}`);
  checks++;
  return { status: "passed", checks, ...config, scenarios: observed, layout, browser_errors: browserErrors };
}

if (nonce) {
  let watchdog: ReturnType<typeof setTimeout>;
  void Promise.race([mount(), new Promise<never>((_, reject) => {
    watchdog = setTimeout(() => reject(new Error(`System player count checks stalled after ${checks} scenarios`)), 25000);
  })]).finally(() => {
    clearTimeout(watchdog);
    Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false });
  }).catch((error) => ({ status: "failed", checks,
    error: error instanceof Error ? error.stack : String(error), browser_errors: browserErrors }))
    .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
} else {
  void mount();
}
