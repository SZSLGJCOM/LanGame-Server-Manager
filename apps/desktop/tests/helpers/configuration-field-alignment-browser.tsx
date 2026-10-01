import { act, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { bootstrapApp, readInstanceDetails, readModuleDetails } from "../../src/api";
import { I18nProvider, useI18n } from "../../src/i18n";
import { NativeSavePolicyFields } from "../../src/views/servers/NativeSavePolicyFields";
import { GuidedSettingsForm } from "../../src/views/settings/GuidedSettingsForm";
import { WindroseWorldSettingsPanel } from "../../src/views/settings/WindroseWorldSettingsPanel";
import type { ConfigurationSpecializedRendererProps } from "../../src/views/settings/module-types";
import type { GuidedSettingsField, SettingsObject } from "../../src/views/settings/settings-schema";
import "../../src/app.css";
import "../../src/views/servers/workbench/operations/maintenance.css";
import "../../src/views/servers/workbench/operations/maintenance-workspace.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN");
document.documentElement.dataset.theme = "dark";
const nonce = new URLSearchParams(location.search).get("nonce");
const fixture = document.getElementById("fixture")!;
const root = createRoot(fixture);
const errors: string[] = [];
const measurements: { pair: string; left: number; right: number }[] = [];
let checks = 0;
let renders = 0;
let catalogRenders = 0;
let patches = 0;
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };

function check(condition: unknown, message: string): asserts condition { if (!condition) throw new Error(message); }
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const found = fixture.querySelector<T>(selector);
  check(found, `Missing ${selector}`);
  return found;
}
function field(key: string, title: string, control: GuidedSettingsField["control"] = "text",
  extra: Partial<GuidedSettingsField> = {}): GuidedSettingsField {
  return { key, title, control, type: control === "checkbox" ? "boolean" : "string", required: true,
    sectionId: "room", description: `${title}的设置说明。`, defaultValue: control === "checkbox" ? false : "合成配置",
    presentation: { state: "editable", owner: "configuration", sectionId: "room" }, ...extra };
}
const text = field("name", "服务器名称");
const toggle = field("enabled", "公开服务器", "checkbox");
const mode = field("mode", "游戏模式", "select", { defaultValue: "pve",
  enumOptions: [{ value: "pve", label: "合作生存" }, { value: "pvp", label: "玩家对战" }] });
const secret = field("secret", "管理密码", "password", {
  presentation: { ...text.presentation, behavior: "secret" } });
const long = field("long", "只有允许名单中的玩家在管理员确认访问权限后才能进入当前服务器的配置说明".repeat(3));
const badge = field("badge", "重新启动后生效的设置", "text", {
  presentation: { ...text.presentation, restartScope: "server" } });
const textarea = field("notes", "多行公告", "textarea", {
  presentation: { ...text.presentation, behavior: "multiline" } });
interface CaseProps { fields: GuidedSettingsField[]; width?: number; disabled?: boolean; readOnly?: boolean;
  invalid?: string[]; values?: SettingsObject; title?: string; maintenance?: boolean;
  consumer?: Omit<ConfigurationSpecializedRendererProps, "settings" | "onPatch"> }
function Case(props: CaseProps) {
  const { t } = useI18n();
  useEffect(() => { catalogRenders++; }, [t]);
  const [settings, setSettings] = useState<SettingsObject>(() => ({
    ...Object.fromEntries(props.fields.map((candidate) => [candidate.key, candidate.defaultValue])), ...props.values }));
  const patch = (values: SettingsObject) => { patches++; setSettings((current) => ({ ...current, ...values })); };
  return <main className={`configuration-workspace${props.maintenance ? " maintenance-workspace" : ""}`}
    style={{ display: "block", height: "100vh", overflow: "auto", padding: 24 }}>
    <h2>{props.title ?? "配置控件 · 同行对齐验收"}</h2>
    <button id="before-fields" className="secondary-button">配置前的操作</button>
    <div className="configuration-workspace__main" style={{ display: "block", width: props.width ?? 900, marginTop: 16 }}>
      {props.consumer ? <WindroseWorldSettingsPanel {...props.consumer} settings={settings} onPatch={patch} />
        : props.maintenance ? <div className="server-save-policy-editor"><div className="server-native-save-policies">
          <NativeSavePolicyFields instanceId="alignment-fixture" moduleId="dontstarve" fields={props.fields}
            settings={settings} issues={[]} disabled={false} t={t} onChange={(candidate, value) => patch({ [candidate.key]: value })} />
        </div></div> : <GuidedSettingsForm schema={{ title: "配置", fields: props.fields, sections: [{ id: "room", title: "配置" }] }}
        selectedSectionId="room" idPrefix="alignment" settings={settings} disabled={props.disabled} readOnly={props.readOnly}
        validationIssues={props.invalid?.map((fieldKey) => ({ fieldKey, reason: "invalid_value", message: "设置值不符合要求，请检查后再保存。" }))}
        onChange={(candidate, value) => patch({ [candidate.key]: value })}
        onBatchChange={(changes) => patch(Object.fromEntries(changes.map(({ field: candidate, value }) => [candidate.key, value])))} />}
    </div>
  </main>;
}
async function render(props: CaseProps) {
  await act(async () => { root.render(<I18nProvider><Case key={++renders} {...props} /></I18nProvider>); });
  const deadline = performance.now() + 5000;
  while (!fixture.querySelector("[data-field-key]") || catalogRenders < 2) {
    check(performance.now() < deadline, `Configuration did not mount: ${errors.join("; ")}`);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
  await act(async () => { await document.fonts.ready; await new Promise(requestAnimationFrame); });
}
function fieldRoot(key: string) { return element(`[data-field-key="${key}"]`); }
function control(key: string): HTMLElement {
  const container = fieldRoot(key);
  const checkbox = container.querySelector<HTMLInputElement>('input[type="checkbox"]');
  const target = checkbox?.closest<HTMLElement>(".settings-toggle-card")
    ?? container.querySelector<HTMLElement>("input, select, textarea");
  check(target, `Missing visible control for ${key}`);
  return target;
}
function aligned(left: string, right: string) {
  const a = control(left).getBoundingClientRect();
  const b = control(right).getBoundingClientRect();
  check(a.width > 0 && b.width > 0 && a.right <= b.left + 1, `${left}/${right} must occupy two columns`);
  measurements.push({ pair: `${left}/${right}`, left: a.top, right: b.top });
  check(Math.abs(a.top - b.top) <= 1, `${left}/${right} control tops differ: ${a.top} / ${b.top}`);
}
function compact(key: string) {
  check(Math.abs(control(key).getBoundingClientRect().height - 38) <= 1, `${key} must retain its 38px control height`);
}
async function nativeTab() {
  await act(async () => {
    const response = await fetch(`/__reliability_key/${nonce}/Tab`, { method: "POST" });
    check(response.ok, "Native Tab dispatch failed");
  });
}
Object.assign(window, { __reliabilityFixtureCleanup: async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  await act(async () => { root.unmount(); });
  check(!document.querySelector(".configuration-field-help-tooltip"), "Unmount must remove tooltip portals");
  console.error = originalError;
  return { browser_errors: errors, native_dialogs: 0 };
} });

async function run() {
  await Promise.all([import("../../src/i18n-messages"), import("../../src/i18n-messages-zh-cn")]);
  const bootstrap = await bootstrapApp({ includeSystemSnapshot: false });
  const summary = bootstrap.state.instances[0];
  check(summary, "Browser mock must provide a synthetic instance");
  const [stored, moduleDetails] = await Promise.all([readInstanceDetails(summary.id), readModuleDetails("windrose")]);
  for (const pair of [[text, toggle], [toggle, text], [mode, toggle], [secret, toggle], [long, toggle], [badge, toggle]]) {
    await render({ fields: pair });
    aligned(pair[0].key, pair[1].key);
    compact(pair[1].key);
    if (pair[0] === long) {
      const label = element('[data-field-key="long"] .settings-field-label');
      check(label.getBoundingClientRect().height > Number.parseFloat(getComputedStyle(label).lineHeight) * 2,
        "Long-label fixture must actually wrap across multiple lines");
    }
    if (pair[0] === badge) check(fieldRoot("badge").querySelector(".configuration-field-restart-scope"), "Restart badge is required");
    if (pair[0] === secret) {
      const action = element<HTMLButtonElement>(".configuration-secret-toggle");
      const input = element<HTMLInputElement>('[data-field-key="secret"] input');
      check(input.type === "password", "Secret starts concealed");
      await act(async () => { action.click(); });
      check(input.getAttribute("type") === "text" && action.getAttribute("aria-pressed") === "true", "Visibility action must still work");
      aligned("secret", "enabled");
    }
    checks++;
  }

  await render({ fields: [text, toggle], invalid: ["name", "enabled"] });
  aligned("name", "enabled");
  for (const key of ["name", "enabled"]) {
    compact(key);
    const input = element<HTMLInputElement>(`[data-field-key="${key}"] input`);
    const error = document.getElementById(input.getAttribute("aria-errormessage")!);
    check(error, "Invalid fields must expose an error element");
    check(input.getAttribute("aria-invalid") === "true" && error?.textContent?.includes("设置值不符合要求"), "Error association is required");
    check(error.getBoundingClientRect().top >= control(key).getBoundingClientRect().bottom - 1, "Errors must follow the control");
  }
  checks++;

  const otherToggle = field("other", "允许玩家加入", "checkbox");
  await render({ fields: [toggle, otherToggle] });
  aligned("enabled", "other");
  check(Math.abs(control("enabled").getBoundingClientRect().top - element(".settings-schema-grid").getBoundingClientRect().top) <= 1,
    "A checkbox-only row must not reserve an empty label row");
  checks++;
  await render({ fields: [long, text] });
  aligned("long", "name");
  checks++;

  await render({ fields: [text, toggle, textarea, mode, otherToggle] });
  aligned("name", "enabled");
  aligned("mode", "other");
  const wide = control("notes").getBoundingClientRect();
  const grid = element(".settings-schema-grid").getBoundingClientRect();
  check(Math.abs(wide.width - grid.width) <= 1 && wide.top > control("name").getBoundingClientRect().bottom
    && wide.bottom < control("mode").getBoundingClientRect().top, "Full-width textarea must separate adjacent rows");
  checks++;

  await render({ fields: [text, toggle], disabled: true });
  aligned("name", "enabled");
  const beforeDisabled = patches;
  await act(async () => { control("enabled").click(); });
  check(element<HTMLInputElement>('[data-field-key="name"] input').disabled
    && element<HTMLInputElement>('[data-field-key="enabled"] input').disabled && patches === beforeDisabled, "Disabled fields must remain inert");
  checks++;

  await render({ fields: [secret, toggle], readOnly: true, values: { secret: "fixture-password", enabled: true } });
  aligned("secret", "enabled");
  const beforeReadOnly = patches;
  check(element<HTMLInputElement>('[data-field-key="secret"] input').readOnly, "Saved text must remain read-only");
  await act(async () => { control("enabled").click(); element<HTMLButtonElement>(".configuration-secret-toggle").click(); });
  check(element<HTMLInputElement>('[data-field-key="enabled"] input').disabled && patches === beforeReadOnly, "Saved fields cannot patch settings");
  checks++;

  await render({ fields: [text, toggle], readOnly: true, values: { enabled: undefined } });
  aligned("name", "enabled");
  check(control("enabled") instanceof HTMLTextAreaElement && !fieldRoot("enabled").querySelector('input[type="checkbox"]'),
    "An absent saved boolean must retain its read-only value display instead of becoming a toggle");
  checks++;

  await render({ fields: [text, toggle], width: 600 });
  const first = control("name").getBoundingClientRect();
  const second = control("enabled").getBoundingClientRect();
  check(second.top > first.bottom && Math.abs(first.left - second.left) <= 1 && Math.abs(first.width - second.width) <= 1,
    "Narrow containers must form one column without offsets");
  check(Math.abs(second.top - fieldRoot("enabled").getBoundingClientRect().top) <= 1, "Single-column toggles must not reserve a label row");
  checks++;

  await render({ fields: [text, toggle] });
  await act(async () => { element("#before-fields").focus(); });
  await nativeTab();
  check(document.activeElement === fieldRoot("name").querySelector("input"), "Tab must reach the first field");
  await nativeTab();
  const checkbox = element<HTMLInputElement>('[data-field-key="enabled"] input');
  check(document.activeElement === checkbox, "Tab must reach the checkbox in field order");
  check(Boolean(checkbox.labels?.length), "Checkbox must retain an associated label");
  checks++;
  const beforeClick = patches;
  await act(async () => { checkbox.labels![0].click(); });
  check(checkbox.checked && patches === beforeClick + 1, "Clicking the checkbox card label must toggle once");
  checks++;

  await render({ fields: [], values: { world_island_id: "synthetic-world" }, consumer: {
    sectionId: "world", disabled: false, moduleDetails,
    details: { ...stored, summary: { ...stored.summary, status: "Stopped" } } } });
  const nativeLabel = element('[data-field-key="world_preset_type"] .settings-field-label').getBoundingClientRect();
  check(Math.abs(control("world_preset_type").getBoundingClientRect().top - nativeLabel.bottom - 6) <= 1,
    "Handwritten Windrose fields must retain their 6px label/control gap");
  checks++;

  await render({ maintenance: true, fields: [field("autosaver_enabled", "自动保存", "checkbox"),
    field("max_snapshots", "备份数量", "number", { type: "integer", defaultValue: 5 })] });
  const saveLabel = element('[data-field-key="max_snapshots"] .settings-field-label').getBoundingClientRect();
  check(Math.abs(control("max_snapshots").getBoundingClientRect().top - saveLabel.bottom - 6) <= 1,
    "Native save fields must retain the maintenance 6px label/control gap");
  check(Math.abs(control("autosaver_enabled").getBoundingClientRect().top - fieldRoot("autosaver_enabled").getBoundingClientRect().top) <= 1,
    "Native save toggles must not create an empty heading track");
  checks++;

  await render({ fields: [text, toggle, secret, otherToggle, badge, mode], title: "配置控件 · 对齐结果" });
  aligned("name", "enabled"); aligned("secret", "other"); aligned("badge", "mode");
  check(element(".settings-schema-grid").scrollWidth <= element(".settings-schema-grid").clientWidth + 1,
    "Configuration fields must not overflow horizontally");
  check(errors.length === 0, `Unexpected browser errors: ${errors.join("; ")}`);
  checks++;
  return { status: "passed", checks, measurements, browser_errors: errors, keyboard_events: "native CDP Tab" };
}
void run().catch((error) => ({ status: "failed", checks, error: error instanceof Error ? error.stack : String(error), browser_errors: errors }))
  .then((report) => {
    Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false });
    return fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) });
  });
