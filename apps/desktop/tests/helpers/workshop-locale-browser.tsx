import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import React, { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { readInstanceDetails, updateInstance } from "../../src/api";
import { buildMockModuleDetails } from "../../src/api-mock/module-details";
import { ActivityNoticeTarget } from "../../src/components/ActivityNotice";
import { I18nProvider, useI18n } from "../../src/i18n";
import { ModWorkbench } from "../../src/views/servers/ModWorkbench";
import { InstanceSettingsSaveProvider } from "../../src/views/settings/InstanceSettingsSaveContext";
import type { InstanceDetails, SteamWorkshopLookupItem } from "../../src/types";
import { createModWorkbenchFixtureIpc, workshopItem } from "./mod-workbench-browser-support";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "en-US");
document.documentElement.dataset.theme = "dark";
const fixture = document.getElementById("fixture")!;
fixture.style.cssText = "height:100vh;padding:16px;box-sizing:border-box;display:flex;flex-direction:column";
const root = createRoot(fixture), checks: string[] = [], errors: string[] = [];
const moduleDetails = buildMockModuleDetails("dontstarve");
const parentId = "456789", childId = "111111", originalId = "222222", configuredId = "333333";
const originalTitle = "Architect's Original 标题", originalDescription = "Creator supplied text remains exactly as published.";
let details: InstanceDetails, epoch = 0, writes = 0, downloads = 0;
let holdEnglish = false, holdEnglishBrowse = false, holdChineseParent = false, holdDownload = false;
let holdValidationBatch = false, newerClientOnlyDetails = false;
let failOriginalLocalization = true;
const localizationFailure = JSON.stringify({ code: "steam_workshop_network_failed", stage: "details",
  message: "Workshop details were rate limited", origin: "https://steamcommunity-a.akamaihd.net", reason: "http", status: 429, retry_after_seconds: 60 });
const clientOnlyDescription = "较新的详情已确认该模组仅在客户端运行。";
const cached = new Set([configuredId]);
const requests: { command: string; locale: string; ids: string[]; query: string }[] = [];
const pending: { command: string; locale: string; ids: string[]; release: () => void }[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
function assert(value: unknown, label: string): asserts value { if (!value) throw new Error(`${label}: ${fixture.textContent}`); checks.push(label); }
function catalog(locale: string): SteamWorkshopLookupItem[] {
  const chinese = locale === "zh-CN";
  const child = { ...workshopItem(childId, chinese ? "Locale 作者成员·中文" : "Locale Creator member·English"),
    description: chinese ? "作者提供的中文成员说明。" : "Creator supplied English member description." };
  const original = { ...workshopItem(originalId, originalTitle), description: originalDescription };
  const parent = { ...workshopItem(parentId, chinese ? "Locale 作者合集·中文" : "Locale Creator collection·English"),
    description_excerpt: chinese ? "中文合集摘要。" : "English collection summary.",
    description: chinese ? "作者提供的完整中文合集说明。" : "Creator supplied full English collection description.",
    item_kind: "collection", child_count: 2, children: [child, original].map(({ id, title, item_kind, status, tags }) =>
      ({ id, title, item_kind, status, tags, consumer_app_id: 322330 })) };
  return [parent, child, original, { ...workshopItem(configuredId, chinese ? "已添加模组·中文" : "Configured Mod·English") }];
}
function Controls() {
  const { locale, setLocale } = useI18n();
  return <div data-active-locale={locale} style={{ display: "flex", gap: 8, marginBottom: 8 }}>
    <button type="button" className="ghost-button" onClick={() => setLocale("en-US")}>English</button>
    <button type="button" className="ghost-button" onClick={() => setLocale("zh-CN")}>中文</button>
  </div>;
}
function Workspace() {
  const [noticeTarget, setNoticeTarget] = useState<HTMLDivElement | null>(null);
  return <ActivityNoticeTarget.Provider value={{ element: noticeTarget, dismissLabel: "Close" }}>
    <Controls />
    <div style={{ flex: 1, minHeight: 0 }}><ModWorkbench key={epoch} details={details} moduleDetails={moduleDetails}
      launchPlan={null} onSaveSettings={async (input, options) => {
        const saved = await updateInstance(input, options?.expectedSettingsJson ?? "", options?.collectionRemoval);
        draw(); return saved;
      }} /></div>
    <footer className="shell-activity-bar"><span className="shell-activity-label">Activity</span>
      <div className="shell-activity-notices" ref={setNoticeTarget} /></footer>
  </ActivityNoticeTarget.Provider>;
}
function draw() { root.render(<I18nProvider><InstanceSettingsSaveProvider><Workspace /></InstanceSettingsSaveProvider></I18nProvider>); }
function element<T extends HTMLElement = HTMLElement>(selector: string, within: ParentNode = fixture): T {
  const found = within.querySelector<T>(selector); if (!found) throw new Error(`Missing ${selector}`); return found;
}
function button(label: string) {
  const found = [...fixture.querySelectorAll<HTMLButtonElement>("button")].find((node) => node.textContent?.trim() === label || node.getAttribute("aria-label") === label);
  if (!found) throw new Error(`Missing button ${label}`); return found;
}
async function settle(predicate: () => boolean, label: string) {
  const deadline = performance.now() + 6000;
  while (!predicate()) {
    if (performance.now() > deadline) throw new Error(`${label}: ${fixture.textContent}`);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
async function click(node: HTMLElement) { await act(async () => { node.scrollIntoView({ block: "nearest" }); node.click(); }); }
async function input(node: HTMLInputElement | HTMLTextAreaElement, value: string) {
  await act(async () => {
    Object.getOwnPropertyDescriptor(node.tagName === "TEXTAREA" ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype, "value")!.set!.call(node, value);
    node.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
async function language(label: "English" | "中文") {
  await click(button(label));
  await settle(() => fixture.querySelector("[data-active-locale]")?.getAttribute("data-active-locale") === (label === "中文" ? "zh-CN" : "en-US"), "Language control did not update");
}
function detailText() { return fixture.querySelector(".mw-store-detail")?.textContent ?? ""; }
async function titleIs(title: string) { await settle(() => fixture.querySelector(".mw-store-detail h2")?.textContent === title, `Expected detail title ${title}`); }
async function releaseWhere(predicate: (request: typeof pending[number]) => boolean) {
  const matched = pending.filter(predicate);
  await act(async () => { for (const request of matched) { pending.splice(pending.indexOf(request), 1); request.release(); } });
}
async function run() {
  const template = await readInstanceDetails("srv-dst-terminal-error");
  details = { ...template, summary: { ...template.summary, id: "fixture-workshop-locale", status: "Stopped", active_process_count: 0 }, active_run: null,
    settings_json: JSON.stringify({ shared_workshop_mod_ids: configuredId, master_enabled_workshop_mod_ids: configuredId }) };
  mockWindows("workshop-locale");
  mockIPC(async (command, payload) => {
    const args = (payload ?? {}) as Record<string, unknown>, locale = String(args.locale), ids = (args.ids ?? (args.id ? [args.id] : [])) as string[];
    if (command === "download_steam_workshop_items" && holdDownload) {
      await new Promise<void>((release) => pending.push({ command, locale, ids: [...ids], release }));
    }
    if (["lookup_steam_workshop_items", "read_steam_workshop_item_details", "search_steam_workshop_items"].includes(command)) {
      assert(locale === "en-US" || locale === "zh-CN", "Every Workshop browse and lookup dispatches an explicit supported locale");
      requests.push({ command, locale, ids: [...ids], query: String(args.query ?? "") });
      const heldLookup = ["lookup_steam_workshop_items", "read_steam_workshop_item_details"].includes(command) && ((holdEnglish && locale === "en-US" && ids.some((id) => [childId, configuredId].includes(id))) ||
        (holdChineseParent && locale === "zh-CN" && ids.includes(parentId)));
      const heldValidation = holdValidationBatch && command === "lookup_steam_workshop_items" && ids.includes(configuredId);
      if (heldLookup || heldValidation || (command === "search_steam_workshop_items" && holdEnglishBrowse && locale === "en-US")) {
        await new Promise<void>((release) => pending.push({ command, locale, ids: [...ids], release }));
      }
    }
    if (newerClientOnlyDetails && command === "read_steam_workshop_item_details" && args.id === configuredId) {
      return { ...catalog(locale).find((item) => item.id === configuredId)!, tags: ["client_only_mod"], description: clientOnlyDescription };
    }
    if (command === "read_steam_workshop_item_details" && args.id === originalId) {
      const original = catalog(locale).find((item) => item.id === originalId)!;
      return failOriginalLocalization ? { ...original, item_kind: "unknown", status: "unverified",
        message: localizationFailure, localization_warning: localizationFailure } : { ...original,
        description: locale === "zh-CN" ? "已恢复当前语言的工坊说明。" : "Recovered Workshop description in the selected language.",
        localization_warning: null };
    }
    return createModWorkbenchFixtureIpc(() => ({ details, moduleDetails, catalog: catalog(command === "lookup_steam_workshop_items" ? "en-US" : locale), cached }), {
      check: assert, save: (saved) => { details = saved; writes += 1; }, download: () => { downloads += 1; }
    })(command, payload);
  }, { shouldMockEvents: true });
  Object.assign(window, { isTauri: true });
  await act(async () => { await prepareBrowserLocaleCatalogs(); draw(); });
  await settle(() => Boolean(fixture.querySelector(".mw-mod-card")), "Initial catalog did not load");
  // Warm both real message catalogs so changing language keeps this workspace mounted.
  await language("中文"); await settle(() => fixture.textContent?.includes("Locale 作者成员·中文") === true, "Chinese catalog did not load");
  await language("English"); await settle(() => fixture.textContent?.includes("Locale Creator member·English") === true, "English catalog did not return");
  holdEnglish = true; epoch += 1; await act(async () => { draw(); });
  await settle(() => pending.some((request) => request.ids.includes(configuredId)), "English configured lookup was not held");
  await click(button("Collections"));
  await input(element<HTMLInputElement>(".mw-search-input"), "Locale");
  await settle(() => fixture.querySelectorAll(".mw-mod-card").length === 1 && requests.some((request) => request.command === "search_steam_workshop_items" && request.query === "Locale"),
    "English collection search did not load");
  await click(element(".mw-mod-thumb"));
  const englishParent = catalog("en-US")[0], chineseParent = catalog("zh-CN")[0];
  await settle(() => detailText().includes(englishParent.description!), "Same-language lookup did not load the full English description");
  assert(element(".mw-mod-card-title").textContent === englishParent.title && element(".mw-store-detail h2").textContent === englishParent.title,
    "Opening full details preserves the browse title in the requested language");
  holdEnglishBrowse = true;
  await click(button("Top rated all time"));
  await settle(() => pending.some((request) => request.command === "search_steam_workshop_items"), "English browse was not held");
  await click(element(".mw-mod-thumb"));
  await settle(() => Boolean(fixture.querySelector(".mw-store-detail-child")), "Reopening the collection after sorting lost its loaded members");
  await click(element<HTMLButtonElement>(".mw-store-detail-child"));
  await settle(() => pending.some((request) => request.ids.includes(childId)), "English child details were not held");
  await language("中文");
  await titleIs(catalog("zh-CN")[1].title!);
  assert(!fixture.textContent?.includes("·English") && !detailText().includes("English member description"),
    "Switching locale hides old browse, configured metadata and child previews while old requests are pending");
  holdEnglish = false; holdEnglishBrowse = false;
  await releaseWhere((request) => request.locale === "en-US");
  await settle(() => fixture.querySelector(".mw-mod-card-title")?.textContent === chineseParent.title &&
    fixture.querySelector(".mw-mod-grid")?.getAttribute("aria-busy") === "false", "Chinese browse did not replace the released old request");
  assert(detailText().includes("作者提供的中文成员说明。") && !fixture.textContent?.includes("·English"),
    "Late English detail and configured lookup responses cannot overwrite the Chinese page");
  assert(element<HTMLInputElement>(".mw-search-input").value === "Locale", "Locale changes retain the active search");
  holdChineseParent = true;
  await click(element(".mw-store-detail-back"));
  await settle(() => pending.some((request) => request.locale === "zh-CN" && request.ids.includes(parentId)), "Returning to the parent did not request its current language");
  assert(!detailText().includes(englishParent.title!) && !detailText().includes(englishParent.description!),
    "Back navigation keeps the parent ID without replaying its previous-language snapshot");
  holdChineseParent = false; await releaseWhere((request) => request.locale === "zh-CN");
  await settle(() => detailText().includes(chineseParent.description!), "Chinese parent details did not finish loading");
  assert(element(".mw-store-detail h2").textContent === chineseParent.title && !fixture.querySelector(".mw-store-detail-back"),
    "Returning to the parent preserves its path and current-language details");
  const originalChild = [...fixture.querySelectorAll<HTMLButtonElement>(".mw-store-detail-child")].find((node) => node.querySelector("small")?.textContent === originalId)!;
  await click(originalChild); await titleIs(originalTitle);
  assert(detailText().includes(originalDescription), "Author-provided titles and fallback descriptions are never machine translated");
  for (const currentLanguage of ["中文", "English"] as const) {
    if (currentLanguage === "English") {
      failOriginalLocalization = true;
      await language("English");
      await settle(() => detailText().includes(originalDescription), "English original-text fallback did not load");
    }
    const warning = currentLanguage === "中文" ? "现显示作者原始文案" : "Showing the author's original text";
    await settle(() => fixture.querySelector(".shell-activity-bar")?.textContent?.includes(warning) === true,
      "Original-text fallback did not explicitly identify its language limitation");
    assert(element<HTMLButtonElement>(".mw-store-detail-metric").disabled,
      "Retained original text cannot enable download for an unverified item type");
    failOriginalLocalization = false;
    const notice = [...fixture.querySelectorAll<HTMLElement>(".shell-activity-notice.is-warning")]
      .find((node) => node.textContent?.includes(warning))!;
    const retry = element<HTMLButtonElement>(".shell-activity-notice-actions button", notice);
    await click(retry);
    const recoveredText = currentLanguage === "中文" ? "已恢复当前语言的工坊说明。" : "Recovered Workshop description in the selected language.";
    await settle(() => detailText().includes(recoveredText), "Retry did not restore localized details");
    assert(!fixture.querySelector(".shell-activity-bar")?.textContent?.includes(warning),
      "Successful retry removes the original-text warning in the selected language");
  }
  await language("中文");
  assert(writes === 0 && downloads === 0 && pending.length === 0, "Language changes and detail inspection cause no installation or settings writes and leave no held requests");
  await click(button("清单模式"));
  await input(element<HTMLTextAreaElement>(".mw-manifest-input"), parentId);
  holdChineseParent = true; await click(button("核对清单"));
  await settle(() => pending.some((request) => request.locale === "zh-CN" && request.ids.includes(parentId)), "Chinese manifest review was not held");
  await language("English");
  assert(!fixture.querySelector(".mw-manifest-summary") && element<HTMLTextAreaElement>(".mw-manifest-input").value === parentId,
    "Changing language discards a pending manifest review while preserving its draft");
  holdChineseParent = false; await releaseWhere((request) => request.locale === "zh-CN");
  assert(!fixture.querySelector(".mw-manifest-summary"), "Late old-language manifest inspection cannot publish its review");
  const requestCount = requests.length;
  await click(button("Check list"));
  await settle(() => Boolean(fixture.querySelector(".mw-manifest-summary")), "Current-language manifest review did not complete");
  assert(requests.slice(requestCount).filter((request) => request.command === "lookup_steam_workshop_items").every((request) => request.locale === "en-US"),
    "Rechecking the preserved manifest sends the active locale for every lookup batch");
  holdDownload = true; await click(button("Download missing and enable"));
  await settle(() => pending.some((request) => request.command === "download_steam_workshop_items"), "Manifest installation was not held");
  await language("中文");
  assert(element<HTMLTextAreaElement>(".mw-manifest-input").disabled && button("核对清单").disabled,
    "A locale change keeps the pending installation locked until its own operation finishes");
  holdDownload = false; await releaseWhere((request) => request.command === "download_steam_workshop_items");
  await settle(() => writes === 1 && !element<HTMLTextAreaElement>(".mw-manifest-input").disabled, "Manifest installation did not save and release its own lock");
  assert(downloads === 1 && pending.length === 0, "Language switching neither duplicates installation nor leaves held requests");
  await click(element('[role="group"] button:last-child'));
  await settle(() => Boolean(fixture.querySelector(".mw-mod-thumb")), "Returning from the manifest did not show collections");
  await click(element(".mw-mod-thumb"));
  await settle(() => detailText().includes(chineseParent.description!), "Final collection details lost their Chinese presentation after canonical manifest lookups");
  assert(element(".mw-mod-card-title").textContent === chineseParent.title,
    "Canonical metadata from a completed manifest cannot replace the localized collection title");
  holdValidationBatch = true; newerClientOnlyDetails = true; epoch += 1;
  await act(async () => { draw(); });
  await settle(() => pending.some((request) => request.command === "lookup_steam_workshop_items" && request.ids.includes(configuredId)),
    "The earlier same-language metadata batch was not held");
  await settle(() => [...fixture.querySelectorAll(".mw-mod-card-title")].some((node) => node.textContent === "已添加模组·中文"),
    "The configured item was not present before its newer validation");
  const configuredCard = [...fixture.querySelectorAll<HTMLElement>(".mw-mod-card")].find((node) => node.querySelector(".mw-mod-card-title")?.textContent === "已添加模组·中文")!;
  await click(element(".mw-mod-thumb", configuredCard));
  await settle(() => detailText().includes(clientOnlyDescription), "The newer client-only details did not load");
  assert(detailText().includes("仅客户端") && element<HTMLButtonElement>(".mw-store-detail-metric").getAttribute("aria-label") === "管理",
    "A newer single-item validation marks a client-only Mod while retaining management of its existing instance entry");
  holdValidationBatch = false;
  await releaseWhere((request) => request.command === "lookup_steam_workshop_items" && request.ids.includes(configuredId));
  await settle(() => fixture.querySelector(".mw-mod-grid")?.getAttribute("aria-busy") === "false", "The released older metadata batch did not finish");
  assert(detailText().includes("仅客户端") && detailText().includes(clientOnlyDescription) &&
    ![...fixture.querySelectorAll(".mw-mod-card-title")].some((node) => node.textContent === "已添加模组·中文"),
    "An older same-language server-capable batch cannot overwrite newer client-only validation, restore its server catalog card or erase its localized description");
  await click(element(".mw-store-detail-metric"));
  assert(writes === 1 && downloads === 1 && pending.length === 0, "The validation race performs no installation and releases every held request");
  assert(!fixture.querySelector(".mw-store-detail") && button("我的 Mod").getAttribute("aria-selected") === "true",
    "Managing the existing client-only entry opens My Mods without downloading or changing settings");
  await click(button("本周热门"));
  await click(button("合集"));
  await settle(() => Boolean(fixture.querySelector(".mw-mod-thumb")), "Collection browse did not return after validation checks");
  await click(element(".mw-mod-thumb"));
  await settle(() => detailText().includes(chineseParent.description!), "Final Chinese collection details did not return");
  failOriginalLocalization = true;
  const cachedOriginalChild = [...fixture.querySelectorAll<HTMLButtonElement>(".mw-store-detail-child")]
    .find((node) => node.querySelector("small")?.textContent === originalId)!;
  await click(cachedOriginalChild);
  await settle(() => detailText().includes(originalDescription) &&
    fixture.querySelector(".shell-activity-bar")?.textContent?.includes("现显示作者原始文案") === true,
  "A cached item's failed refresh must keep readable original text and a retry notice");
  assert(element<HTMLButtonElement>(".mw-store-detail-metric").getAttribute("aria-label") === "管理",
    "A failed cached refresh preserves management of the already installed item without offering a new download");
  await act(async () => { element(".mw-store-detail-description").scrollIntoView({ block: "start" }); });
  assert(errors.length === 0, "Workshop language switching has no browser errors");
  return { status: "passed", checks, browser_errors: errors, requests, stale_requests_ignored: true, parent_language_preserved: true,
    author_text_preserved: true, manifest_review_isolated: true, pending_install_lock: true, latest_validation_preserved: true,
    localized_failure_retains_text: true, localized_retry_recovers: true, unverified_fallback_blocks_download: true,
    writes, downloads, viewport: { width: innerWidth, height: innerHeight } };
}
let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => { watchdog = setTimeout(() => reject(new Error("Workshop locale acceptance exceeded 40 seconds")), 40000); })])
  .catch((error) => ({ status: "failed", checks, error: String(error), browser_errors: errors }))
  .finally(() => { clearTimeout(watchdog); Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }); })
  .then((report) => fetch(`/__reliability_result/${new URLSearchParams(location.search).get("nonce")}`, { method: "POST", body: JSON.stringify(report) }));
