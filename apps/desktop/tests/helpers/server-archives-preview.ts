import { act } from "react";
import type { InstanceArchiveDetails } from "../../src/storage-management-types";

interface PreviewCall {
  command: string;
  args: { input?: { archive_id?: string } };
  operation: { promise: Promise<unknown>; resolve: (value: unknown) => void; reject: (error: Error) => void };
}

interface NormalIdentity {
  selectedInstanceId: string | null;
  detailsId: string | null;
  moduleId: string | null;
  settingsJson: string | null;
}

function normalizeReactIds(html: string) {
  const tree = document.createElement("div"); tree.innerHTML = html;
  const ids = new Map<string, string>();
  for (const node of tree.querySelectorAll("[id],[for],[aria-controls],[aria-describedby],[aria-labelledby]")) {
    for (const attribute of ["id", "for", "aria-controls", "aria-describedby", "aria-labelledby"]) {
      const value = node.getAttribute(attribute);
      if (value === null) continue;
      node.setAttribute(attribute, value.replace(/_r_[0-9a-z]+_|:[rR][0-9a-z]+:/g, (id) => {
        if (!ids.has(id)) ids.set(id, `react-generated-id-${ids.size}`);
        return ids.get(id)!;
      }));
    }
  }
  return tree.innerHTML;
}

export function captureNormalWorkspace(fixture: HTMLElement, identity: NormalIdentity) {
  const panel = fixture.querySelector<HTMLElement>(".server-detail-panel")!;
  const selectedTab = panel.querySelector<HTMLElement>('.server-detail-tabs [role="tab"][aria-selected="true"]');
  const tabBounds = panel.querySelector<HTMLElement>(".server-detail-subheader")?.getBoundingClientRect();
  const panelBounds = panel.getBoundingClientRect();
  const html = panel.innerHTML;
  return { html, normalizedHtml: normalizeReactIds(html), identity,
    tabLayout: tabBounds ? { top: tabBounds.top - panelBounds.top, left: tabBounds.left - panelBounds.left,
      width: tabBounds.width, height: tabBounds.height } : null,
    tabs: [...panel.querySelectorAll<HTMLElement>('.server-detail-tabs [role="tab"]')].map((tab) => ({
      key: tab.id.split("-").at(-1), label: tab.querySelector(".server-detail-tab-label")?.textContent,
      icon: tab.querySelector(".server-detail-tab-icon")?.innerHTML })),
    selectedTab: selectedTab ? { key: selectedTab.id.split("-").at(-1), text: selectedTab.textContent, disabled: selectedTab.getAttribute("aria-disabled") } : null,
    values: [...panel.querySelectorAll<HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement | HTMLOutputElement>("input,textarea,select,output")]
      .map((control) => ({ tag: control.tagName, name: control.getAttribute("name"), label: control.getAttribute("aria-label"), value: control.value,
        checked: "checked" in control ? control.checked : null, disabled: "disabled" in control ? control.disabled : null,
        readOnly: "readOnly" in control ? control.readOnly : null })),
    actions: [...panel.querySelectorAll<HTMLButtonElement>("button")].map((button) => ({ text: button.textContent,
      label: button.getAttribute("aria-label"), title: button.title, disabled: button.disabled })) };
}

function firstDifference(before: string, after: string) {
  let index = 0;
  while (index < before.length && index < after.length && before[index] === after[index]) index++;
  return { index, before: before.slice(Math.max(0, index - 100), index + 180), after: after.slice(Math.max(0, index - 100), index + 180) };
}

export function assertNormalWorkspaceRestored(fixture: HTMLElement, check: Hooks["check"],
  before: ReturnType<typeof captureNormalWorkspace>, identity: NormalIdentity) {
  const after = captureNormalWorkspace(fixture, identity);
  check(JSON.stringify(after.identity) === JSON.stringify(before.identity), "Normal workspace retains the original instance identity and saved settings source");
  check(JSON.stringify(after.selectedTab) === JSON.stringify(before.selectedTab), "Normal workspace restores the original selected tab");
  check(JSON.stringify(after.values) === JSON.stringify(before.values), "Normal workspace restores every saved form value and control state");
  check(JSON.stringify(after.actions) === JSON.stringify(before.actions), "Normal workspace restores its original action labels and enabled states");
  check(after.normalizedHtml === before.normalizedHtml,
    "Normal workspace restores all content and ID relationships; only React-generated IDs may change", () => ({
      raw: firstDifference(before.html, after.html), normalized: firstDifference(before.normalizedHtml, after.normalizedHtml) }));
  check(!fixture.querySelector(".archived-instance-workspace,.archive-configuration-preview"), "The normal workspace does not retain an archived workspace");
}

interface Hooks {
  fixture: HTMLElement;
  check: (condition: unknown, description: string, diagnostics?: () => unknown) => asserts condition;
  click: (selector: string) => Promise<void>;
  setInput: (selector: string, value: string) => Promise<void>;
  selectArchive: (id: string) => Promise<void>;
  changeMode: (mode: "normal" | "archived") => Promise<void>;
  waitFor: (selector: string) => Promise<void>;
  resolve: (call: PreviewCall, result: unknown) => Promise<void>;
  reject: (call: PreviewCall, message: string) => Promise<void>;
  last: (command: string) => PreviewCall;
  calls: PreviewCall[];
  moduleCalls: { moduleId?: string; module_id?: string; includePreservedProgramCounts?: boolean; include_preserved_program_counts?: boolean }[];
  configuration: (id: string) => InstanceArchiveDetails;
  setManualReads: (value: boolean) => void;
  setManualModuleReads: (value: boolean) => void;
  resolveModuleRead: () => Promise<void>;
  failNextModuleRead: () => void;
  originalModuleId: string;
  runWorkspaceAssertions: () => Promise<void>;
  normalSnapshot: ReturnType<typeof captureNormalWorkspace>;
  normalIdentity: () => NormalIdentity;
  saves: () => number;
  starts: () => number;
  phases: string[];
}

const command = "read_instance_archive_details";
const workspace = (id: string) => `.archived-instance-workspace[data-archive-id="${id}"]`;
const preview = (id: string) => `${workspace(id)} [data-archive-workspace-tab="settings"]`;
const state = (id: string, value: string) => `${workspace(id)}[data-archive-configuration-state="${value}"]`;

export async function selectArchiveSettings(hooks: Pick<Hooks, "click" | "waitFor">, id: string) {
  await hooks.click(`${workspace(id)} .server-detail-tab[id$="-settings"]`);
  await hooks.waitFor(`${workspace(id)} [data-archive-workspace-tab="settings"]`);
  await hooks.waitFor(preview(id));
}

export async function selectArchivePreviewSection(hooks: Pick<Hooks, "fixture" | "click" | "waitFor">, id: string, sectionId: string) {
  const selector = `${preview(id)} [data-configuration-section-id="${sectionId}"] > .configuration-section-navigation__button`;
  await hooks.waitFor(selector);
  const toggle = hooks.fixture.querySelector<HTMLButtonElement>(`${preview(id)} .configuration-workspace__navigation-toggle`);
  if (toggle && toggle.getBoundingClientRect().width > 0 && toggle.getAttribute("aria-expanded") !== "true") {
    await hooks.click(`${preview(id)} .configuration-workspace__navigation-toggle`);
  }
  await hooks.click(selector);
}

export async function runArchivePreviewAssertions(hooks: Hooks) {
  const { fixture, check, phases } = hooks;
  function element<T extends HTMLElement = HTMLElement>(selector: string): T {
    const found = fixture.querySelector<T>(selector);
    check(found, `Preview element is present: ${selector}`);
    return found;
  }
  const count = () => hooks.calls.filter((call) => call.command === command).length;
  const field = (key: string) => ["input", "textarea", "select"].map((control) => `${preview("a")} [data-field-key="${key}"] .configuration-field-control ${control}`).join(",");
  const value = (key: string) => element<HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement>(field(key)).value;
  const savedBefore = hooks.saves(), startedBefore = hooks.starts();
  check(!fixture.querySelector(".archived-instance-workspace[data-archive-id]")
    && element(".server-detail-panel").textContent?.includes("Select an archived instance to view its retained details."),
    "Archives mode starts with retained-detail selection guidance instead of another normal server's workspace");
  hooks.setManualReads(true);
  await hooks.selectArchive("a");
  check(Boolean(element(state("a", "loading")).querySelector('.server-detail-tabs [role="tab"][id$="-runtime"][aria-selected="true"]'))
    && element(state("a", "loading")).getAttribute("aria-label") === "Alpha world"
    && element('.server-list-card[data-archive-id="a"]').classList.contains("is-active")
    && !element(state("a", "loading")).querySelector(".archive-configuration-preview__header")
    && !element(".server-detail-panel").textContent?.includes("Normal world"),
    "Selecting an archive opens its first Runtime tab while loading without an extra archive heading or another server's data");
  check(hooks.last(command).args.input?.archive_id === "a", "Archive configuration reads pass an opaque archive ID instead of a path");
  phases.push("loading");
  hooks.setManualModuleReads(true);
  await hooks.resolve(hooks.last(command), hooks.configuration("a"));
  await hooks.waitFor(state("a", "ready"));
  await selectArchiveSettings(hooks, "a");
  const pendingRaw = `${preview("a")} .configuration-workspace__raw`;
  check(!element<HTMLDetailsElement>(pendingRaw).open
    && element(preview("a")).querySelector('[data-configuration-state="loading"]'),
    "Loading module descriptions never automatically reveals archived raw JSON");
  await hooks.click(`${pendingRaw} > summary`);
  check(element<HTMLDetailsElement>(pendingRaw).open
    && element<HTMLTextAreaElement>(`${preview("a")} .configuration-workspace__raw-json`).readOnly,
    "A user may explicitly reveal the read-only original JSON while module descriptions load");
  await hooks.click(`${pendingRaw} > summary`);
  hooks.setManualModuleReads(false); await hooks.resolveModuleRead();
  phases.push("module loading disclosure");
  await selectArchivePreviewSection(hooks, "a", "saved_settings");
  check(value("archive_marker") === "ARCHIVED_CONFIG_a"
    && value("archived_false") === "false" && value("archived_zero") === "0"
    && value("archived_null") === "null" && value("archived_empty") === "",
    "Archive fields preserve saved values including false, zero, null, empty strings and unknown keys");
  const marker = element<HTMLInputElement>(field("archive_marker"));
  await act(async () => { marker.focus(); });
  check(marker.readOnly && document.activeElement === marker, "Saved read-only values remain reachable by keyboard");
  const raw = element<HTMLTextAreaElement>(`${preview("a")} .configuration-workspace__raw-json`);
  check(raw.readOnly && raw.value === hooks.configuration("a").instance.settings_json,
    "The original archived JSON remains available as read-only text without adding module defaults");
  check(JSON.parse(raw.value).archived_empty === "", "Archived empty strings remain distinct from absent values");
  await selectArchivePreviewSection(hooks, "a", "network");
  check(element<HTMLSelectElement>(`${preview("a")} .instance-connection-settings__listener-row select`).value === "192.168.1.42"
    && element<HTMLInputElement>(`${preview("a")} .instance-port-fields__input`).value === "25565",
    "Archived configuration shows its saved bind address and port without registering them");
  check([...element(`${preview("a")} .configuration-workspace__main`).querySelectorAll<HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement>("input,textarea,select")]
    .filter((control) => !control.matches(".player-join-address select"))
    .every((control) => control.disabled || ("readOnly" in control && control.readOnly)), "Archive configuration exposes no editable form controls");
  check(!element(preview("a")).querySelector("[contenteditable=true]")
    && [...element(preview("a")).querySelectorAll<HTMLButtonElement>('button[type="submit"]')].every((button) => button.disabled)
    && hooks.saves() === savedBefore && hooks.starts() === startedBefore, "Read-only preview exposes no Save or Start action and submits no write callback");
  check(hooks.moduleCalls.some((args) => args.moduleId === "minecraft" && args.module_id === "minecraft"
    && args.includePreservedProgramCounts === false && args.include_preserved_program_counts === false),
    "Preview reads the archive's own module definition without preserved-program inventory counts");
  if (hooks.originalModuleId !== "minecraft") check(hooks.moduleCalls.at(-1)?.moduleId !== hooks.originalModuleId,
    "Archive preview does not reuse the previously selected normal instance's module");
  phases.push("read-only saved values");

  const secret = element<HTMLInputElement>(`${preview("a")} [data-field-key="rcon_password"] input`);
  const secretToggle = `${preview("a")} [data-field-key="rcon_password"] .configuration-secret-toggle`;
  check(secret.type === "password" && secret.value === JSON.parse(hooks.configuration("a").instance.settings_json).rcon_password
    && !element<HTMLButtonElement>(secretToggle).disabled, "Saved secrets use the normal masked input with a local disclosure action");
  await hooks.click(secretToggle);
  check(secret.type === "text" && secret.readOnly, "Revealing an archived secret changes only its local presentation");
  await hooks.click(secretToggle);
  check(secret.type === "password", "Saved archive secrets can be concealed again");
  await selectArchivePreviewSection(hooks, "a", "saved_settings");
  const collisionKeys = ["foo_bar", "foo-bar", "测试甲", "测试乙"];
  const collisionIds = collisionKeys.map((key) => element(field(key)).id);
  check(new Set(collisionIds).size === collisionKeys.length && collisionIds.every(Boolean)
    && collisionKeys.every((key) => {
      const row = element(`${preview("a")} [data-field-key="${key}"]`);
      return row.querySelector<HTMLLabelElement>("label")?.htmlFor === element(field(key)).id;
    }), "Unknown archived keys retain unique output IDs and matching labels even when ordinary sanitization would collide");
  for (const key of ["foo-bar", "测试乙"]) {
    const navigationToggle = fixture.querySelector<HTMLButtonElement>(`${preview("a")} .configuration-workspace__navigation-toggle`);
    if (navigationToggle && navigationToggle.getBoundingClientRect().width > 0 && navigationToggle.getAttribute("aria-expanded") !== "true") {
      await hooks.click(`${preview("a")} .configuration-workspace__navigation-toggle`);
    }
    await hooks.setInput(`${preview("a")} .configuration-search input`, key);
    await hooks.waitFor(`${preview("a")} .configuration-search-results button`);
    const results = [...element(preview("a")).querySelectorAll<HTMLButtonElement>(".configuration-search-results button")];
    const index = results.findIndex((button) => button.querySelector("strong")?.textContent === key);
    check(index >= 0, `Searching an unknown archived key returns its exact field: ${key}`);
    await hooks.click(`${preview("a")} .configuration-search-results li:nth-child(${index + 1}) button`);
    check(document.activeElement === element(field(key)), `Searching a colliding key focuses its own saved output: ${key}`);
  }
  phases.push("unique unknown field navigation");
  await hooks.runWorkspaceAssertions();

  const beforeUnavailable = count();
  await hooks.selectArchive("c"); await hooks.waitFor(state("c", "unavailable"));
  await hooks.selectArchive("d"); await hooks.waitFor(state("d", "unavailable"));
  check(count() === beforeUnavailable && !fixture.querySelector(".configuration-workspace__raw-json"),
    "Archives without an archived state show unavailable guidance and never read configuration");
  phases.push("unavailable states");
  await hooks.selectArchive("e"); await hooks.resolve(hooks.last(command), hooks.configuration("e"));
  await hooks.waitFor(state("e", "ready"));
  await selectArchiveSettings(hooks, "e");
  check(element<HTMLButtonElement>('.server-list-card[data-archive-id="e"] .server-list-card-primary-action').disabled
    && element<HTMLTextAreaElement>(`${preview("e")} .configuration-workspace__raw-json`).value.includes("ARCHIVED_CONFIG_e"),
    "An unavailable required program does not prevent reading the safely archived configuration");
  phases.push("blocked recovery preview");

  await hooks.selectArchive("b"); await hooks.reject(hooks.last(command), "Archived settings unavailable CONFIG_PREVIEW_FAILURE_END");
  await hooks.waitFor(state("b", "error"));
  check(element(".shell-activity-bar").textContent?.includes("CONFIG_PREVIEW_FAILURE_END")
    && !element(".server-detail-panel").textContent?.includes("CONFIG_PREVIEW_FAILURE_END")
    && !fixture.querySelector(".configuration-workspace__raw-json"),
    "Configuration read failure exposes its cause in the activity bar without showing another archive's settings");
  const beforeRetry = count(); await hooks.click(`${workspace("b")} .archived-instance-workspace__retry`);
  check(count() === beforeRetry + 1 && hooks.last(command).args.input?.archive_id === "b", "Preview retry reads only the selected archive again");
  await hooks.resolve(hooks.last(command), hooks.configuration("b")); await hooks.waitFor(state("b", "ready"));
  await selectArchiveSettings(hooks, "b");
  check(element<HTMLButtonElement>('.server-list-card[data-archive-id="b"] .server-list-card-primary-action').disabled
    && element<HTMLTextAreaElement>(`${preview("b")} .configuration-workspace__raw-json`).value.includes("ARCHIVED_CONFIG_b"),
    "A conflicting archived port blocks Restore while its configuration remains safely previewable");
  phases.push("error retry");

  hooks.failNextModuleRead(); await hooks.selectArchive("a");
  await hooks.resolve(hooks.last(command), hooks.configuration("a")); await hooks.waitFor(state("a", "ready"));
  await selectArchiveSettings(hooks, "a");
  await hooks.waitFor(`${preview("a")} .configuration-load-error__retry`);
  check(element<HTMLTextAreaElement>(`${preview("a")} .configuration-workspace__raw-json`).value === hooks.configuration("a").instance.settings_json
    && element<HTMLDetailsElement>(`${preview("a")} .configuration-workspace__raw`).open,
    "Module-definition failure preserves the original archived JSON as the configuration fallback");
  phases.push("module fallback");

  await hooks.selectArchive("b"); const stale = hooks.last(command), beforeRapid = count();
  await hooks.selectArchive("a"); await hooks.selectArchive("e");
  check(count() === beforeRapid && Boolean(fixture.querySelector(state("e", "loading")))
    && element(state("e", "loading")).getAttribute("aria-label") === "Version mismatch"
    && element('.server-list-card[data-archive-id="e"]').classList.contains("is-active"),
    "Rapid selection queues the latest read behind the active native request without showing old configuration");
  const staleDetails = hooks.configuration("b");
  const staleResult = { ...staleDetails, instance: { ...staleDetails.instance, settings_json: '{"archive_marker":"STALE_CONFIG_B"}' } };
  await hooks.resolve(stale, staleResult);
  check(hooks.last(command).args.input?.archive_id === "e" && count() === beforeRapid + 1
    && !fixture.querySelector(workspace("b")) && !element(".server-detail-panel").textContent?.includes("STALE_CONFIG_B"),
    "A stale native result is ignored and the superseded queued archive read is cancelled");
  await hooks.resolve(hooks.last(command), hooks.configuration("e")); await hooks.waitFor(state("e", "ready"));
  await selectArchiveSettings(hooks, "e");
  check(element<HTMLTextAreaElement>(`${preview("e")} .configuration-workspace__raw-json`).value.includes("ARCHIVED_CONFIG_e"),
    "The latest selected archive owns the completed read-only preview");
  phases.push("stale selection");
  hooks.setManualReads(false);
  await hooks.changeMode("normal");
  assertNormalWorkspaceRestored(fixture, check, hooks.normalSnapshot, hooks.normalIdentity());
  phases.push("normal workspace restored");
  await hooks.changeMode("archived"); await hooks.selectArchive("a"); await hooks.waitFor(state("a", "ready"));
  check(hooks.saves() === savedBefore && hooks.starts() === startedBefore, "All archive preview paths remain read-only and never start a server");
}
