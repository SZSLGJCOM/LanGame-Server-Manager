import React, { act, StrictMode, useState } from "react";
import { createRoot } from "react-dom/client";
import { readModuleDetails } from "../../src/api";
import { I18nProvider } from "../../src/i18n";
import { SelectedPlayerRosterActions } from "../../src/views/servers/player-center/SelectedPlayerRosterActions";
import { PlayerAccessRosterList } from "../../src/views/servers/player-center/PlayerAccessRosterList";
import { rosterEntryPlayer } from "../../src/views/servers/player-center/player-center-selection";
import { usePlayerAccess } from "../../src/views/servers/player-center/use-player-access";
import type { RosterField } from "../../src/views/servers/player-center/player-access-roster-model";
import type { InstanceDetails, RuntimeLivePlayerEntry } from "../../src/types";
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
window.confirm = () => { throw new Error("Player actions must not open a browser confirmation"); };
const nonce = new URLSearchParams(location.search).get("nonce");
const mutations: { field: string; operation: string; value: unknown }[] = [];
let checks = 0;
const fields: RosterField[] = [{
  key: "blocklist", title: "黑名单", description: null, lane: "block", kind: "string-list",
  property: { type: "array", items: { type: "string" } }, currentValue: ["76561198000000001"],
  entries: [{ key: "first", label: "76561198000000001", rawValue: "76561198000000001" }], sortWeight: 0
}, {
  key: "admins", title: "管理员", description: null, lane: "admin", kind: "object-list",
  property: { type: "array", items: { type: "object", properties: { name: { type: "string", title: "玩家名称" } } } },
  currentValue: [], entries: [], sortWeight: 1
}, {
  key: "admin_permissions", title: "管理员权限", description: null, lane: "admin", kind: "object-list",
  property: { type: "array", "x-lsgm-player-access-codec": "object_identity", "x-lsgm-player-access-identity-field": "name",
    items: { type: "object", required: ["name", "permission"], properties: {
      name: { type: "string", title: "玩家名称" }, permission: { type: "integer", title: "权限级别", default: 1000 }
    } } }, currentValue: [{ name: "Offline", permission: 1000, source: "imported" }],
  entries: [{ key: "Offline", label: "Offline", rawValue: { name: "Offline", permission: 1000, source: "imported" } }], sortWeight: 2
}];

function check(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}
function select<ElementType extends Element>(selector: string): ElementType {
  const element = document.querySelector<ElementType>(selector);
  check(element, `Missing element: ${selector}`);
  return element;
}
const root = createRoot(select<HTMLDivElement>("#fixture"));
const pause = () => new Promise<void>((resolve) => setTimeout(resolve, 10));
const instance: InstanceDetails = {
  summary: { id: "roster-fixture", module_id: "roster-fixture", name: "Roster fixture", status: "Stopped",
    active_process_count: 0, autostart: false, bind_ip: "127.0.0.1" },
  ports: [], settings_json: "{}", config_file_path: "", saves_path: "", backup_uses_declared_saves_path: false,
  auto_backup_on_stop: false, backup_retention_count: 3
};
function Fixture(props: { selectedPlayer?: RuntimeLivePlayerEntry }) {
  const [fieldKey, setFieldKey] = useState("blocklist");
  const [entryKey, setEntryKey] = useState<string | null>(null);
  const [createFieldKey, setCreateFieldKey] = useState<string | undefined>("blocklist");
  const [editorVersion, setEditorVersion] = useState(0);
  const access = usePlayerAccess({ details: instance, moduleDetails: null, onApplyPlayerAccessMutation: async (mutation) => {
    mutations.push({ field: mutation.fieldKey, operation: mutation.operation, value: mutation.value });
    return { instanceId: mutation.instanceId, fieldKey: mutation.fieldKey, operation: mutation.operation,
      persistentStatus: "updated", liveStatus: "not_running", verificationStatus: "unavailable" };
  } });
  const field = fields.find((candidate) => candidate.key === fieldKey)!;
  const entry = field.entries.find((candidate) => candidate.key === entryKey) ?? null;
  const selectedRoster = entry ? { field, entry } : null;
  function create() { setEntryKey(null); setCreateFieldKey(fieldKey); setEditorVersion((value) => value + 1); }
  return <main style={{ width: "min(960px, calc(100vw - 64px))", margin: "48px auto" }}>
    <nav>{fields.map((item) => <button key={item.key} type="button" data-field={item.key}
      onClick={() => { setFieldKey(item.key); setEntryKey(null); setCreateFieldKey(undefined); }}>{item.title}</button>)}</nav>
    <PlayerAccessRosterList field={field} disabled={false} locale="zh-CN" selectedEntryKey={entryKey}
      onSelect={(selected) => { setEntryKey(selected.key); setCreateFieldKey(undefined); }} />
    <button type="button" className="fixture-create" onClick={create}>新增</button>
    <div className="player-center-controls">
      {entry ? <div className="player-center-member-heading"><strong className="player-access-roster-entry-value">{entry.label}</strong></div> : null}
      <SelectedPlayerRosterActions key={`${field.key}:${entryKey}:${editorVersion}`} fields={fields}
        busyFieldKey={null} disabled={false} locale="zh-CN" selectedRoster={selectedRoster}
        createFieldKey={createFieldKey} selectedPlayer={props.selectedPlayer ?? rosterEntryPlayer(selectedRoster)}
        onClearSelection={() => { setEntryKey(null); setCreateFieldKey(undefined); }} onMutate={access.onMutate} />
      {access.feedback}
    </div>
  </main>;
}
async function render(instance = "instance-one", selectedPlayer?: RuntimeLivePlayerEntry) {
  await act(async () => {
    root.render(<StrictMode><I18nProvider>
      <Fixture key={instance} selectedPlayer={selectedPlayer} />
    </I18nProvider></StrictMode>);
  });
  const deadline = performance.now() + 5000;
  while (!document.querySelector(".player-access-selected-actions")) {
    check(performance.now() < deadline, "Roster did not mount");
    await act(pause);
  }
  await act(async () => {
    // The provider also preloads the fallback catalog after mounting its children.
    await Promise.all([import("../../src/i18n-messages-zh-cn"), import("../../src/i18n-messages")]);
  });
}
async function click(element: HTMLElement) { await act(async () => { element.click(); }); }
async function input(selector: string, value: string) {
  await act(async () => {
    const element = select<HTMLInputElement>(selector);
    element.focus();
    const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
    check(setter, "Input setter is missing");
    setter.call(element, value);
    element.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
async function key(name: "Enter" | "Escape") {
  await act(async () => {
    const response = await fetch(`/__reliability_key/${nonce}/${name}`, { method: "POST" });
    check(response.ok, `Native ${name} dispatch failed`);
  });
}
function review() { return document.querySelector<HTMLElement>(".inline-confirm-review"); }
function submit() { return select<HTMLButtonElement>(".inline-confirm-submit"); }
function cancel() { return select<HTMLButtonElement>(".inline-confirm-buttons button:first-child"); }
function remove(field: string) {
  return select<HTMLButtonElement>(`[data-roster-field="${field}"] [data-roster-operation="remove"] button`);
}
function verifyLayout() {
  const group = review();
  check(group, "Inline review is missing");
  const panel = select<HTMLElement>(".player-center-controls").getBoundingClientRect();
  const box = group.getBoundingClientRect();
  check(box.width >= 150 && box.height >= 50, `Confirmation controls lack usable space: ${box.width} x ${box.height}, check ${checks}`);
  check(box.left >= panel.left && box.right <= panel.right + 1, "Confirmation escaped its roster panel");
  for (const button of group.querySelectorAll("button")) {
    const rect = button.getBoundingClientRect();
    check(rect.left >= box.left && rect.right <= box.right + 1, "A confirmation button overflowed the review");
    check(button.scrollWidth <= button.clientWidth + 1, "Confirmation button text was clipped");
  }
  const message = select<HTMLElement>(".inline-confirm-message");
  check(message.scrollWidth <= message.clientWidth + 1 && message.scrollHeight <= message.clientHeight + 1,
    "Confirmation text was clipped");
  check(document.documentElement.scrollWidth <= innerWidth, "Roster review caused horizontal page overflow");
}

async function run() {
  await render();
  await input("#player-access-roster-blocklist", " 76561198000000002 ");
  await key("Enter");
  check(review()?.textContent?.includes("76561198000000002"), "Native Enter must open review for the entered player");
  check(mutations.length === 0, "Native Enter bypassed roster review");
  verifyLayout();
  checks++;

  await click(cancel());
  check(!review() && mutations.length === 0, "Cancel must not mutate the roster");
  select<HTMLInputElement>("#player-access-roster-blocklist").focus();
  await key("Enter");
  await key("Escape");
  check(!review() && mutations.length === 0, "Escape must not mutate the roster");
  checks++;

  await click(select<HTMLButtonElement>(".player-access-roster-add-button"));
  await input("#player-access-roster-blocklist", "76561198000000003");
  check(!review(), "Changing the target must dismiss its existing review");
  await key("Enter");
  await click(submit());
  check(mutations.length === 1 && mutations[0].field === "blocklist"
    && mutations[0].operation === "add" && mutations[0].value === "76561198000000003", "Only the reviewed target may be added");
  checks++;

  await click(select<HTMLButtonElement>(".player-access-roster-select"));
  check(!document.querySelector(".player-center-controls .player-access-roster-entries"), "Selected controls contain a duplicate roster");
  await click(remove("blocklist"));
  check(review()?.textContent?.includes("76561198000000001"), "Removal review must show the selected entry");
  verifyLayout();
  await click(cancel());
  check(mutations.length === 1, "Cancelling removal must preserve the roster");
  await click(remove("blocklist"));
  await click(submit());
  check(mutations.length === 2 && mutations[1].operation === "remove"
    && mutations[1].value === "76561198000000001", "Removal must use the reviewed raw entry");
  checks++;

  await click(select<HTMLButtonElement>('[data-field="admins"]'));
  await click(select<HTMLButtonElement>(".fixture-create"));
  await input("#player-access-roster-admins-name", "Alice");
  await key("Enter");
  check(review()?.textContent?.includes("Alice") && mutations.length === 2, "Object form Enter must review without writing");
  verifyLayout();
  await input("#player-access-roster-admins-name", "Bob");
  check(!review(), "Object draft edits must dismiss stale confirmation");
  checks++;

  await key("Enter");
  await click(submit());
  check(mutations.length === 3 && mutations[2].field === "admins"
    && JSON.stringify(mutations[2].value) === '{"name":"Bob"}', "Object confirmation must submit the reviewed draft");
  checks++;

  await click(select<HTMLButtonElement>(".fixture-create"));
  await input("#player-access-roster-admins-name", "Carol");
  await key("Enter");
  await render("instance-two");
  check(!review() && mutations.length === 3, "Changing instances must cancel pending roster review");
  checks++;

  select<HTMLElement>("main").style.width = "530px";
  await click(select<HTMLButtonElement>(".player-access-roster-select"));
  await click(remove("blocklist"));
  verifyLayout();
  const identity = select<HTMLElement>(".player-center-member-heading .player-access-roster-entry-value").getBoundingClientRect();
  check(identity.width >= 120 && identity.bottom <= review()!.getBoundingClientRect().top,
    "Narrow review must preserve the visible player identity above its controls");
  checks++;

  await click(cancel());
  await click(select<HTMLButtonElement>(".fixture-create"));
  await input("#player-access-roster-blocklist", "76561198000000002");
  await key("Enter");
  verifyLayout();
  const inputBox = select<HTMLElement>("#player-access-roster-blocklist").getBoundingClientRect();
  check(inputBox.width >= 120 && inputBox.bottom <= review()!.getBoundingClientRect().top,
    "Narrow review must preserve the input above its controls");
  checks++;

  await click(cancel());
  const selectedPlayer: RuntimeLivePlayerEntry = {
    player_key: "selected-bob", display_name: "Bob Display", identifiers: [{ kind: "player_name", value: "Bob", stable: false }],
    available_action_ids: [], ping_ms: null, role: null, session_started_at_unix_ms: null, attributes: []
  };
  await render("instance-three");
  await click(select<HTMLButtonElement>('[data-field="admin_permissions"]'));
  await click(select<HTMLButtonElement>(".fixture-create"));
  await input("#player-access-roster-admin_permissions-name", "Manual name");
  await input("#player-access-roster-admin_permissions-permission", "500");
  await render("instance-three", selectedPlayer);
  check(select<HTMLInputElement>("#player-access-roster-admin_permissions-name").value === "Manual name",
    "Selecting a player silently overwrote the object draft");
  await click(select<HTMLButtonElement>(".player-access-roster-selected-fill"));
  check(select<HTMLInputElement>("#player-access-roster-admin_permissions-name").value === "Bob"
    && select<HTMLInputElement>("#player-access-roster-admin_permissions-permission").value === "500",
    "Explicit identity fill used display_name or replaced hand-edited permissions");
  check(mutations.length === 3 && !review(), "Filling an identity sent a mutation or skipped full entry review");
  checks++;

  await click(select<HTMLButtonElement>(".player-access-roster-action-pair .inline-confirm-action button"));
  check(mutations.length === 3, "Object identity fill bypassed final confirmation");
  await click(submit());
  check(mutations.length === 4 && JSON.stringify(mutations[3].value) === '{"name":"Bob","permission":500}',
    "Object save failed to preserve the explicitly reviewed full draft");
  checks++;

  await click(select<HTMLButtonElement>(".player-access-roster-select"));
  await click(select<HTMLButtonElement>(".player-access-edit-selected"));
  check(select<HTMLInputElement>("#player-access-roster-admin_permissions-name").disabled,
    "Editing an existing entry permits silent identity replacement");
  await input("#player-access-roster-admin_permissions-permission", "250");
  await key("Enter");
  check(review()?.textContent?.includes("Offline") && mutations.length === 4, "Offline object update skipped confirmation");
  await click(submit());
  check(mutations.length === 5 && JSON.stringify(mutations[4].value) === '{"name":"Offline","permission":250,"source":"imported"}',
    "Selected object edit lost identity or uneditable stored metadata");
  checks++;

  await click(remove("admin_permissions"));
  check(review()?.textContent?.includes("Offline"), "Offline removal did not identify the stored target");
  await click(submit());
  check(mutations.length === 6 && mutations[5].operation === "remove"
    && JSON.stringify(mutations[5].value) === '{"name":"Offline","permission":1000,"source":"imported"}',
    "Offline removal did not submit the exact stored entry");
  check(errors.length === 0, `Browser errors: ${errors.join("; ")}`);
  checks++;

  const humanitz = JSON.parse((await readModuleDetails("humanitz")).schema_json);
  const historicalId = "76561198000000009";
  fields.push({
    key: "admin_steam_ids", title: "HumanitZ 管理员 NetID", description: null, lane: "admin", kind: "string-lines",
    property: humanitz.properties.admin_steam_ids, currentValue: historicalId,
    entries: [{ key: historicalId, label: historicalId, rawValue: historicalId }], sortWeight: 3
  });
  await render("humanitz-netid");
  await click(select<HTMLButtonElement>('[data-field="admin_steam_ids"]'));
  const productId = "0123456789abcdef0123456789abcdef";
  const epicId = "ABCDEF0123456789".repeat(2);
  for (const netId of [`${epicId}|${productId}`, `|${productId}`]) {
    await click(select<HTMLButtonElement>(".fixture-create"));
    await input("#player-access-roster-admin_steam_ids", netId);
    const before = mutations.length;
    await key("Enter");
    check(review()?.textContent?.includes(netId), "HumanitZ must review the complete NetID including its separator");
    verifyLayout();
    check(mutations.length === before, "HumanitZ NetID bypassed explicit review");
    await click(submit());
    check(mutations.length === before + 1 && mutations.at(-1)?.value === netId,
      "HumanitZ NetID must reach the save boundary without truncation or conversion");
    checks++;
  }
  await click(select<HTMLButtonElement>(".fixture-create"));
  await input("#player-access-roster-admin_steam_ids", historicalId);
  const beforeInvalid = mutations.length;
  await key("Enter");
  await click(submit());
  check(mutations.length === beforeInvalid && document.body.textContent?.includes("请输入完整 NetID"),
    "A new Steam64 value must not reach persistence and must show the NetID format error");
  checks++;
  await click(select<HTMLButtonElement>(".player-access-roster-select"));
  await click(remove("admin_steam_ids"));
  check(review()?.textContent?.includes(historicalId), "An existing historical entry must remain explicitly removable");
  await click(submit());
  check(mutations.length === beforeInvalid + 1 && mutations.at(-1)?.operation === "remove"
    && mutations.at(-1)?.value === historicalId, "Removing a historical entry must preserve its original identity");
  check(errors.length === 0, `Browser errors: ${errors.join("; ")}`);
  checks++;
  await click(select<HTMLButtonElement>(".fixture-create"));
  await input("#player-access-roster-admin_steam_ids", `${epicId}|${productId}`);
  await key("Enter");
  verifyLayout();
  return { status: "passed", checks, browser_errors: errors };
}

let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => {
  watchdog = setTimeout(() => reject(new Error(`Roster interaction stalled after ${checks} checks`)), 20_000);
})]).finally(() => clearTimeout(watchdog))
  .catch((error) => ({ status: "failed", checks, error: String(error), browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
