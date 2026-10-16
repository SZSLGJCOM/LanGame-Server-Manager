import { act, useState, type ReactNode } from "react";
import type { Root } from "react-dom/client";
import { ActivityBar } from "../../src/components/ActivityBar";
import { ActivityNoticeTarget } from "../../src/components/ActivityNotice";
import { createActivityNoticeStore } from "../../src/components/activity-notice-store";
import { I18nProvider } from "../../src/i18n";
import type { InstanceDetails, ModuleDetails } from "../../src/types";
import type { ValheimWorldRules } from "../../src/valheim-world";
import { ValheimWorldRulesPanel } from "../../src/views/settings/ValheimWorldRulesPanel";
import { ValheimWorldRulesProvider } from "../../src/views/settings/ValheimWorldRulesContext";
import { ValheimWorldRulesReadback } from "../../src/views/settings/ValheimWorldRulesReadback";

export function WorldRulesActivityFixture({ children }: { children: ReactNode }) {
  const [store] = useState(createActivityNoticeStore);
  return <ActivityNoticeTarget.Provider value={{ element: null, store, dismissLabel: "Dismiss" }}>
    <main className="server-detail-panel" style={{ height: "calc(100vh - 80px)", margin: 16 }}>{children}</main>
    <footer data-valheim-test-activity="true" className="shell-activity-bar"><ActivityBar store={store} entries={[]} /></footer>
  </ActivityNoticeTarget.Provider>;
}

export function createValheimNativeTransport() {
  const native = {
    keys: [] as string[], source: "saved" as ValheimWorldRules["source"], version: 42,
    error: null as string | null, calls: [] as Array<{ instanceId: string; worldName: string }>,
    deferred: null as null | { response?: unknown; resolve?: (value: unknown) => void },
    blockNext() {
      const pending: { response?: unknown; resolve?: (value: unknown) => void } = {};
      native.deferred = pending;
      return () => {
        if (!pending.resolve) throw new Error("Deferred native read has not started");
        pending.resolve(pending.response);
      };
    }
  };
  Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: { invoke: async (command: string, args: Record<string, unknown>) => {
    if (command !== "read_valheim_world_rules") throw new Error(`Unexpected native command: ${command}`);
    native.calls.push({ instanceId: String(args.instanceId), worldName: String(args.worldName) });
    if (native.error) throw new Error(native.error);
    const response = { instance_id: args.instanceId, world_name: args.worldName, source: native.source,
      world_version: native.source === "saved" ? native.version : null, saved_keys: structuredClone(native.keys) };
    const pending = native.deferred;
    if (!pending) return response;
    native.deferred = null;
    pending.response = response;
    return new Promise<unknown>((resolve) => { pending.resolve = resolve; });
  } } });
  return native;
}

type Check = (condition: unknown, description: string) => asserts condition;
export async function verifyValheimReadback(root: Root, locale: string,
  native: ReturnType<typeof createValheimNativeTransport>, instance: InstanceDetails,
  moduleDetails: ModuleDetails, initial: Record<string, unknown>, check: Check) {
  const fixture = document.getElementById("fixture");
  if (!fixture) throw new Error("Fixture root is missing");
  const messages = locale === "zh-CN"
    ? { on: "开启", off: "关闭", normal: "标准", easy: "简单", hard: "困难", custom: "自定义", resources: "3 倍", unknown: "尚未读取", retry: "重试" }
    : { on: "On", off: "Off", normal: "Normal", easy: "Easy", hard: "Hard", custom: "Custom", resources: "3 ×", unknown: "Not read", retry: "Retry" };
  let values: Record<string, unknown> = {};
  const patches: Readonly<Record<string, unknown>>[] = [];
  let updateDetails: (status: string, runId: number | null) => void;
  function Scenario() {
    const [settings, setSettings] = useState(initial);
    const [details, setDetails] = useState({ ...instance, settings_json: JSON.stringify(initial) });
    values = settings;
    updateDetails = (status, runId) => setDetails((current) => ({ ...current,
      summary: { ...current.summary, status, active_process_count: runId === null ? 0 : 1 },
      active_run: runId === null ? null : { run_id: runId } }));
    const props = { details, moduleDetails, settings, disabled: false, sectionId: "world", onPatch: (patch: Readonly<Record<string, unknown>>) => {
      patches.push(structuredClone(patch));
      setSettings((current) => ({ ...current, ...patch }));
    } };
    return <ValheimWorldRulesProvider details={details}>
      <ValheimWorldRulesPanel {...props} /><ValheimWorldRulesReadback {...props} />
    </ValheimWorldRulesProvider>;
  }
  function element<T extends Element>(selector: string): T {
    const result = fixture!.querySelector<T>(selector);
    if (!result) throw new Error(`Missing native readback element: ${selector}`);
    return result;
  }
  const activity = () => element<HTMLElement>("[data-valheim-test-activity]");
  const panel = () => element<HTMLElement>("main");
  const row = (name: string) => [...element<HTMLTableRowElement>(`[data-valheim-readback-rule="${name}"]`).cells].slice(1).map((cell) => cell.textContent);
  const toggle = (name: string) => element<HTMLInputElement>(`[data-valheim-rule="valheim_key_${name}"] input`);
  const modifier = (name: string) => element<HTMLSelectElement>(`[data-valheim-rule="valheim_modifier_${name}"] select`);
  const preset = () => element<HTMLSelectElement>('[data-field-key="world_preset"] select');
  const frame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
  async function draw(suffix: string) {
    await act(async () => { root.render(<I18nProvider key={`${locale}-native-${suffix}`}>
      <WorldRulesActivityFixture><Scenario /></WorldRulesActivityFixture>
    </I18nProvider>); await frame(); });
  }
  async function change(select: HTMLSelectElement, value: string) {
    await act(async () => { select.value = value; select.dispatchEvent(new Event("change", { bubbles: true })); });
  }
  async function setToggle(name: string, checked: boolean) {
    await act(async () => { const input = toggle(name); if (input.checked !== checked) input.labels?.[0]?.click(); });
  }
  async function transition(status: string, runId: number | null) {
    await act(async () => { updateDetails(status, runId); await frame(); });
  }
  const editable = () => [...panel().querySelectorAll<HTMLInputElement | HTMLSelectElement>("select,input[type='checkbox']")]
    .every((input) => !input.disabled);
  const unchanged = () => JSON.stringify(values) === JSON.stringify(initial) && patches.length === 0;

  native.source = "saved";
  native.version = 42;
  native.error = null;
  native.keys = ["playerdamage 85", "enemydamage 150", "enemyspeedsize 110", "enemyleveluprate 120", "nomap", "resourcerate 300"];
  await draw("v42");
  check(row("combat").every((value) => value === messages.hard) && row("resources").every((value) => value === messages.resources),
    `${locale}: version 42 saved metadata displays concrete current and next rules in the read-only table`);
  check(preset().value === "" && modifier("combat").value === "" && !toggle("nomap").checked && editable(),
    `${locale}: saved enabled rules do not replace startup configuration or disable its controls`);
  check(unchanged(), `${locale}: native readback never backfills persisted startup settings`);
  check(!panel().querySelector(".shell-activity-notice") && activity().querySelector(".shell-activity-notice"),
    `${locale}: native status uses the real activity notice store outside configuration DOM`);
  await change(modifier("combat"), "easy");
  check(modifier("combat").value === "easy" && row("combat")[0] === messages.hard && row("combat")[1] === messages.custom,
    `${locale}: choosing Easy over saved Hard preserves native residual keys and projects Custom`);
  await change(modifier("combat"), "");
  await setToggle("nomap", true);
  await setToggle("nomap", false);
  check(values.world_set_keys === "" && !toggle("nomap").checked && row("nomap").every((value) => value === messages.on),
    `${locale}: removing an enabled world's startup override retains its effective saved rule`);
  await change(preset(), "normal");
  check(row("nomap")[0] === messages.on && row("nomap")[1] === messages.off && !toggle("nomap").checked,
    `${locale}: selecting Normal resets the projected baseline without inventing a key override`);
  check(activity().textContent?.includes(locale === "zh-CN" ? "下次启动" : "next server start"),
    `${locale}: pending startup changes are reported in the activity bar`);
  native.keys = ["preset hammer"];
  patches.length = 0;
  await draw("preset-tag");
  check(row("nobuildcost").every((value) => value === messages.on) && row("passivemobs").every((value) => value === messages.on) &&
    row("raids").every((value) => value === (locale === "zh-CN" ? "无袭击" : "No Raids")) &&
    !toggle("nobuildcost").checked && editable() && unchanged(),
    `${locale}: saved preset tags expand only in readback without becoming startup overrides`);
  await change(modifier("raids"), "more");
  check(modifier("raids").value === "more" && row("raids")[0] === (locale === "zh-CN" ? "无袭击" : "No Raids") &&
    row("raids")[1] === (locale === "zh-CN" ? "较多" : "More"),
    `${locale}: a saved Hammer preset tag is not replayed over the explicit raid override`);
  native.keys = ["preset hard"];
  await draw("preset-tag-hard");
  await change(modifier("combat"), "easy");
  check(modifier("combat").value === "easy" && row("combat")[0] === messages.hard && row("combat")[1] === messages.custom,
    `${locale}: tagged Hard rules retain residual keys without replaying the tag over Easy`);

  native.keys = [];
  native.error = "fixture-world-read-denied";
  patches.length = 0;
  await draw("failed");
  check(activity().textContent?.includes("fixture-world-read-denied") && !panel().textContent?.includes("fixture-world-read-denied"),
    `${locale}: detailed native failures appear only in the activity bar`);
  check(editable() && unchanged() && !panel().querySelector("input:indeterminate"),
    `${locale}: failed readback leaves every valid configuration control editable without fabricated defaults`);
  check(row("combat").every((value) => value === messages.unknown), `${locale}: an unknown saved baseline stays in the read-only table`);
  await change(modifier("combat"), "easy");
  check(modifier("combat").value === "easy" && row("combat").every((value) => value === messages.unknown),
    `${locale}: choosing Easy over unknown saved rules keeps the partial native projection unknown`);
  await change(modifier("combat"), "hard");
  await setToggle("nomap", true);
  check(row("combat")[0] === messages.unknown && row("combat")[1] === messages.hard &&
    row("nomap")[0] === messages.unknown && row("nomap")[1] === messages.on && row("resources")[1] === messages.unknown,
    `${locale}: explicit startup overrides have known projections even when native readback fails`);
  check(values.future_setting === initial.future_setting && values.server_password === initial.server_password && !values.world_preset,
    `${locale}: editing after a read failure preserves unrelated values and does not select a preset`);
  await change(preset(), "normal");
  check(row("resources")[0] === messages.unknown && row("resources")[1] === messages.normal,
    `${locale}: an explicit preset determines the next baseline even when current rules are unknown`);
  await change(modifier("combat"), "easy");
  check(modifier("combat").value === "easy" && row("combat")[0] === messages.unknown && row("combat")[1] === messages.easy,
    `${locale}: an explicit Normal baseline makes the Easy startup projection determinate`);
  await change(modifier("combat"), "hard");
  await change(preset(), "");
  native.error = null;
  const beforeRetry = native.calls.length;
  const retry = activity().querySelector<HTMLButtonElement>(".shell-activity-notice-actions button");
  check(retry?.textContent === messages.retry && !retry.disabled, `${locale}: the activity bar provides native retry`);
  await act(async () => { retry.click(); await frame(); });
  check(native.calls.length === beforeRetry + 1 && row("resources").every((value) => value === messages.normal) &&
    modifier("combat").value === "hard" && toggle("nomap").checked,
    `${locale}: native retry recovers readback and retains edited startup overrides`);

  const beforeLifecycle = native.calls.length;
  await transition("Starting", 10);
  check(native.calls.length === beforeLifecycle, `${locale}: Starting defers native metadata reads`);
  await transition("Running", 10);
  check(native.calls.length === beforeLifecycle + 1, `${locale}: entering Running refreshes metadata exactly once`);
  await transition("Running", 10);
  check(native.calls.length === beforeLifecycle + 1, `${locale}: identical runtime polling does not reread metadata`);
  await transition("Running", 11);
  check(native.calls.length === beforeLifecycle + 2, `${locale}: a new run identity refreshes metadata once`);
  await transition("Stopping", 11);
  check(native.calls.length === beforeLifecycle + 2, `${locale}: Stopping defers native metadata reads`);
  await transition("Stopped", null);
  check(native.calls.length === beforeLifecycle + 3, `${locale}: a completed stop refreshes metadata exactly once`);
  await transition("Stopped", null);
  check(native.calls.length === beforeLifecycle + 3, `${locale}: stopped polling does not reread metadata`);
  native.keys = ["nomap"];
  const release = native.blockNext();
  await transition("Running", 12);
  check(activity().querySelector<HTMLButtonElement>(".shell-activity-notice-actions button")?.disabled && editable(),
    `${locale}: an in-flight read disables only its refresh action`);
  native.keys = [];
  await transition("Stopped", null);
  check(row("nomap")[0] === messages.off, `${locale}: the latest lifecycle read is displayed`);
  await act(async () => { release(); await frame(); });
  check(row("nomap")[0] === messages.off, `${locale}: a late result from an earlier run cannot replace current readback`);

  native.source = "new_world";
  patches.length = 0;
  await draw("new");
  check(row("combat")[0] === messages.unknown && row("combat")[1] === messages.normal &&
    row("nomap")[0] === messages.unknown && row("nomap")[1] === messages.off && editable() && unchanged(),
    `${locale}: new-world readback projects native defaults without storing them in configuration`);
  native.source = "missing_metadata";
  await draw("missing");
  check(row("combat").every((value) => value === messages.unknown) && editable() && unchanged(),
    `${locale}: missing metadata is isolated to readback and leaves startup editing available`);
  check(!panel().querySelector(".shell-activity-notice") && activity().querySelector(".shell-activity-notice-actions button"),
    `${locale}: missing metadata and manual refresh remain in the activity bar`);
  native.source = "saved";
  native.keys = [];
}
