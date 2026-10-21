import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import React, { act, useEffect, useRef } from "react";
import { createRoot } from "react-dom/client";
import { invokeMock } from "../../src/api-mock";
import { I18nProvider } from "../../src/i18n";
import type { BootstrapResponse, InstanceDetails } from "../../src/types";
import { DST_REGENERATION_PENDING_RUN, DstWorldRegeneration, useDstWorldRegeneration } from "../../src/views/servers/DstWorldRegeneration";
import { InstanceSettingsSaveProvider, useInstanceSettingsSaveCoordinator } from "../../src/views/settings/InstanceSettingsSaveContext";
import "../../src/app.css";
import "../../src/views/servers/workbench.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN");
document.documentElement.dataset.theme = "dark";
const nonce = new URLSearchParams(location.search).get("nonce");
const fixture = document.getElementById("fixture")!, root = createRoot(fixture);
const errors: string[] = [], scenarios: string[] = [];
addEventListener("error", event => errors.push(event.message));
addEventListener("unhandledrejection", event => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
let checks = 0, original: InstanceDetails;
const events: string[] = [], freshRuns: (number | null)[] = [], busyStates: boolean[] = [];
let release: () => void = () => undefined, failure: string | null = null;
function check(value: unknown, message: string): asserts value { if (!value) throw new Error(message); checks++; }
const regenerations = () => events.filter(event => event.startsWith("regenerate:"));
const tool = () => fixture.querySelector<HTMLButtonElement>('button[aria-label="重新生成世界"]')!;
async function wait(predicate: () => boolean, message: string) {
  const deadline = performance.now() + 6000;
  while (!predicate()) {
    if (performance.now() > deadline) throw new Error(`${message}\n${fixture.textContent}`);
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 20)); });
  }
}
async function press(selector: string) {
  await act(async () => fixture.querySelector<HTMLButtonElement>(selector)!.click());
}

function SaveEditors({ id }: { id: string }) {
  const coordinator = useInstanceSettingsSaveCoordinator(), owner = useRef({});
  useEffect(() => {
    const live = coordinator.register(id, async () => { events.push("before-stop"); }, owner.current, { beforeStop: true });
    return () => live.detach(Promise.resolve());
  }, [coordinator, id]);
  return null;
}
/** A sibling map view reading the session store, as the workbench does. */
function MapProbe({ id }: { id: string }) {
  const state = useDstWorldRegeneration(id);
  useEffect(() => { freshRuns.push(state.freshFromRunId); busyStates.push(state.busy); }, [state]);
  return null;
}
async function render(id = "dst-world") {
  const details = { ...original, summary: { ...original.summary, id, name: `Fixture ${id}` } } as InstanceDetails;
  await act(async () => root.render(<I18nProvider><InstanceSettingsSaveProvider>
    <main style={{ padding: 24 }}><SaveEditors id={id} /><MapProbe id={id} /><div className="dst-gm-tool-grid">
      <DstWorldRegeneration details={details} /></div></main>
  </InstanceSettingsSaveProvider></I18nProvider>));
  await wait(() => Boolean(fixture.querySelector("main")), "The localized fixture did not render");
}

// Only IPC is replaced; the confirmation, save barrier and component are production code.
Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: { invoke: async (command: string, args: Record<string, unknown>) => {
  if (command !== "regenerate_dontstarve_world") throw new Error(`Unexpected native IPC: ${command}`);
  events.push(`regenerate:${JSON.stringify(args)}`);
  await new Promise<void>(resolve => { release = resolve; });
  if (failure) throw new Error(failure);
  return { run_id: 18 };
} } });

async function run() {
  await act(prepareBrowserLocaleCatalogs);
  const bootstrap = await invokeMock<BootstrapResponse>("bootstrap", { includeSystemSnapshot: false });
  const instance = bootstrap.state.instances.find(value => value.module_id === "dontstarve")!;
  original = await invokeMock<InstanceDetails>("read_instance_details_from_storage", { instanceId: instance.id });

  scenarios.push("icon-confirm-cancel"); await render();
  check(tool()?.textContent?.trim() === "重抽" && tool().title === "重新生成世界" && Boolean(tool().querySelector("svg"))
    && tool().classList.contains("dst-gm-tool"), `Regeneration is a GM tool button: refresh icon and 重抽, named by aria-label and title: ${tool()?.outerHTML}`);
  check(!tool().disabled && !tool().hasAttribute("aria-describedby"), "An idle regeneration has no disabled reason");
  check(fixture.querySelectorAll("button").length === 1, "There is no preview, backup or recovery action");
  await act(async () => tool().click());
  check(fixture.querySelector(".inline-confirm-message")?.textContent === "重新生成世界将丢失当前世界，是否确认？", "One plain confirmation explains the loss");
  check(Boolean(fixture.querySelector(".dst-gm-confirm .inline-confirm-submit")), "The confirmation opens over the tool rail");
  const cancel = [...fixture.querySelectorAll<HTMLButtonElement>(".inline-confirm-review button")].find(button => button.textContent === "取消");
  await act(async () => cancel!.click());
  check(!fixture.querySelector(".inline-confirm-review") && events.length === 0, "Cancelling does nothing");

  scenarios.push("confirm-runs-in-background");
  await act(async () => tool().click()); await press(".inline-confirm-submit");
  await wait(() => regenerations().length === 1, "Confirming must request regeneration");
  check(events[0] === "before-stop" && regenerations()[0] === `regenerate:${JSON.stringify({ instanceId: "dst-world" })}`,
    "Live edits are saved before the backend stops, deletes and starts");
  check(!fixture.querySelector(".inline-confirm-review"), "The confirmation closes while the backend works");
  check(freshRuns.at(-1) === DST_REGENERATION_PENDING_RUN && busyStates.at(-1) === true, "The old map is invalid as soon as regeneration begins");
  await render("dst-world-other");
  check(!tool().disabled, "Regeneration state is per instance");
  await render();
  check(tool().disabled && freshRuns.at(-1) === DST_REGENERATION_PENDING_RUN, "Reopening the view mid-regeneration keeps the busy state and the stale-map guard");
  const reasonId = tool().getAttribute("aria-describedby");
  check(tool().disabled && reasonId && document.getElementById(reasonId)?.textContent?.includes("进度见底部动态栏") === true,
    "A running regeneration disables the button and points at the activity bar");
  check(tool().title.includes("进度见底部动态栏"), "The disabled reason is also the button's title");
  check(!fixture.textContent?.includes("正在停服") && !fixture.textContent?.includes("正在生成新世界"), "Stages belong to the activity bar");
  await act(async () => tool().click());
  check(regenerations().length === 1, "A disabled button cannot start a second regeneration");
  await act(async () => release());
  await wait(() => !tool().disabled, "Completion re-enables regeneration");
  check(freshRuns.at(-1) === 18 && busyStates.at(-1) === false && !tool().hasAttribute("aria-describedby"), "The map refreshes from the new run");

  scenarios.push("failure-visible"); failure = "Fixture stop failed";
  await act(async () => tool().click()); await press(".inline-confirm-submit");
  await wait(() => regenerations().length === 2, "A second confirmation regenerates again");
  await act(async () => release());
  await wait(() => fixture.textContent?.includes("Fixture stop failed") === true, "A failure is shown");
  await wait(() => !tool().disabled, "A failed regeneration can be retried");
  check(freshRuns.at(-1) === null && busyStates.at(-1) === false, "A failure leaves no fresh-run guard behind");

  scenarios.push("lan-hidden"); await act(async () => root.render(null));
  const savedToken = sessionStorage.getItem("langameLanToken"), savedFetch = window.fetch;
  let lanCalls = 0;
  try {
    Object.assign(window, { isTauri: false }); sessionStorage.setItem("langameLanToken", "fixture-local-only-check");
    window.fetch = async (input, init) => {
      if (String(input).includes("/__langame/api")) { lanCalls++; throw new Error("A local-only world action attempted LAN IPC"); }
      return savedFetch(input, init);
    };
    await render();
    check(!tool() && lanCalls === 0, "LAN management must not expose local-only regeneration");
  } finally {
    await act(async () => root.render(null));
    window.fetch = savedFetch; Object.assign(window, { isTauri: true });
    if (savedToken === null) sessionStorage.removeItem("langameLanToken"); else sessionStorage.setItem("langameLanToken", savedToken);
  }
  await act(async () => root.unmount());
  check(errors.length === 0, `Browser errors: ${errors.join("; ")}`);
  return { status: "passed", checks, scenarios, browser_errors: errors, native_server_mutated: false,
    fixture_provenance: "Production regeneration component, InlineConfirmAction and save coordinator; synthetic IPC only" };
}
void run().catch(error => ({ status: "failed", checks, scenarios, error: error instanceof Error ? error.stack : String(error), browser_errors: errors }))
  .finally(() => Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }))
  .then(report => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
