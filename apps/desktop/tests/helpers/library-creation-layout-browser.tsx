import React, { act, StrictMode, useState } from "react";
import { createRoot } from "react-dom/client";
import { invokeMock } from "../../src/api-mock";
import { I18nProvider } from "../../src/i18n";
import { LibraryDetailPage } from "../../src/views/library/LibraryDetailPage";
import type { ModuleMediaItem, ModuleStoreEntry } from "../../src/store-media";
import type { CreateInstanceInput, ModuleDetails, SteamCmdStatus } from "../../src/types";
import type { ModuleProgramInventory } from "../../src/storage-management-types";
import "../../src/app.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
document.documentElement.dataset.theme = "dark";
const fixture = document.getElementById("fixture")!;
const root = createRoot(fixture);
const nonce = new URLSearchParams(location.search).get("nonce");
const errors: string[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
addEventListener("securitypolicyviolation", (event) => errors.push(`Blocked unexpected resource: ${event.blockedURI}`));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
let checks = 0;
function check(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
  checks++;
}
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const value = fixture.querySelector<T>(selector);
  if (!value) throw new Error(`Missing ${selector}`);
  return value;
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
async function settle(predicate: () => boolean, message: string) {
  const deadline = performance.now() + 6000;
  while (!predicate()) {
    if (performance.now() >= deadline) throw new Error(message);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
const inventoryPath = "D:/Fixture/server-files/astroneer";
const requests: Array<{ input: Record<string, unknown>; reply: ReturnType<typeof deferred<ModuleProgramInventory>> }> = [];
const created: CreateInstanceInput[] = [];
const creationReplies: Array<ReturnType<typeof deferred<void>>> = [];
let uninstallCalls = 0;
let automaticReply: ModuleProgramInventory | Error | null = null;
Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: {
  invoke(command: string, args: { input?: Record<string, unknown> } = {}) {
    if (command !== "inspect_module_programs") throw new Error(`Unexpected native command: ${command}`);
    const reply = deferred<ModuleProgramInventory>();
    requests.push({ input: args.input ?? {}, reply });
    if (automaticReply instanceof Error) reply.reject(automaticReply);
    else if (automaticReply) reply.resolve(automaticReply);
    return reply.promise;
  },
} });
function geometry() {
  const rect = (selector: string) => {
    const { left, top, right, bottom, width, height } = element(selector).getBoundingClientRect();
    return { left, top, right, bottom, width, height };
  };
  return { media: rect(".library-detail-stage-media"), player: rect(".library-detail-stage-player"),
    sidebar: rect(".library-sidebar-summary"), actions: rect(".library-sidebar-server-actions") };
}
async function keyboard(key: "Escape" | "Enter") {
  await act(async () => {
    const response = await fetch(`/__reliability_key/${nonce}/${key}`, { method: "POST" });
    check(response.ok, `Native ${key} dispatch failed`);
  });
}
function visibleCopy() {
  const walker = document.createTreeWalker(element(".library-sidebar-server-actions"), NodeFilter.SHOW_TEXT);
  const parts: string[] = [];
  while (walker.nextNode()) {
    const parent = walker.currentNode.parentElement;
    if (!parent || parent.closest(".configuration-field-help-a11y, .configuration-field-help-tooltip")) continue;
    const style = getComputedStyle(parent);
    if (style.display !== "none" && style.visibility !== "hidden") parts.push(walker.currentNode.textContent ?? "");
  }
  return parts.join(" ");
}
async function helpOnFocus(target: HTMLElement, expected: string, dismiss = true, descriptionTarget = target) {
  await act(async () => { target.scrollIntoView({ block: "center" }); target.focus(); });
  check(document.activeElement === target, "Help control must be keyboard focusable");
  const description = document.getElementById(descriptionTarget.getAttribute("aria-describedby") ?? "");
  check(description?.textContent?.includes(expected), "Help must expose its complete accessible description");
  await settle(() => [...document.querySelectorAll<HTMLElement>(".configuration-field-help-tooltip.is-visible")]
    .some((node) => node.textContent?.includes(expected)), `Missing visible tooltip for ${expected}`);
  const tooltip = [...document.querySelectorAll<HTMLElement>(".configuration-field-help-tooltip.is-visible")]
    .find((node) => node.textContent?.includes(expected))!;
  await settle(() => Number(getComputedStyle(tooltip).opacity) >= 0.99,
    "The visible help bubble did not finish its entrance transition");
  const bounds = tooltip.getBoundingClientRect();
  check(bounds.width > 0 && bounds.height > 0 && bounds.left >= 0 && bounds.right <= innerWidth
    && bounds.top >= 0 && bounds.bottom <= innerHeight + 1, "Help bubble must remain within the viewport");
  if (!dismiss) return;
  await keyboard("Escape");
  await settle(() => !document.querySelector(".configuration-field-help-tooltip.is-visible"), "Escape did not dismiss help");
  check(document.activeElement === target, "Dismissing a help bubble must preserve keyboard focus");
}
function assertPlayerRatio() {
  const player = element(".library-detail-stage-player").getBoundingClientRect();
  check(Math.abs(player.width / player.height - 16 / 9) < 0.005,
    `The media player must retain 16:9, actual ${player.width}x${player.height}`);
}
function assertAligned() {
  const layout = geometry();
  check(Math.abs(layout.media.top - layout.sidebar.top) < 1 && Math.abs(layout.media.bottom - layout.sidebar.bottom) < 1,
    `The media and action cards must share top/bottom boundaries: ${JSON.stringify(layout)}`);
}

async function run() {
  const config = await fetch("/__library_creation_config").then((response) => response.json()) as {
    locale: "zh-CN" | "en-US"; captureHelp: boolean;
  };
  localStorage.setItem("langame.locale", config.locale);
  await Promise.all([import("../../src/i18n-messages"), import("../../src/i18n-messages-zh-cn")]);
  const chinese = config.locale === "zh-CN";
  const original = await invokeMock<ModuleDetails>("read_module_details", { moduleId: "astroneer" });
  check(original.summary.id === "astroneer" && original.summary.steam_app_id === 728470,
    "The fixture must use the real Astroneer module contract");
  const details: ModuleDetails = { ...original, summary: { ...original.summary, install_state: "Installed" } };
  const ready: SteamCmdStatus = { ready: true, root: "D:/Fixture/steamcmd", executable_path: "D:/Fixture/steamcmd/steamcmd.exe",
    executable_exists: true, configured_root: "D:/Fixture/steamcmd", configured_executable_path: "D:/Fixture/steamcmd/steamcmd.exe",
    source: "managed", ownership: "managed", can_uninstall: true };
  const reason = chinese
    ? "现有安装已用于其他实例。新服务器将复制已核验的原始程序文件，使用独立配置与存档，不会继承未知文件、模组或已有世界。"
    : "This installation has already been used by another instance. A new server copies verified original program files with separate settings and saves, excluding unknown files, Mods and existing worlds.";
  const inventory: ModuleProgramInventory = {
    requires_archive_inventory: false,
    installations: [{ id: 71, install_root: inventoryPath, scope: "library", install_state: "Installed",
      current_version: "25119807", used_by: [{ id: "fixture-existing", name: chinese ? "已有合作世界" : "Existing co-op world" }],
      modification_state: "verified_original", pending_removal: false, size_bytes: 2.5 * 1024 ** 3 }],
    creation: { can_create: true, action: "independent_install", program_path: inventoryPath,
      additional_bytes: 2.5 * 1024 ** 3, reason },
  };
  const artwork = "/__library_creation_art.svg";
  const media: ModuleMediaItem[] = Array.from({ length: 4 }, (_, index) => ({ key: `fixture-${index}`, kind: "screenshot",
    title: `Astroneer ${index + 1}`, badge: "", thumbnailSrc: artwork, imageSrc: artwork, streamUrl: null }));
  const story = chinese ? "探索遥远的星球，与好友建造基地。本页面只使用本地合成媒体和隔离库存验证界面。"
    : "Explore distant planets and build a base with friends. This page uses local synthetic media and isolated inventory for layout verification.";
  const store: ModuleStoreEntry = { storeSource: "official", storeAppId: null, storeName: "ASTRONEER", coverUrl: artwork,
    shortDescription: story, aboutParagraphs: [story], genres: [], categories: [], developers: ["System Era Softworks"],
    publishers: ["System Era Softworks"], releaseDate: "2019-02-06", storeUrl: "https://example.invalid/astroneer",
    officialLinks: [], screenshots: [], trailers: [] };
  let refreshInventory: () => void;
  function Harness() {
    const [name, setName] = useState(chinese ? "星际合作世界" : "Astroneer co-op world");
    const [active, setActive] = useState(media[0]);
    const [creating, setCreating] = useState(false);
    const [inventoryRevision, setInventoryRevision] = useState(0);
    refreshInventory = () => setInventoryRevision((value) => value + 1);
    return <main className="shell-content-scroll" style={{ height: "100vh", padding: "20px", overflowY: "auto" }}>
      <LibraryDetailPage selected={{ ...details.summary, instance_program_count: inventoryRevision }} selectedModuleDetails={details} steamCmdStatus={ready} steamCmdBusy={false}
        storeEntry={store} mediaItems={media} activeMedia={active} heroTitle="ASTRONEER" heroSubtitle={story}
        detailHeaderMeta={chinese ? "异星探险家 · 专用服务器" : "Space exploration · Dedicated server"}
        storyParagraphs={[story]} categoryTags={chinese ? ["在线合作"] : ["Online co-op"]}
        genreTags={chinese ? ["冒险", "沙盒"] : ["Adventure", "Sandbox"]}
        installLabel={chinese ? "已安装" : "Installed"} installBusy={false} installStatusClass="is-success"
        creating={creating} instanceName={name} onBackToCatalog={() => {}}
        onActiveMediaChange={(key) => setActive(media.find((item) => item.key === key)!)}
        onInstall={() => {}} onUninstall={() => { uninstallCalls++; }} onInstanceNameChange={setName}
        onCreateServer={async (input) => {
          created.push(input);
          const reply = deferred<void>();
          creationReplies.push(reply);
          setCreating(true);
          try { await reply.promise; } finally { setCreating(false); }
        }} />
    </main>;
  }
  await act(async () => { root.render(<StrictMode><I18nProvider><Harness /></I18nProvider></StrictMode>); });
  await settle(() => requests.length > 0, "The real inventory hook did not request native inventory");
  const submit = () => element<HTMLButtonElement>('.library-create-submit button[type="submit"]');
  check(element(".library-program-inventory").getAttribute("aria-busy") === "true"
    && Boolean(fixture.querySelector('.library-program-inventory [role="status"]')),
    "Inventory loading must have an accessible pending state");
  check(submit().disabled, "Creation must stay disabled until inventory is known");
  check(!fixture.querySelector(".library-program-choices select"), "Independent-only creation requires no program ownership or content choice");
  check(requests.every((request) => request.input.program_source === "verified"), "Inventory always plans a clean program");
  assertPlayerRatio();
  assertAligned();
  const failure = new Error("Fixture inventory unavailable");
  await act(async () => {
    automaticReply = failure;
    for (const request of requests) request.reply.reject(failure);
  });
  await settle(() => Boolean(fixture.querySelector('.library-program-inventory [role="alert"]')),
    "Inventory failure must show an accessible error");
  check(submit().disabled, "Failed inventory must block creation");
  assertAligned();
  await helpOnFocus(element('.library-program-inventory [role="alert"]'), failure.message);
  const failedRequestCount = requests.length;
  await act(async () => {
    automaticReply = null;
    element<HTMLButtonElement>(".library-program-inventory button").click();
  });
  await settle(() => requests.length > failedRequestCount, "Retry must request a fresh inventory");
  check(element(".library-program-inventory").getAttribute("aria-busy") === "true" && submit().disabled,
    "Retry must restore the loading state and keep creation disabled");
  await act(async () => {
    automaticReply = inventory;
    for (const request of requests) request.reply.resolve(inventory);
  });
  await settle(() => fixture.querySelector(".library-program-inventory")?.getAttribute("aria-busy") === "false",
    "Program inventory did not settle");
  await document.fonts.ready;
  await settle(() => Boolean(fixture.querySelector<HTMLImageElement>(".library-detail-stage-player img")?.naturalWidth),
    "The locally served fixture artwork did not load");
  await settle(() => Boolean(fixture.querySelector<HTMLImageElement>(".library-sidebar-cover img")?.naturalWidth),
    "The sidebar must display the supplied game cover");
  const cover = element(".library-sidebar-cover").getBoundingClientRect();
  const infoBox = element(".library-sidebar-info").getBoundingClientRect();
  check(cover.width > 0 && Math.abs(cover.width / cover.height - 460 / 215) < 0.01,
    "The sidebar cover must retain the horizontal store capsule ratio");
  check(cover.left >= infoBox.left - 1 && cover.right <= infoBox.right + 1
    && Math.abs(cover.top - infoBox.top) < 1 && cover.bottom <= infoBox.bottom + 1,
    "The complete cover must fit at the top of the sidebar information area");
  check(element(".library-sidebar-specs").getBoundingClientRect().top >= cover.bottom,
    "Game metadata must follow the cover without overlap");
  await act(async () => { await new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))); });
  const stablePlayer = element(".library-detail-stage-player").getBoundingClientRect();
  check(!submit().disabled, "Successful inventory retry must enable creation");
  check(element<HTMLInputElement>('input.text-input').value === (chinese ? "星际合作世界" : "Astroneer co-op world"),
    "Inventory failure/retry must preserve the instance name");
  check(!fixture.querySelector(".library-program-choices select"),
    "The real Astroneer module keeps independent ownership without an unnecessary choice");
  check(!visibleCopy().includes(inventoryPath) && !visibleCopy().includes("25119807") && !visibleCopy().includes(reason),
    "Paths, version and long creation explanations must not remain in the default layout");
  check(!fixture.querySelector(".library-program-source-hint"), "Program source help must not be a permanent paragraph");
  const metrics = [...fixture.querySelectorAll<HTMLElement>('.library-program-metric[role="note"]')];
  check(metrics.length === 2, "Available programs and additional disk use need two compact metrics");
  check(metrics.every((metric) => metric.getBoundingClientRect().height <= 36), "Inventory metrics must remain compact");
  await helpOnFocus(metrics[0], inventoryPath);
  check(document.getElementById(metrics[0].getAttribute("aria-describedby")!)?.textContent?.includes("25119807"),
    "The inventory bubble must retain the installed version");
  const creationHelp = document.getElementById(metrics[1].getAttribute("aria-describedby") ?? "")?.textContent;
  check(creationHelp && /2\.5\s*GB/.test(creationHelp)
    && (chinese ? /复制.*独立程序/.test(creationHelp) : /copy an independent program/i.test(creationHelp)),
    "Ready creation help must retain the action and additional disk usage");
  check(!creationHelp.includes(reason) && !creationHelp.includes(inventoryPath)
    && creationHelp.split("\n").filter(Boolean).length <= 2,
    "Ready creation help must stay concise without repeated paths or long success explanations");
  await helpOnFocus(metrics[1], creationHelp);
  assertAligned();
  const createHelpTarget = submit().parentElement!;
  const cleanHelp = document.getElementById(createHelpTarget.getAttribute("aria-describedby") ?? "")?.textContent;
  check(cleanHelp && (chinese ? /自动校验.*补齐原版/.test(cleanHelp) : /Automatically verify.*original files/.test(cleanHelp)),
    "Creation help explains automatic verification and acquisition");
  check(chinese ? cleanHelp.includes("不继承旧配置、存档和模组") : cleanHelp.includes("Old settings, saves and mods are not inherited"),
    "Creation help explains that the new server does not inherit previous instance data");
  await helpOnFocus(submit(), cleanHelp, true, createHelpTarget);
  const blockedReason = chinese ? "磁盘空间不足，无法创建独立程序。" : "Insufficient disk space for an independent program.";
  await act(async () => {
    automaticReply = { ...inventory, creation: { ...inventory.creation, can_create: false, reason: blockedReason } };
    refreshInventory();
  });
  await settle(() => Boolean(fixture.querySelector('.library-program-metric[role="status"]'))
    && element(".library-program-inventory").getAttribute("aria-busy") === "false",
    "A blocked creation plan must expose its status");
  check(submit().disabled, "A blocked creation plan must disable creation");
  await helpOnFocus(element('.library-program-metric[role="status"]'), blockedReason);
  assertAligned();
  await act(async () => {
    automaticReply = inventory;
    refreshInventory();
  });
  await settle(() => !submit().disabled && requests.at(-1)?.input.program_source === "verified",
    "A ready inventory must restore creation after a blocked plan");
  await act(async () => {
    const name = element<HTMLInputElement>('input.text-input');
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(name, "  Fixture explorer  ");
    name.dispatchEvent(new Event("input", { bubbles: true }));
  });
  check(requests.every((request) => request.input.program_source === "verified"),
    "Every inventory refresh keeps clean program preparation without user selection");
  await act(async () => { submit().click(); });
  await settle(() => created.length === 1 && submit().disabled, "Creation must show its pending state");
  check(element<HTMLInputElement>('input.text-input').disabled,
    "Pending creation must disable the instance name without discarding it");
  check(Boolean(fixture.querySelector('.shell-activity-notice[role="status"]')),
    "Pending creation must keep its progress visible in the action area");
  assertAligned();
  assertPlayerRatio();
  check(Math.abs(element(".library-detail-stage-media").getBoundingClientRect().height - geometry().sidebar.height) < 1,
    "Creation progress must not stretch either column");
  await act(async () => { creationReplies[0].resolve(); });
  await settle(() => created.length === 1 && !submit().disabled, "Create must finish and refresh the inventory");
  check(created[0].module_id === "astroneer" && created[0].name === "Fixture explorer"
    && created[0].program_mode === "independent" && !("program_source" in created[0]),
    `Creation lost selected values: ${JSON.stringify(created[0])}`);
  check(!fixture.querySelector(".library-program-choices select") && element<HTMLInputElement>('input.text-input').value === "  Fixture explorer  ",
    "Creation's inventory refresh preserves the name without adding a content choice");
  assertPlayerRatio();
  const finalPlayer = element(".library-detail-stage-player").getBoundingClientRect();
  check(Math.abs(finalPlayer.width - stablePlayer.width) < 1 && Math.abs(finalPlayer.height - stablePlayer.height) < 1,
    "Tooltip and creation state changes must not stretch the media surface");
  const maintenance = [...fixture.querySelectorAll<HTMLButtonElement>(".library-sidebar-maintenance-row button")];
  check(maintenance.length === 3, "Install, update and uninstall must remain available as three actions");
  const tops = maintenance.map((button) => button.getBoundingClientRect().top);
  check(Math.max(...tops) - Math.min(...tops) < 1, "Maintenance actions must stay on one row");
  check(maintenance.slice(1).every((button) => Boolean(button.getAttribute("aria-label")) && !button.textContent?.trim()),
    "Compact maintenance icons need accessible names without permanent text");
  await act(async () => { maintenance[2].click(); });
  await settle(() => Boolean(fixture.querySelector(".inline-confirm-review")), "Uninstall must open its inline confirmation");
  check(uninstallCalls === 0, "Opening uninstall review must not uninstall files");
  assertAligned();
  assertPlayerRatio();
  const reviewMessage = element(".inline-confirm-message");
  const reviewButtons = [...fixture.querySelectorAll<HTMLButtonElement>(".inline-confirm-buttons button")];
  check(reviewButtons.length === 2, "Uninstall review must retain both cancel and confirm actions");
  const summaryBounds = element(".library-sidebar-summary").getBoundingClientRect();
  for (const control of [reviewMessage, ...reviewButtons]) {
    const bounds = control.getBoundingClientRect();
    check(bounds.left >= summaryBounds.left && bounds.right <= summaryBounds.right
      && bounds.top >= summaryBounds.top && bounds.bottom <= summaryBounds.bottom,
      `Uninstall confirmation content must fit inside its card: ${control.className} ${JSON.stringify(bounds.toJSON())}`);
    check(control.scrollWidth <= control.clientWidth + 1 && control.scrollHeight <= control.clientHeight + 1,
      "Uninstall confirmation must not clip its message or action labels");
  }
  for (const button of reviewButtons) {
    const bounds = button.getBoundingClientRect();
    const hit = document.elementFromPoint(bounds.left + bounds.width / 2, bounds.top + bounds.height / 2);
    check(hit && (Object.is(hit, button) || button.contains(hit)),
      `Uninstall confirmation action must be visible and not covered: ${button.textContent}`);
  }
  check(!fixture.querySelector(".library-program-choices select") && element<HTMLInputElement>('input.text-input').value === "  Fixture explorer  ",
    "Maintenance confirmation must preserve creation inputs");
  await keyboard("Escape");
  await settle(() => !fixture.querySelector(".inline-confirm-review"), "Escape must cancel uninstall review");
  check(uninstallCalls === 0, "Cancelling uninstall review must not call the native operation");
  check(Object.is(document.activeElement, element(".inline-confirm-action > button")),
    "Escape must return keyboard focus to the uninstall trigger");
  check(document.documentElement.scrollWidth <= innerWidth + 1, "The full detail page must not overflow horizontally");
  for (const control of [...fixture.querySelectorAll<HTMLElement>('input, select, .library-sidebar-maintenance-row button, .library-create-submit button')]) {
    check(control.scrollWidth <= control.clientWidth + 1, `Control label is clipped: ${control.outerHTML.slice(0, 180)}`);
  }
  await act(async () => { (document.activeElement as HTMLElement | null)?.blur(); element(".shell-content-scroll").scrollTop = 0; });
  await settle(() => !document.querySelector(".configuration-field-help-tooltip"), "Help portals did not close before the final layout capture");
  const layout = geometry();
  assertAligned();
  const info = element(".library-sidebar-info");
  if (innerWidth === 960) {
    check(info.scrollHeight > info.clientHeight && getComputedStyle(info).overflowY === "auto",
      "At the compact viewport, game metadata must remain available through its own scroll area");
    const actionsTop = element(".library-sidebar-server-actions").getBoundingClientRect().top;
    info.scrollTop = info.scrollHeight;
    check(info.scrollTop > 0 && Math.abs(element(".library-sidebar-server-actions").getBoundingClientRect().top - actionsTop) < 1,
      "Scrolling metadata must not move creation controls");
    const link = element(".library-sidebar-info .library-detail-store-link").getBoundingClientRect();
    const infoBox = info.getBoundingClientRect();
    check(link.top >= infoBox.top - 1 && link.bottom <= infoBox.bottom + 1,
      "Scrolling metadata must expose its last store link");
    info.scrollTop = 0;
    const createBox = submit().getBoundingClientRect();
    check(createBox.top >= 0 && createBox.bottom <= innerHeight + 1,
      "The create action must remain visible in the compact viewport");
  }
  if (config.captureHelp) await helpOnFocus(
    fixture.querySelectorAll<HTMLElement>('.library-program-metric[role="note"]')[1], creationHelp, false);
  check(errors.length === 0, `Browser errors: ${errors.join("; ")}`);
  return { status: "passed", checks, locale: config.locale, geometry: layout, requests: requests.length,
    created, browser_errors: errors };
}
void run().catch((error) => ({ status: "failed", checks, error: String(error instanceof Error ? error.stack : error), browser_errors: errors }))
  .then((report) => {
    Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false });
    return fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) });
  });
