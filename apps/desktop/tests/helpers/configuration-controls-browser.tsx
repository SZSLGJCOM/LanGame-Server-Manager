import "../../src/app.css";
import { act, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { bootstrapApp, readInstanceDetails, readModuleDetails } from "../../src/api";
import { I18nProvider, useI18n } from "../../src/i18n";
import { GuidedSettingsForm } from "../../src/views/settings/GuidedSettingsForm";
import { InstanceConnectionSettingsPanel } from "../../src/views/settings/InstanceConnectionSettingsPanel";
import { ArkRulesEditor } from "../../src/views/settings/ArkRulesEditor";
import { SevenDaysServerAdminPanel } from "../../src/views/settings/SevenDaysServerAdminPanel";
import { ScumJsonSettingsRenderer } from "../../src/views/settings/ScumJsonSettingsRenderer";
import type { InstanceDetails, ModuleDetails } from "../../src/types";
import type { GuidedSettingsField, SettingsObject } from "../../src/views/settings/settings-schema";
import { controlContractViolations, measureControl } from "./measure-control-contracts";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN");
document.documentElement.dataset.theme = "dark";
const nonce = new URLSearchParams(location.search).get("nonce");
const fixture = document.getElementById("fixture")!;
const root = createRoot(fixture);
const errors: string[] = [];
let catalogRenders = 0;
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
const fields: GuidedSettingsField[] = [
  { key: "fixture_name", title: "服务器名称", type: "string", control: "text", required: false, sectionId: "room",
    description: "服务器列表中显示的名称。", presentation: { state: "editable", owner: "configuration", sectionId: "room" } },
  { key: "fixture_mode", title: "游戏模式", type: "string", control: "select", required: false, sectionId: "room",
    enumOptions: [{ value: "pve", label: "合作生存" }, { value: "pvp", label: "玩家对战" }],
    presentation: { state: "editable", owner: "configuration", sectionId: "room" } },
  { key: "fixture_secret", title: "管理密码", type: "string", control: "password", required: false, sectionId: "room",
    presentation: { state: "editable", owner: "configuration", sectionId: "room", behavior: "secret" } },
  { key: "fixture_public", title: "公开服务器", type: "boolean", control: "checkbox", required: true, defaultValue: true, sectionId: "room",
    presentation: { state: "editable", owner: "configuration", sectionId: "room" } }
];
function check(value: unknown, message: string): asserts value { if (!value) throw new Error(message); }
function element(selector: string) { const value = fixture.querySelector<HTMLElement>(selector); check(value, `Missing ${selector}`); return value; }
function Controls({ details, moduleDetails }: { details: InstanceDetails; moduleDetails: ModuleDetails }) {
  const { t } = useI18n();
  useEffect(() => { catalogRenders++; }, [t]);
  const [settings, setSettings] = useState<SettingsObject>({ fixture_name: "合作世界", fixture_mode: "pve", fixture_secret: "synthetic-fixture", fixture_public: true,
    npc_replacements: '(FromClassName="Raptor_Character_BP_C",ToClassName="")',
    command_permissions: [{ cmd: "help", permission_level: 0 }],
    raid_times: [{ day: "Monday", time: "18:00", "start-announcement-time": "17:55", "end-announcement-time": "20:00" }] });
  const patch = (values: Readonly<SettingsObject>) => setSettings((current) => ({ ...current, ...values }));
  const [ports, setPorts] = useState(details.ports);
  const [bindIp, setBindIp] = useState("0.0.0.0");
  return <main className="configuration-workspace" style={{ padding: 24, height: "100vh", display: "block", overflow: "auto" }}>
    <div className="configuration-workspace__content settings-schema-section" style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 32 }}>
      <section className="configuration-workspace__main" style={{ display: "block" }}><h3>配置 · 常规与连接</h3>
        <GuidedSettingsForm schema={{ title: "常规", fields, sections: [{ id: "room", title: "常规" }] }}
          selectedSectionId="room" settings={settings} onChange={(field, value) => patch({ [field.key]: value })}
          onBatchChange={(changes) => patch(Object.fromEntries(changes.map(({ field, value }) => [field.key, value])))} />
      <InstanceConnectionSettingsPanel details={details} moduleDetails={moduleDetails} bindAddressCandidates={[]} bindIp={bindIp}
        ports={ports} defaultPorts={details.ports} onPortsChange={setPorts} onBindIpChange={setBindIp} />
        <SevenDaysServerAdminPanel sectionId="commands" details={details} moduleDetails={moduleDetails}
          settings={settings} disabled={false} onPatch={patch} />
      </section>
      <section className="configuration-workspace__main" style={{ display: "block" }}><h3>配置 · ARK 与 SCUM 专用编辑器</h3>
        <ArkRulesEditor settingKey="npc_replacements" sectionId="spawns" details={details} moduleDetails={moduleDetails}
          settings={settings} disabled={false} onPatch={patch} />
        <ScumJsonSettingsRenderer fieldKey="raid_times" sectionId="raid" details={details} moduleDetails={moduleDetails}
          settings={settings} disabled={false} onPatch={patch} />
      </section>
    </div>
  </main>;
}
async function run() {
  const bootstrap = await bootstrapApp({ includeSystemSnapshot: false });
  const summary = bootstrap.state.instances.find((instance) => instance.module_id === "minecraft");
  check(summary, "A synthetic Minecraft instance is required");
  const stored = await readInstanceDetails(summary.id);
  const details = { ...stored, summary: { ...stored.summary, status: "Stopped" as const, active_process_count: 0 }, active_run: null };
  const sourceModule = await readModuleDetails(summary.module_id);
  const moduleDetails = { ...sourceModule, runtime: { ...sourceModule.runtime,
    bind_address: { mode: "strict" as const, port_names: [details.ports[0].name], startup_timeout_ms: 30_000 }, port_roles: [{ port_names: [details.ports[0].name], role: "player" as const }] } };
  await act(async () => {
    await Promise.all([import("../../src/i18n-messages"), import("../../src/i18n-messages-zh-cn")]);
    root.render(<I18nProvider><Controls details={details} moduleDetails={moduleDetails} /></I18nProvider>);
  });
  const deadline = performance.now() + 5000;
  while (!fixture.querySelector('[data-field-key="fixture_name"] input') || catalogRenders < 2) {
    check(performance.now() < deadline, `Configuration did not mount: ${fixture.textContent}; ${errors.join("; ")}`);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
  // Flush the provider's fallback-catalog effect before measuring or reporting console errors.
  await act(async () => { await document.fonts.ready; await new Promise(requestAnimationFrame); });
  const measurements = [
    measureControl("generic input", element('[data-field-key="fixture_name"] input'), 38),
    measureControl("generic select", element('[data-field-key="fixture_mode"] select'), 38),
    measureControl("secret input", element('[data-field-key="fixture_secret"] input'), 38),
    measureControl("secret visibility action", element(".configuration-secret-toggle"), 28, "action"),
    measureControl("generic toggle", element('[data-field-key="fixture_public"] .settings-toggle-card'), 38),
    measureControl("listen address", element('.instance-connection-settings__listener-row select'), 38),
    measureControl("player port", element('.instance-port-fields__input'), 38),
    measureControl("join address", element('.player-join-address__select'), 38),
    measureControl("join copy action", element('.player-join-address__copy'), 38, "action"),
    measureControl("7 Days inline remove action", element(".sevendays-admin-row > button"), 38, "action"),
    measureControl("7 Days add action", element(".sevendays-admin-block-toolbar > button"), 30, "action"),
    measureControl("ARK text input", element('.ark-editor__rule input'), 38),
    measureControl("ARK text mode action", element('.ark-editor__heading > button'), 30, "action"),
    measureControl("ARK icon action", element('.ark-editor__icon-button'), 28, "action"),
    measureControl("ARK inline add action", element(".ark-editor__properties > .ark-editor__toolbar button"), 38, "action"),
    measureControl("ARK add action", element('.ark-editor > .secondary-button'), 30, "action"),
    measureControl("SCUM text input", element('.scum-json-settings-renderer input'), 38),
    measureControl("SCUM remove action", element('.scum-json-settings-renderer .guided-field-group-head button'), 30, "action"),
    measureControl("SCUM add action", element('.scum-settings-action-help > button'), 30, "action")
  ];
  const secretBounds = element('[data-field-key="fixture_secret"] input').getBoundingClientRect();
  const toggleBounds = element('[data-field-key="fixture_public"] .settings-toggle-card').getBoundingClientRect();
  check(Math.abs(secretBounds.top - toggleBounds.top) <= 0.5,
    `Same-row password and checkbox controls must align: ${secretBounds.top} vs ${toggleBounds.top}`);
  const listener = element(".instance-connection-settings__listener-row").getBoundingClientRect();
  const join = element(".player-join-address").getBoundingClientRect();
  check(listener.bottom <= join.top || join.bottom <= listener.top || listener.right <= join.left || join.right <= listener.left,
    "Player join address overlaps listen address or port controls");
  for (const selector of [".instance-connection-settings__listener-row select", ".instance-port-fields__input", ".player-join-address__select", ".player-join-address__copy"]) {
    const control = element(selector).getBoundingClientRect();
    const container = element(selector).closest(".configuration-workspace__main")!.getBoundingClientRect();
    check(control.left >= container.left - 1 && control.right <= container.right + 1,
      `Network control escapes its configuration content: ${selector}`);
  }
  check(errors.length === 0, `Browser errors: ${errors.join("; ")}`);
  return { status: "passed", browser_errors: errors, measurements, violations: controlContractViolations(measurements) };
}
run().catch((error) => ({ status: "failed", error: String(error), browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
