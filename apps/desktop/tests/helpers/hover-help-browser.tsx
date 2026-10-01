import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import React, { act, Profiler, StrictMode, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { ConfigurationHelp } from "../../src/views/settings/ConfigurationFieldHelp";
import type { ConfigurationSectionNode } from "../../src/views/settings/settings-schema";
import type { LocaleCode } from "../../src/i18n";
import "../../src/app.css";
import "../../src/views/servers/workbench/mods.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
document.documentElement.dataset.theme = "dark";
const nonce = new URLSearchParams(location.search).get("nonce");
const root = createRoot(document.getElementById("fixture")!);
const errors: string[] = [];
const results: { name: string; error?: string }[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
let revision = 0;
let unmounted = false;

function check(condition: unknown, description: string): asserts condition {
  if (!condition) throw new Error(description);
}
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const target = document.querySelector<T>(selector);
  check(target, `Missing ${selector}`);
  return target;
}
const bubbles = () => [...document.querySelectorAll<HTMLElement>(".configuration-field-help-tooltip")];
const painted = () => bubbles().filter((bubble) => Number(getComputedStyle(bubble).opacity) > 0.01);
async function pause(milliseconds: number) {
  await act(async () => { await new Promise((resolve) => setTimeout(resolve, milliseconds)); });
}
async function until(predicate: () => boolean, description: string, milliseconds = 1600) {
  const expires = performance.now() + milliseconds;
  while (!predicate()) {
    check(performance.now() < expires, `${description}; bubbles=${bubbles().map((bubble) => bubble.textContent?.slice(0, 80)).join(" | ")}`);
    await pause(10);
  }
}
async function opened(text: string) {
  await until(() => painted().length === 1 && painted()[0].textContent === text
    && getComputedStyle(painted()[0]).opacity === "1", `Expected one visible help: ${text.slice(0, 80)}`);
  return painted()[0];
}
async function closed(reason: string) {
  await until(() => bubbles().length === 0, reason, 800);
}
async function mouse(type: "move" | "click", x: number, y: number) {
  await act(async () => {
    const response = await fetch(`/__reliability_pointer/${nonce}`, {
      method: "POST", body: JSON.stringify({ type, x, y }),
    });
    check(response.ok, "Native mouse dispatch failed");
  });
}
async function point(target: HTMLElement, type: "move" | "click" = "move") {
  const rect = target.getBoundingClientRect();
  await mouse(type, rect.left + rect.width / 2, rect.top + rect.height / 2);
}
async function key(value: "Tab" | "Escape") {
  await act(async () => {
    const response = await fetch(`/__reliability_key/${nonce}/${value}`, { method: "POST" });
    check(response.ok, "Native key dispatch failed");
  });
}
async function outside() { await mouse("move", 900, 40); }

function Field({ id, children, button = false, description }: {
  id: string; children?: ReactNode; button?: boolean; description?: string;
}) {
  return <ConfigurationHelp description={description ?? `${id} help`}>{(help) => (
    <section id={id} ref={help.anchorRef} {...help.interactionProps} style={{ width: 360 }}>
      <label htmlFor={`${id}-control`} style={{ display: "block", lineHeight: "20px", marginBottom: 4 }}>{id}</label>
      <div className="configuration-field-control">
        {button ? <button id={`${id}-control`} className="secondary-button" aria-describedby={help.descriptionId}>Action</button>
          : <input id={`${id}-control`} className="settings-schema-input" aria-describedby={help.descriptionId}
            defaultValue={id} style={{ width: 320, height: 34 }} />}
      </div>
      {children}
    </section>
  )}</ConfigurationHelp>;
}
function Board() {
  return <main style={{ height: "100dvh", padding: 24 }}>
    <button id="outside" className="secondary-button">Other action</button>
    <div style={{ position: "absolute", left: 40, top: 110, display: "grid", gap: 28 }}>
      <Field id="alpha" />
      <Field id="beta" button />
      <Field id="gamma" />
      <Field id="parent"><div style={{ paddingTop: 20, paddingLeft: 20 }}><Field id="child" /></div></Field>
    </div>
    <div id="scroll-shell" style={{ position: "absolute", left: 550, top: 160, width: 380, height: 330, overflow: "auto" }}>
      <div style={{ paddingTop: 100, height: 750 }}><Field id="scrolled" /></div>
    </div>
  </main>;
}
async function reset() {
  await outside();
  await act(async () => { root.render(<StrictMode key={++revision}><Board /></StrictMode>); });
  await point(element("#outside"), "click");
  await outside();
  await closed("Reset must release the previous field");
}
async function scenario(name: string, run: () => Promise<void>) {
  try { await reset(); await run(); results.push({ name }); }
  catch (error) { results.push({ name, error: error instanceof Error ? error.message : String(error) }); }
}
async function observePaint(run: () => Promise<void>) {
  const seen = new Set<string>();
  let maximum = 0;
  let frame = 0;
  const sample = () => {
    const current = painted();
    maximum = Math.max(maximum, current.length);
    current.forEach((bubble) => seen.add(bubble.textContent ?? ""));
    frame = requestAnimationFrame(sample);
  };
  sample();
  try {
    await run();
    // A scenario can finish between samples after its final tooltip becomes visible.
    await act(async () => { await new Promise<void>((resolve) => requestAnimationFrame(() => resolve())); });
  } finally { cancelAnimationFrame(frame); }
  return { seen: [...seen], maximum };
}
async function lifecycle(event: () => void, reason: string) {
  await point(element("#alpha-control"));
  await opened("alpha help");
  await act(async () => { event(); });
  await closed(reason);
  await pause(260);
  check(bubbles().length === 0, `${reason}: stale input must not reopen help`);
}
async function pendingLifecycle(event: () => void, reason: string) {
  const observation = await observePaint(async () => {
    await point(element("#alpha-control"));
    await pause(55);
    check(bubbles().length === 0, "Lifecycle event must arrive before the hover entrance completes");
    await act(async () => { event(); });
    await pause(350);
  });
  check(observation.seen.length === 0 && bubbles().length === 0, reason);
  await outside();
  await point(element("#alpha-control"));
  await opened("alpha help");
}
async function workshop() {
  const [{ SteamWorkshopStoreDetail }, { I18nProvider }] = await Promise.all([
    import("../../src/views/servers/SteamWorkshopStoreDetail"), import("../../src/i18n"),
  ]);
  await act(async () => { root.render(<StrictMode><I18nProvider>
    <main style={{ height: "100dvh", padding: 24 }}>
      <button id="workshop-outside" className="secondary-button">Other action</button>
      <div style={{ position: "absolute", left: 40, top: 70, width: 370, height: 620, display: "grid" }}>
        <SteamWorkshopStoreDetail item={{ id: "1000001", title: "Hover lifecycle test item", item_kind: "item",
          status: "resolved", detail_url: "https://steamcommunity.com/sharedfiles/filedetails/?id=1000001",
          child_count: 0, children: [], file_size: 2048,
          tags: Array.from({ length: 100 }, (_, index) => `Server configuration category ${index + 1}`) }}
          lifecycleState="installing" action="install" actionBusy actionDisabled detailsReady progressPercent={42}
          onAction={() => { throw new Error("Disabled workshop action must not run"); }}
          onOpenChild={() => {}} onClose={() => {}} onOpenExternal={() => {}} />
      </div>
    </main>
  </I18nProvider></StrictMode>); });
  await until(() => Boolean(document.querySelector(".mw-store-detail-metric-help")), "Workshop details did not mount");
}
async function dstNavigation(locale: LocaleCode) {
  const [{ readModuleDetails }, { I18nProvider, useI18n }, { parseGuidedSettingsSchema },
    { buildConfigurationWorkspaceModel, configurationNavigationRoots }, { ConfigurationSectionNavigation }] = await Promise.all([
    import("../../src/api"), import("../../src/i18n"), import("../../src/views/settings/guided-settings"),
    import("../../src/views/settings/configuration-workspace-model"), import("../../src/views/settings/ConfigurationSectionNavigation"),
  ]);
  const descriptor = await readModuleDetails("dontstarve");
  let activeLocale: LocaleCode | null = null;
  let changeLocale: ((next: LocaleCode) => void) | undefined;
  let roots: ConfigurationSectionNode[] = [];
  function Navigation() {
    const { locale: currentLocale, setLocale, t } = useI18n();
    const [selected, setSelected] = React.useState<string | null>(null);
    roots = React.useMemo(() => configurationNavigationRoots(buildConfigurationWorkspaceModel(
      parseGuidedSettingsSchema(descriptor, currentLocale, t)).roots), [currentLocale, t]);
    activeLocale = currentLocale;
    changeLocale = setLocale;
    return <div id="dst-navigation" style={{ position: "absolute", left: 24, top: 24, width: 600, height: 660, overflow: "auto" }}>
      <ConfigurationSectionNavigation roots={roots} selectedSectionId={selected} ariaLabel="DST configuration categories"
        onSelectSection={setSelected} />
    </div>;
  }
  localStorage.setItem("langame.locale", "zh-CN");
  await act(async () => { root.render(<StrictMode><I18nProvider><Navigation /></I18nProvider></StrictMode>); });
  await until(() => Boolean(changeLocale), "DST navigation must load its real locale catalog");
  await act(async () => { changeLocale!(locale); });
  await until(() => activeLocale === locale, `DST navigation must switch to ${locale}`);
  await document.fonts.ready;
  const paths = new Map<string, ConfigurationSectionNode[]>();
  const visit = (nodes: ConfigurationSectionNode[], ancestors: ConfigurationSectionNode[] = []) => {
    for (const node of nodes) { const path = [...ancestors, node]; paths.set(node.id, path); visit(node.children, path); }
  };
  visit(roots);
  for (const id of ["cluster-runtime", "advanced", "cluster", "cluster-rules", "cluster-shard-coordination",
    "surface", "mastergen", "mastersettings", "caves", "cavesgen", "cavessettings"]) {
    const path = paths.get(id);
    check(path, `${locale}.${id}: category must exist in the real configuration model`);
    for (const ancestor of path.slice(0, -1)) {
      const toggle = document.querySelector<HTMLElement>(`[data-configuration-section-id="${ancestor.id}"] > button[aria-expanded="false"]`);
      if (toggle) { await act(async () => { toggle.scrollIntoView({ block: "center" }); }); await point(toggle, "click"); await outside(); }
    }
    const target = element(`[data-configuration-section-id="${id}"] > :is(.configuration-section-navigation__button,.configuration-section-navigation__group-label)`);
    await act(async () => { target.scrollIntoView({ block: "center" }); });
    await pause(20);
    const title = target.querySelector<HTMLElement>(".configuration-section-navigation__title")!;
    check(title.scrollWidth <= title.clientWidth, `${locale}.${id}: fixture must show the full category title`);
    await point(target);
    await until(() => painted().length === 1, `${locale}.${id}: untruncated category must show a help bubble`);
    const description = path[path.length - 1].description;
    check(description && description !== title.textContent, `${locale}.${id}: help must explain the category, not repeat its title`);
    await opened(description);
    if (target.classList.contains("configuration-section-navigation__button")) {
      await point(target, "click");
      check(target.getAttribute("aria-current") === "page", `${locale}.${id}: clicking must select the category`);
    }
    await outside();
    await closed(`${locale}.${id}: category help must close after moving away`);
    if (target.classList.contains("configuration-section-navigation__button")) {
      check(document.activeElement === target, `${locale}.${id}: closing help must preserve selection focus`);
    }
  }
}

Object.assign(window, { __reliabilityFixtureCleanup: async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  if (!unmounted) await act(async () => { root.unmount(); unmounted = true; });
  check(bubbles().length === 0, "Unmount must remove all tooltip portals");
  return { browser_errors: errors, native_dialogs: 0 };
} });

async function run() {
  await act(prepareBrowserLocaleCatalogs);
  await scenario("mouse click keeps input focus without pinning hover help", async () => {
    const input = element("#alpha-control");
    await point(input, "click");
    await opened("alpha help");
    check(document.activeElement === input, "Native click must focus the input");
    await outside();
    await closed("Help remained after a clicked input lost mouse hover");
    check(document.activeElement === input, "Dismissing mouse help must preserve input focus");
  });
  await scenario("mouse click keeps button focus without pinning hover help", async () => {
    const button = element("#beta-control");
    await point(button, "click");
    await opened("beta help");
    await outside();
    await closed("Help remained after a clicked button lost mouse hover");
    check(document.activeElement === button, "Dismissing mouse help must preserve button focus");
  });
  await scenario("brief pass over a field never paints help", async () => {
    const observation = await observePaint(async () => {
      await point(element("#alpha-control"));
      await pause(55);
      await outside();
      await pause(350);
    });
    check(observation.seen.length === 0, `Brief hover flashed: ${observation.seen.join(", ")}`);
    await closed("Cancelled entrance must not leave a portal");
  });
  await scenario("rapid sweep only reveals its final field", async () => {
    const observation = await observePaint(async () => {
      for (const id of ["alpha", "beta", "gamma"]) {
        await point(element(`#${id}-control`));
        if (id !== "gamma") await pause(45);
      }
      await opened("gamma help");
    });
    check(observation.seen.length === 1 && observation.seen[0] === "gamma help",
      `Passing fields flashed during sweep: ${observation.seen.join(", ")}`);
    check(observation.maximum === 1, "Sweeping must never stack visible bubbles");
  });
  await scenario("crossing the visual gap keeps help readable until the bubble is left", async () => {
    const anchor = element("#alpha");
    await point(element("#alpha-control"));
    const bubble = await opened("alpha help");
    const anchorRect = anchor.getBoundingClientRect();
    const bubbleRect = bubble.getBoundingClientRect();
    const x = Math.max(anchorRect.left, bubbleRect.left) + 12;
    const y = bubbleRect.top >= anchorRect.bottom ? (anchorRect.bottom + bubbleRect.top) / 2
      : (bubbleRect.bottom + anchorRect.top) / 2;
    await mouse("move", x, y);
    await pause(35);
    await point(bubble);
    await pause(350);
    check(painted().length === 1 && painted()[0].textContent === "alpha help", "Bubble disappeared during reading");
    await outside();
    await closed("Leaving the bubble must dismiss readable help");
  });
  await scenario("Tab focus opens help and Escape dismisses without moving focus", async () => {
    await key("Tab");
    const input = element("#alpha-control");
    check(document.activeElement === input, "Tab must reach the first input");
    await opened("alpha help");
    await key("Escape");
    await closed("Escape must dismiss keyboard help");
    await pause(260);
    check(document.activeElement === input && bubbles().length === 0, "Escape must retain focus without reopening help");
  });
  await scenario("Escape on an overlapping bubble stays dismissed until a genuine re-entry", async () => {
    const description = "Long help remains readable across the viewport. ".repeat(100).trim();
    await act(async () => { root.render(<StrictMode>
      <main style={{ position: "absolute", left: 40, top: 300 }}>
        <Field id="overlap" description={description} />
      </main>
    </StrictMode>); });
    const input = element("#overlap-control");
    await point(input);
    const bubble = await opened(description);
    await point(input);
    const rect = input.getBoundingClientRect();
    check(document.elementFromPoint(rect.left + rect.width / 2, rect.top + rect.height / 2) === bubble,
      "Escape fixture must place the bubble over the anchor's pointer location");
    await key("Escape");
    await closed("Escape must dismiss help while the mouse is inside its bubble");
    await point(input);
    await pause(400);
    check(bubbles().length === 0, "Revealing the anchor below a dismissed bubble must not reopen help");
    await outside();
    await point(input);
    await opened(description);
  });
  await scenario("Escape inside separate help permits a deliberate move back to its anchor", async () => {
    const input = element("#alpha-control");
    await point(input);
    const bubble = await opened("alpha help");
    await point(bubble);
    await key("Escape");
    await closed("Escape inside the readable bubble must dismiss it");
    await point(input);
    await opened("alpha help");
  });
  await scenario("returning during exit restores the readable bubble", async () => {
    const input = element("#alpha-control");
    await point(input);
    const bubble = await opened("alpha help");
    await outside();
    await until(() => bubble.isConnected && Number(getComputedStyle(bubble).opacity) < 0.95,
      "Help must begin fading after pointer exit", 500);
    await point(input);
    await opened("alpha help");
    check(bubbles()[0] === bubble, "A returning pointer should restore the still-mounted readable bubble");
    await pause(300);
    check(painted().length === 1, "A stale exit must not remove the restored bubble");
  });
  await scenario("switching open help never overlaps two painted bubbles", async () => {
    await point(element("#alpha-control"));
    await opened("alpha help");
    const observation = await observePaint(async () => {
      await point(element("#beta-control"));
      await opened("beta help");
    });
    check(observation.maximum === 1, "Help bubbles overlapped while switching fields");
  });
  await scenario("window blur clears active help", async () => {
    await lifecycle(() => window.dispatchEvent(new Event("blur")), "Window blur must clear mouse help");
  });
  await scenario("leaving the viewport clears active help", async () => {
    await lifecycle(() => document.documentElement.dispatchEvent(new PointerEvent("pointerout", {
      bubbles: true, relatedTarget: null, pointerType: "mouse",
    })), "Viewport exit must clear mouse help");
  });
  await scenario("pointer cancellation clears active help", async () => {
    await lifecycle(() => element("#alpha-control").dispatchEvent(new PointerEvent("pointercancel", {
      bubbles: true, pointerType: "mouse", pointerId: 1,
    })), "Pointer cancellation must clear mouse help");
  });
  await scenario("window blur cancels a pending entrance", async () => {
    await pendingLifecycle(() => window.dispatchEvent(new Event("blur")), "Blurred pending help must never appear later");
  });
  await scenario("pointer cancellation cancels a pending entrance", async () => {
    await pendingLifecycle(() => element("#alpha-control").dispatchEvent(new PointerEvent("pointercancel", {
      bubbles: true, pointerType: "mouse", pointerId: 1,
    })), "Cancelled pending help must never appear later");
  });
  await scenario("scrolling an anchor away from a stationary mouse clears help", async () => {
    const input = element("#scrolled-control");
    await point(input);
    await opened("scrolled help");
    const before = input.getBoundingClientRect();
    await act(async () => { element("#scroll-shell").scrollTop = 75; });
    const after = input.getBoundingClientRect();
    check(after.top > 0 && after.bottom < innerHeight && after.bottom < before.top,
      "Scroll fixture must move the anchor off the mouse while keeping it inside the viewport");
    await closed("Help remained after its visible anchor scrolled away from the mouse");
  });
  await scenario("nested fields own only their nearest help", async () => {
    await point(element("#parent-control"));
    await opened("parent help");
    await point(element("#child-control"));
    await opened("child help");
    await pause(300);
    check(painted().length === 1 && painted()[0].textContent === "child help", "Parent help must not replace child help");
    await outside();
    await closed("Nested help must close when leaving its whole region");
  });
  await scenario("stationary SectionNavigation help does not continually rerender", async () => {
    const { ConfigurationSectionNavigation } = await import("../../src/views/settings/ConfigurationSectionNavigation");
    let commits = 0;
    await act(async () => { root.render(<StrictMode>
      <div style={{ position: "absolute", left: 40, top: 110, width: 280 }}>
        <Profiler id="section-navigation" onRender={() => { commits++; }}>
          <ConfigurationSectionNavigation roots={[{ id: "navigation", title: "Server configuration",
            description: "Navigation help", order: 0, breadcrumb: ["Server configuration"], items: [],
            children: [], actionable: true }]} selectedSectionId={null} ariaLabel="Configuration sections"
            onSelectSection={() => {}} />
        </Profiler>
      </div>
    </StrictMode>); });
    await document.fonts.ready;
    await point(element(".configuration-section-navigation__button"));
    const bubble = await opened("Navigation help");
    await pause(200);
    const settledCommits = commits;
    await pause(350);
    check(commits - settledCommits <= 1,
      `Stationary navigation help kept committing React updates: ${commits - settledCommits}`);
    check(bubbles()[0] === bubble && painted().length === 1, "Stable navigation help must remain readable");
  });
  await scenario("disabled Workshop action retains hover and keyboard descriptions", async () => {
    await workshop();
    const button = element<HTMLButtonElement>(".mw-store-detail-metric:disabled");
    const wrapper = element(".mw-store-detail-metric-help");
    const description = button.getAttribute("aria-label")!;
    check(button.disabled && Boolean(description), "Workshop fixture needs a labelled disabled action");
    await point(button);
    await opened(description);
    await outside();
    await closed("Disabled action hover must dismiss");
    await act(async () => { element(".mw-store-detail-close").focus(); });
    await key("Tab");
    check(document.activeElement === wrapper, "Disabled action description must be keyboard reachable");
    await opened(description);
    await key("Escape");
    await closed("Escape must dismiss disabled action help");
    check(document.activeElement === wrapper, "Escape must preserve disabled action description focus");
  });
  await scenario("long Workshop tags stay bounded, scrollable and dismissible", async () => {
    await workshop();
    const tags = [...document.querySelectorAll<HTMLElement>(".mw-store-detail-metric")]
      .find((target) => target.getAttribute("aria-label")?.includes("Server configuration category 100"));
    check(tags, "Workshop tags trigger did not render");
    await point(tags);
    const bubble = await opened(tags.getAttribute("aria-label")!);
    const rect = bubble.getBoundingClientRect();
    check(rect.left >= 7 && rect.right <= innerWidth - 7 && rect.top >= 7 && rect.bottom <= innerHeight - 7,
      "Long Workshop tags must stay within the viewport");
    check(bubble.scrollWidth <= bubble.clientWidth + 1 && bubble.scrollHeight > bubble.clientHeight,
      "Long tags must wrap and provide vertical scrolling");
    await point(bubble);
    await act(async () => { bubble.scrollTop = bubble.scrollHeight; });
    await pause(350);
    check(bubble.scrollTop > 0 && painted().length === 1, "Workshop tags must remain readable while scrolled");
    await outside();
    await closed("Leaving the tags bubble must dismiss it");
  });
  for (const locale of ["zh-CN", "en-US"] as const) {
    await scenario(`all previously missing DST category bubbles work in ${locale}`, () => dstNavigation(locale));
  }
  await scenario("unmount cancels pending entrance and removes visible help", async () => {
    await point(element("#alpha-control"));
    await opened("alpha help");
    await point(element("#gamma-control"));
    await act(async () => { root.unmount(); unmounted = true; });
    await pause(350);
    check(bubbles().length === 0, "Unmount left a visible or delayed tooltip portal");
  });
  const failures = results.filter((result) => result.error);
  return { status: failures.length || errors.length ? "failed" : "passed", checks: results.length,
    error: failures.length ? `${results.length - failures.length}/${results.length} scenarios passed\n`
      + failures.map((failure) => `${failure.name}: ${failure.error}`).join("\n") : "", results,
    browser_errors: errors, pointer_events: "native CDP mouse move/click", keyboard_events: "native CDP Tab/Escape",
    lifecycle_events: "DOM blur/pointerout/pointercancel" };
}

void run().catch((error) => ({ status: "failed", checks: results.length,
  error: error instanceof Error ? error.stack : String(error), browser_errors: errors }))
  .then((report) => {
    Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false });
    return fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) });
  });
