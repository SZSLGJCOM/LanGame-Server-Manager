import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import React, { act, useState } from "react";
import { createRoot } from "react-dom/client";
import schema from "../../../../modules/enshrouded/schema.json";
import { applyMockPlayerAccessMutation } from "../../src/api-mock/player-access";
import { I18nProvider, useI18n } from "../../src/i18n";
import { PlayerAccessRosterEditor } from "../../src/views/servers/player-center/PlayerAccessRosterEditor";
import { PlayerAccessRosterList } from "../../src/views/servers/player-center/PlayerAccessRosterList";
import { usePlayerAccess } from "../../src/views/servers/player-center/use-player-access";
import type { InstanceDetails, InstancePlayerAccessMutationInput, ModuleDetails } from "../../src/types";
import "../../src/app.css";
import "../../src/views/servers/workbench.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
document.documentElement.dataset.theme = "dark";
localStorage.setItem("langame.locale", "en-US");
const nonce = new URLSearchParams(location.search).get("nonce");
const fixture = document.getElementById("fixture");
if (!fixture) throw new Error("Fixture root is missing");
const root = createRoot(fixture);
const checks: string[] = [];
const errors: string[] = [];
const calls: InstancePlayerAccessMutationInput[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
window.confirm = () => { throw new Error("Roster confirmation must stay inline"); };
const maximum = "18446744073709551615";
let saved = JSON.stringify({ banned_player_ids: "76561198000000001,00042" });
const moduleDetails: ModuleDetails = {
  summary: { id: "enshrouded", name: "Enshrouded", version: "1", install_state: "Installed", supported_platforms: ["windows"] },
  default_ports: [], runtime: {}, schema_json: JSON.stringify(schema)
};
const instance: InstanceDetails = {
  summary: { id: "hash-roster", module_id: "enshrouded", name: "Hash roster", status: "Running", active_process_count: 1,
    autostart: false, bind_ip: "127.0.0.1" },
  ports: [], settings_json: saved, config_file_path: "", saves_path: "", backup_uses_declared_saves_path: false,
  auto_backup_on_stop: false, backup_retention_count: 3
};

function Fixture() {
  const { locale } = useI18n();
  const [details, setDetails] = useState({ ...instance, settings_json: saved });
  const access = usePlayerAccess({ details, moduleDetails, onApplyPlayerAccessMutation: async (input) => {
    if (input.instanceId !== instance.summary.id || input.fieldKey !== "banned_player_ids") throw new Error("Unexpected persistence request");
    calls.push(input);
    const values = JSON.parse(saved);
    const outcome = applyMockPlayerAccessMutation(schema.properties.banned_player_ids, values.banned_player_ids, input, true);
    saved = JSON.stringify({ ...values, banned_player_ids: outcome.value });
    setDetails({ ...instance, settings_json: saved });
    return { instanceId: input.instanceId, fieldKey: input.fieldKey, operation: input.operation,
      persistentStatus: outcome.changed ? "updated" : "unchanged", liveStatus: outcome.liveStatus,
      verificationStatus: outcome.verificationStatus };
  } });
  const field = access.fields.find((entry) => entry.key === "banned_player_ids");
  if (!field) throw new Error("Native account hash roster is missing from the player workspace");
  return <main style={{ maxWidth: "920px", margin: "32px auto" }}>
    <h1>{field.title}</h1><p>{field.description}</p>
    <PlayerAccessRosterList field={field} locale={locale} selectedEntryKey={null} onSelect={() => {}} disabled={access.disabled} />
    <PlayerAccessRosterEditor field={field} locale={locale} selectedEntry={null}
      disabled={access.disabled} busy={access.busyFieldKey !== null} onMutate={access.onMutate} />
    {access.feedback}
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
async function click(selector: string) {
  const button = element<HTMLButtonElement>(selector);
  check(!button.disabled && button.getClientRects().length > 0, `Enabled visible control: ${selector}`);
  await act(async () => { button.click(); });
}
async function input(value: string) {
  const field = element<HTMLInputElement>(".player-access-roster-input");
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
  if (!setter) throw new Error("Input setter is unavailable");
  await act(async () => { setter.call(field, value); field.dispatchEvent(new Event("input", { bubbles: true })); });
}
async function submit(value: string) {
  await input(value);
  await click(".player-access-roster-add-button");
  await click(".inline-confirm-submit");
}
function currentEntries() {
  return Array.from(fixture?.querySelectorAll(".player-access-roster-entry-value") ?? [], (entry) => entry.textContent);
}
async function draw(key: string) {
  await act(async () => { root.render(<I18nProvider key={key}><Fixture /></I18nProvider>); });
  const deadline = performance.now() + 5000;
  while (!fixture?.querySelector("h1")) {
    if (performance.now() >= deadline) throw new Error(`Roster did not finish loading: ${fixture?.textContent}`);
    await act(async () => { await new Promise<void>((resolve) => requestAnimationFrame(() => resolve())); });
  }
}
async function run() {
  await act(prepareBrowserLocaleCatalogs);
  await draw("english");
  check(element("h1").textContent === "Banned Native Account Hashes", "English title describes the native account hash");
  check(JSON.stringify(currentEntries()) === JSON.stringify(["76561198000000001", "00042"]), "Existing text is read without Steam conversion or precision loss");
  check(element<HTMLButtonElement>(".player-access-roster-add-button").disabled, "An empty input cannot submit");
  for (const invalid of ["18446744073709551616", "1e3", "-1", "+1", "1.5", "1,invalid", "１８"]) {
    await submit(invalid);
    check(calls.length === 0, `Invalid hash never crosses persistence boundary: ${invalid}`);
    check(fixture?.textContent?.includes("Enter a decimal account hash from 0 to 18446744073709551615"), "Validation explains the complete native range");
    check(element<HTMLInputElement>(".player-access-roster-input").value === invalid, "Invalid input stays available for correction");
  }
  await submit(maximum);
  check(calls.length === 1 && calls[0]?.value === maximum, "The maximum u64 reaches persistence as an exact string");
  check(JSON.parse(saved).banned_player_ids === `76561198000000001\n00042\n${maximum}`, "Saving preserves existing strings and the complete new hash");
  check(fixture?.textContent?.includes("applies on the next server start") && !fixture?.textContent?.includes("applied live"), "A restart roster never claims verified live enforcement");
  await submit("0");
  check(calls.length === 2 && calls[1]?.value === "0", "Zero is a valid string hash");
  await draw("reloaded");
  check(JSON.stringify(currentEntries()) === JSON.stringify(["76561198000000001", "00042", maximum, "0"]), "Remount reads every saved hash without rounding");
  localStorage.setItem("langame.locale", "zh-CN");
  await draw("chinese");
  check(element("h1").textContent === "封禁原生账户哈希", "Chinese title describes the native account hash");
  await submit("18446744073709551616");
  check(calls.length === 2 && fixture?.textContent?.includes("请输入 0 至 18446744073709551615 范围内的十进制账户哈希"), "Chinese validation rejects overflow before persistence");
  await act(async () => { root.unmount(); });
  check(errors.length === 0, "Browser and React report no errors");
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false });
  return { status: "passed", checks, browser_errors: errors };
}
void run().catch((error) => ({ status: "failed", error: String(error?.stack ?? error), checks, browser_errors: errors }))
  .then((result) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(result) }));
