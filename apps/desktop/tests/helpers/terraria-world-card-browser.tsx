import React, { act, StrictMode, useEffect, useState, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { I18nProvider } from "../../src/i18n";
import { createDefaultAiSettings } from "../../src/ai-settings";
import { invokeMock } from "../../src/api-mock";
import type { WorldStartConfirmation } from "../../src/api-terraria-world";
import type { UiMessage } from "../../src/app-ui";
import { InstanceSettingsSaveProvider, useInstanceSettingsSaveCoordinator } from "../../src/views/settings/InstanceSettingsSaveContext";
import { ServersView } from "../../src/views/ServersView";
import "../../src/app.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN");
document.documentElement.dataset.theme = "dark";
const nonce = new URLSearchParams(location.search).get("nonce");
const root = createRoot(document.getElementById("fixture")!);
const errors: string[] = [];
addEventListener("error", e => errors.push(e.message));
addEventListener("unhandledrejection", e => errors.push(String(e.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
const noop = () => {};
const world = { instance_id: "terraria-card", world_file: "PublicStartup1102.wld", suggested_world_file: "PublicStartup1102-tml.wld",
  settings_sha256: "a".repeat(64), world_version: 326, max_world_version: 279 };
const starts: (WorldStartConfirmation | undefined)[] = [];
const activity: UiMessage[] = [];
let previews = 0;
let failDraft = false;
let occupied = false;
let compatible = false;
let releasePreview: (() => void) | null = null;
let delayPreview = false;
let dialogs = 0;
window.confirm = () => { dialogs++; throw Error("Unexpected system confirmation"); };

const fetchOriginal = window.fetch.bind(window);
sessionStorage.setItem("langameLanToken", "ui-fixture");
window.fetch = async (request, init) => {
  if (request !== "/__langame/api") return fetchOriginal(request, init);
  const { command, args } = JSON.parse(String(init?.body));
  if (command === "preview_terraria_mod_world") {
    if (args.instanceId !== world.instance_id) throw Error("Preview belongs to another instance");
    previews++;
    if (delayPreview) await new Promise<void>(resolve => { releasePreview = resolve; });
    return Response.json({ ok: true, value: compatible ? null : world });
  }
  if (command === "read_instance_connection_info_from_storage") return Response.json({ ok: true,
    value: args.instanceIds.map((id: string) => ({ instance_id: id, bind_ip: "127.0.0.1", settings_json: "{}", ports: [
      { name: "game", protocol: "tcp", port: id === world.instance_id ? 7787 : 25565 }
    ] })) });
  try { return Response.json({ ok: true, value: await invokeMock(command, args) }); }
  catch (error) { return Response.json({ ok: false, error: String(error) }); }
};

function Workspace() {
  const [id, setId] = useState<string | null>(world.instance_id);
  const saves = useInstanceSettingsSaveCoordinator();
  useEffect(() => {
    const draft = saves.register(world.instance_id, async () => { if (failDraft) throw Error("Invalid unsaved settings"); });
    return () => draft.detach(Promise.resolve());
  }, [saves]);
  const props: ComponentProps<typeof ServersView> = {
    aiSettings: createDefaultAiSettings(), assistantCanRun: false, bindAddressCandidates: [],
    instances: [world.instance_id, "other-card"].map((id, i) => ({ id, module_id: i ? "minecraft" : "terraria",
      name: i ? "其他实例" : "泰拉瑞亚测试服", status: "Stopped", bind_ip: "0.0.0.0", autostart: false, active_process_count: 0 })),
    moduleInstallations: { terraria: { installState: "Installed", hasManagedInstallSource: true },
      minecraft: { installState: "Installed", hasManagedInstallSource: true } },
    instanceLaunchFailures: {}, instanceLaunchPlans: {}, section: "overview", onWorkspaceSectionChange: noop,
    selectedInstanceId: id, selectedDetails: null, selectedBackups: [], selectedModuleDetails: null, selectedModuleDetailsError: null,
    runtime: null, runtimeWindows: null, launchPlan: null, launchPlanError: null, refreshIssue: null,
    onActivity: notice => { activity.push(notice); }, onResumeAutoRefresh: noop, onSelectInstance: setId,
    onStart: (_id, confirmation) => { starts.push(confirmation); if (occupied) throw Error("New world filename is occupied"); }, onStop: noop,
    onPickDirectory: async () => null, onImportDontStarveWorldData: async () => { throw Error("Unexpected import"); },
    onInstallModule: async () => {}, onOpenModuleLibrary: noop, onOpenLocalPath: noop, onCreateInstance: noop,
    onSendRuntimeCommand: async () => null, onSuppressRuntimeWindows: async () => {}, onCreateBackup: noop, onRestoreBackup: noop,
    onArchivesChanged: async () => {}, onArchiveInstance: noop, onDeleteInstance: noop, onRenameBackup: async () => true, onDeleteBackup: noop,
    onSaveSettings: async () => undefined, onSaveAutostart: async () => {}, onApplyPlayerAccessMutation: async () => { throw Error("Unexpected action"); }
  };
  return <ServersView {...props} />;
}
function check(value: unknown, message: string): asserts value { if (!value) throw Error(message); }
async function waitFor(predicate: () => boolean, message: string) {
  const deadline = performance.now() + 8000;
  while (!predicate()) {
    check(performance.now() < deadline, message);
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 10)); });
  }
}
const card = (id = world.instance_id) => document.querySelector<HTMLElement>(`[data-card-id="${id}"]`);
const review = () => card()?.querySelector<HTMLElement>(".server-list-card-new-world .inline-confirm-review");
async function click(node: HTMLElement | null | undefined) {
  check(node, "Expected an available control");
  await act(async () => { node.click(); });
}
const start = () => click(card()?.querySelector(".server-list-card-primary-action"));
async function ask() { await start(); await waitFor(() => Boolean(review()), "Expected an inline confirmation on the Terraria card"); }

async function run() {
  await act(async () => { root.render(<StrictMode><I18nProvider><InstanceSettingsSaveProvider><Workspace />
    </InstanceSettingsSaveProvider></I18nProvider></StrictMode>); });
  await waitFor(() => Boolean(card()), "Instance cards did not load");
  failDraft = true;
  await start();
  await waitFor(() => activity.at(-1)?.key === "terraria.start.prepareFailed", "Save failure must block preview and startup");
  check(previews === 0 && !review() && starts.length === 0, "An invalid draft reached preview or launch");
  failDraft = false;

  delayPreview = true;
  await start();
  await waitFor(() => Boolean(releasePreview), "Delayed preview never began");
  const filter = document.querySelector<HTMLInputElement>(".server-list-filter input")!;
  await click(filter);
  check(!card(), "Stopped card should currently be filtered out");
  await act(async () => { delayPreview = false; releasePreview!(); });
  await waitFor(() => Boolean(review()), "The review must temporarily reveal the filtered card");
  check(review()?.contains(document.activeElement) && document.activeElement?.textContent === "取消", "Cancel must be focused first");
  await act(async () => { const reply = await fetch(`/__reliability_key/${nonce}/Escape`, { method: "POST" }); check(reply.ok, "Escape dispatch"); });
  await waitFor(() => !card(), "Cancel must restore the unchanged filter");
  check(filter.checked && starts.length === 0, "Esc launched the game or changed the filter");
  await click(filter);

  await ask();
  await click(card("other-card")?.querySelector(".server-list-card-hitarea"));
  await waitFor(() => !review(), "Changing the selected card must dismiss the pending request");
  check(starts.length === 0, "Selection change started the old instance");
  await ask();
  await act(async () => { document.querySelector<HTMLInputElement>(".server-list-search")?.focus(); });
  await waitFor(() => !review(), "Focus outside must dismiss a review");
  await ask();
  await click(review()?.querySelector("button"));
  check(!review() && starts.length === 0 && document.activeElement === card()?.querySelector(".server-list-card-primary-action"),
    "Cancel must restore focus without starting");

  occupied = true;
  await ask();
  await click(review()?.querySelector(".inline-confirm-submit"));
  check(starts.length === 1 && JSON.stringify(starts[0]) === JSON.stringify({ create_terraria_world: world }),
    "Only the reviewed snapshot may reach startup");
  check(activity.at(-1)?.key === "terraria.start.prepareFailed", "A filename collision must be reported without a retry");
  occupied = false;
  compatible = true;
  await start();
  await waitFor(() => starts.length === 2, "The compatible instance never dispatched its start");
  check(starts.length === 2 && starts[1] === undefined && !review(), "A compatible save should start directly");

  compatible = false;
  await ask();
  check(!document.querySelector("dialog[open]") && !dialogs && document.querySelectorAll(".server-list-card-new-world").length === 1,
    "World confirmation must be on one card only");
  const box = card()!.getBoundingClientRect();
  for (const item of review()!.querySelectorAll<HTMLElement>(".inline-confirm-message, button")) {
    const rect = item.getBoundingClientRect();
    check(rect.width > 0 && rect.height > 0 && rect.left >= box.left && rect.right <= box.right && rect.top >= box.top && rect.bottom <= box.bottom,
      "Review text or buttons escaped the card");
    if (item.classList.contains("inline-confirm-message")) check(item.scrollHeight <= item.clientHeight + 1, "The sample filenames need scrolling");
    if (item.tagName === "BUTTON") check(item.contains(document.elementFromPoint(rect.x + rect.width / 2, rect.y + rect.height / 2)), "Button is obscured");
  }
  check(review()!.textContent?.includes(world.world_file) && review()!.textContent?.includes(world.suggested_world_file), "Both save filenames must be visible");
  check(errors.length === 0, errors.join("\n"));
  return { status: "passed", inline: true, native_dialogs: dialogs, browser_errors: errors, reviewed_starts: starts.length };
}
void run().catch(error => ({ status: "failed", error: String(error?.stack ?? error), browser_errors: errors }))
  .then(report => fetchOriginal(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
