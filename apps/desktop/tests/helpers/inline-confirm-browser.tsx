import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import React, { act, StrictMode, type ComponentProps, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { I18nProvider, translate } from "../../src/i18n";
import { InlineConfirmAction } from "../../src/components/InlineConfirmAction";
import { BackupRenameAction } from "../../src/components/BackupRenameAction";
import { LibraryServerActions } from "../../src/views/library/LibraryServerActions";
import type { ModuleProgramInventory } from "../../src/storage-management-types";
import type { ModuleSummary } from "../../src/types";
import "../../src/app.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN");
const parameters = new URLSearchParams(location.search);
const nonce = parameters.get("nonce");
document.documentElement.dataset.theme = parameters.get("theme") ?? "dark";
const errors: string[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
let nativeDialogs = 0;
const rejectNativeDialog = () => { nativeDialogs++; throw new Error("An action called a native browser dialog"); };
window.confirm = rejectNativeDialog;
window.prompt = rejectNativeDialog;
window.alert = rejectNativeDialog;
let checks = 0;
let calls = 0;
let root = createRoot(document.getElementById("fixture")!);
const fixture = document.getElementById("fixture")!;
const message = "要移除服务器文件吗？实例、存档和备份都会保留。";
const selected: ModuleSummary = { id: "fixture-one", name: "测试服务器", version: "1.0", steam_app_id: 123,
  install_state: "Installed", supported_platforms: ["windows"] };
const uninstalled: string[] = [];
const noOperation = () => {};
let inventoryReads = 0;
let activeInventoryReads = 0;
Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: { invoke: async (command: string, args: Record<string, unknown>) => {
  check(command === "inspect_module_programs", `Unexpected inline-confirm command: ${command}`);
  const input = args.input as { module_id?: string; program_mode?: string; program_source?: string } | undefined;
  check(input && ["fixture-one", "fixture-two"].includes(input.module_id ?? "")
    && input.program_mode === "independent" && input.program_source === "verified", "Unexpected inventory request");
  inventoryReads++;
  activeInventoryReads++;
  try {
    const inventory: ModuleProgramInventory = { requires_archive_inventory: false, installations: [{ id: 1, install_root: "fixture/library",
      scope: "library", install_state: "Installed", current_version: "fixture-version", used_by: [],
      modification_state: "unverified", pending_removal: false, size_bytes: 1024 }], creation: { can_create: true,
      action: "existing_install", program_path: "fixture/library", additional_bytes: 0, reason: null } };
    return await Promise.resolve(inventory);
  } finally { activeInventoryReads--; }
} } });

type FixtureCleanup = { browser_errors: string[]; native_dialogs: number; unmounted_fixtures: number };
type FixtureWindow = Window & { __reliabilityFixtureCleanup?: () => Promise<FixtureCleanup> };
let cleanup: Promise<FixtureCleanup> | undefined;
Object.assign(window, { __reliabilityFixtureCleanup: () => cleanup ??= (async () => {
  const children: FixtureCleanup[] = [];
  for (const frame of fixture.querySelectorAll("iframe")) {
    const teardown = (frame.contentWindow as FixtureWindow | null)?.__reliabilityFixtureCleanup;
    if (!teardown) { errors.push(`Missing cleanup for ${frame.title}`); continue; }
    try {
      const child = await teardown();
      children.push({ ...child, browser_errors: child.browser_errors.map((error) => `${frame.title}: ${error}`) });
    } catch (error) { errors.push(`${frame.title}: ${String(error)}`); }
  }
  await act(async () => { root.unmount(); });
  check(activeInventoryReads === 0, "Inventory requests remained active after unmount");
  return { browser_errors: [...errors, ...children.flatMap((child) => child.browser_errors)],
    native_dialogs: nativeDialogs + children.reduce((count, child) => count + child.native_dialogs, 0),
    unmounted_fixtures: 1 + children.reduce((count, child) => count + child.unmounted_fixtures, 0) };
})() });

function check(condition: unknown, description: string): asserts condition {
  if (!condition) throw new Error(description);
}
const pause = () => new Promise<void>((resolve) => setTimeout(resolve, 10));
async function settleUntil(predicate: () => boolean, description: string) {
  const deadline = performance.now() + 5000;
  while (!predicate()) {
    check(performance.now() < deadline, description);
    await act(pause);
  }
}
async function render(element: ReactNode) {
  await act(async () => { root.render(<StrictMode><I18nProvider><div data-ready>{element}</div></I18nProvider></StrictMode>); });
  await settleUntil(() => Boolean(fixture.querySelector("[data-ready]")), "Translated fixture did not mount");
  await settleUntil(() => translate("en-US", "common.cancel") === "Cancel", "Fallback language catalog did not finish loading");
}
async function settleInventory() {
  await settleUntil(() => fixture.querySelector(".library-program-inventory")?.getAttribute("aria-busy") === "false",
    "Library program inventory did not finish");
  check(inventoryReads > 0 && activeInventoryReads === 0 && !fixture.querySelector('.library-program-inventory [role="alert"]'),
    "Library program inventory must finish successfully before interaction or capture");
}
function Harness(props: Partial<ComponentProps<typeof InlineConfirmAction>>) {
  return <>
    <InlineConfirmAction id="trigger" className="secondary-button" scopeKey="instance-one"
      confirmation={message} onConfirm={() => { calls++; }} {...props}>卸载服务器文件</InlineConfirmAction>
    <button id="outside" type="button">外部操作</button>
  </>;
}
function LibraryHarness({ module = selected, busy = false }: { module?: ModuleSummary; busy?: boolean }) {
  return <div className="library-detail-page"><LibraryServerActions selected={module}
    steamCmdStatus={null} steamCmdBusy={false}
    selectedModuleDetails={null} installLabel="已安装" installBusy={busy} installStatusClass="is-success"
    creating={false} instanceName="合作世界" onInstall={noOperation} onCreateServer={async () => {}}
    onInstanceNameChange={noOperation} onUninstall={(id) => { uninstalled.push(id); }} /></div>;
}
function group() { return fixture.querySelector<HTMLElement>(".inline-confirm-review"); }
function trigger() { return fixture.querySelector<HTMLButtonElement>(".inline-confirm-action > button")!; }
function cancelButton() { return group()!.querySelector<HTMLButtonElement>("button")!; }
function confirmButton() { return group()!.querySelector<HTMLButtonElement>(".inline-confirm-submit")!; }
async function click(element: HTMLElement) { await act(async () => { element.click(); }); }
async function open() {
  await act(async () => { trigger().focus(); });
  await click(trigger());
  check(group(), "First click did not open inline confirmation");
}
async function key(name: "Tab" | "Escape" | "Enter") {
  await act(async () => {
    const response = await fetch(`/__reliability_key/${nonce}/${name}`, { method: "POST" });
    check(response.ok, `Native ${name} dispatch failed`);
  });
}
function deferred() {
  let resolve!: () => void;
  let reject!: (cause: Error) => void;
  const promise = new Promise<void>((accept, decline) => { resolve = accept; reject = decline; });
  return { promise, resolve, reject };
}

async function checkSharedAction() {
  await render(<Harness />);
  await open();
  check(calls === 0 && !document.querySelector("dialog"), "First click executed or opened a modal");
  check(document.activeElement === cancelButton(), "Cancel must receive focus after StrictMode replay");
  checks++;
  await click(cancelButton());
  check(!group() && calls === 0 && document.activeElement === trigger(), "Cancel must preserve action and restore trigger focus");
  checks++;
  await open();
  await key("Escape");
  check(!group() && calls === 0 && document.activeElement === trigger(), "Escape must cancel and restore trigger focus");
  checks++;
  await open();
  const outside = document.getElementById("outside")!;
  await act(async () => { outside.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true })); outside.focus(); });
  check(!group() && calls === 0 && document.activeElement === outside, "Outside click must dismiss without stealing focus");
  checks++;
  await open();
  await act(async () => { outside.focus(); });
  check(!group() && calls === 0, "Leaving the action by keyboard focus must dismiss it");
  checks++;
  await open();
  confirmButton().focus();
  await key("Enter");
  check(calls === 1 && !group(), "Native Enter must execute the focused confirmation exactly once");
  check(document.activeElement === trigger(), "Successful keyboard confirmation must restore trigger focus");
  checks++;

  const pending = deferred();
  await render(<Harness onConfirm={() => { calls++; return pending.promise; }} />);
  await open();
  const submit = confirmButton();
  await act(async () => { submit.click(); submit.click(); });
  check(calls === 2 && submit.disabled && cancelButton().disabled && group()!.getAttribute("aria-busy") === "true",
    "Repeated clicks while pending must execute once and disable controls");
  await render(<Harness disabled onConfirm={() => { calls++; return pending.promise; }} />);
  check(group()?.getAttribute("aria-busy") === "true" && confirmButton().disabled && cancelButton().disabled,
    "Execution-driven disabled state must retain the visible pending review");
  await key("Escape");
  check(Boolean(group()), "Pending work must not masquerade as cancelled");
  await act(async () => { pending.resolve(); });
  check(!group(), "Completed action must close inline review");
  checks++;

  await render(<Harness />);
  await open();
  await render(<Harness scopeKey="instance-two" />);
  check(!group() && calls === 2, "Changing scope must discard pending confirmation");
  await render(<Harness />);
  check(!group(), "Returning to the previous scope must not revive confirmation");
  checks++;
  await open();
  await render(<Harness confirmation="新的操作范围" />);
  check(!group() && calls === 2, "Changing confirmation text must discard pending confirmation");
  await render(<Harness />);
  check(!group(), "Returning to previous text must not revive confirmation");
  checks++;
  await open();
  await render(<Harness disabled />);
  check(!group() && trigger().disabled, "Disabling an action must discard its pending confirmation");
  await render(<Harness />);
  check(!group() && !trigger().disabled, "Re-enabling must require a fresh confirmation");
  checks++;
  await open();
  const detachedSubmit = confirmButton();
  await act(async () => { root.unmount(); detachedSubmit.click(); });
  check(calls === 2, "Unmounted confirmation must not execute");
  root = createRoot(fixture);
  checks++;

  let shouldReject = true;
  await render(<Harness onConfirm={async () => { calls++; if (shouldReject) throw new Error("fixture write failed"); }} />);
  await open();
  await click(confirmButton());
  check(group()?.querySelector('[role="alert"]')?.textContent === "fixture write failed" && !confirmButton().disabled,
    "Rejected action must show an accessible error and allow retry");
  shouldReject = false;
  await click(confirmButton());
  check(!group() && calls === 4, "Retry must invoke the action again and close on success");
  checks++;

  let formSubmits = 0;
  await render(<form onSubmit={(event) => { event.preventDefault(); formSubmits++; }}>
    <input id="form-input" aria-label="实例名" />
    <InlineConfirmAction type="submit" scopeKey="form" confirmation={message}
      onConfirm={() => { calls++; }}>删除</InlineConfirmAction>
  </form>);
  document.getElementById("form-input")!.focus();
  await key("Enter");
  check(group() && calls === 4 && formSubmits === 0, "Implicit form submission must only open confirmation");
  await click(cancelButton());
  checks++;

  const stale = deferred();
  await render(<Harness onConfirm={() => { calls++; return stale.promise; }} />);
  await open();
  await click(confirmButton());
  await render(<Harness scopeKey="instance-two" />);
  await act(async () => { stale.reject(new Error("Previous instance failed")); });
  check(!group() && !trigger().disabled, "A previous scope's completion must release pending state without reopening review");
  await open();
  check(!group()!.querySelector('[role="alert"]'), "A late rejection must not leak its error into the new scope");
  await click(cancelButton());
  checks++;
}

async function checkLibraryAction() {
  await render(<LibraryHarness />);
  await settleInventory();
  await open();
  check(group()!.textContent?.includes("独立实例、个人数据和归档依赖的程序会保留"), "Real uninstall must explain retained data");
  check(uninstalled.length === 0, "Real uninstall first click must not invoke backend callback");
  await click(cancelButton());
  check(uninstalled.length === 0, "Cancelling uninstall must preserve server files");
  checks++;
  await open();
  await click(confirmButton());
  check(uninstalled.length === 1 && uninstalled[0] === "fixture-one", "Uninstall must forward the selected module exactly once");
  checks++;
  await open();
  await render(<LibraryHarness module={{ ...selected, id: "fixture-two", name: "另一服务器" }} />);
  await settleInventory();
  check(!group() && uninstalled.length === 1, "Changing games must cancel the previous uninstall confirmation");
  checks++;
  await open();
  await render(<LibraryHarness module={{ ...selected, id: "fixture-two", name: "另一服务器" }} busy />);
  check(!group() && trigger().disabled && uninstalled.length === 1, "A busy game must cancel and disable uninstall");
  checks++;
}

async function checkPreparedAction() {
  let resolve!: (value: string) => void;
  let reject!: (error: Error) => void;
  let reads = 0;
  let removals = 0;
  const prepareConfirmation = () => {
    reads++;
    return new Promise<string>((yes, no) => { resolve = yes; reject = no; });
  };
  await render(<Harness prepareConfirmation={prepareConfirmation} onConfirm={() => { removals++; }} />);
  await open();
  check(confirmButton().disabled && !cancelButton().disabled && group()?.getAttribute("aria-busy") === "true",
    "Ownership inspection blocks deletion while allowing cancellation");
  await click(confirmButton());
  check(removals === 0 && reads === 1, "Unresolved inspection cannot execute deletion");
  checks++;
  await click(cancelButton());
  await act(async () => { resolve("Discarded ownership result"); });
  check(!group() && removals === 0, "Cancelled inspection cannot reopen its confirmation");
  checks++;
  await open();
  await act(async () => { reject(new Error("Ownership inspection failed")); });
  check(confirmButton().disabled && group()?.textContent?.includes("Ownership inspection failed"),
    "Inspection failure remains visible and cannot authorize deletion");
  const retry = [...group()!.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.textContent === "重试")!;
  await click(retry);
  check(reads === 3 && confirmButton().disabled, "Retry performs a fresh ownership inspection");
  await act(async () => { resolve("Delete fixture/instance; keep fixture/library"); });
  check(!confirmButton().disabled && group()?.textContent?.includes("keep fixture/library"), "Prepared confirmation shows the actual retained installation");
  checks++;
  await click(confirmButton());
  check(removals === 1 && !group(), "Only explicit confirmation of the prepared range deletes the instance");
  checks++;
  await open();
  await render(<Harness scopeKey="another-instance" prepareConfirmation={prepareConfirmation} />);
  await act(async () => { resolve("Stale previous instance range"); });
  check(!group(), "Selection changes discard late ownership inspection results");
  checks++;
}

async function checkRenameAction() {
  const renamed: string[] = [];
  await render(<BackupRenameAction name="世界快照" onRename={async (name) => { renamed.push(name); return true; }} />);
  await click(fixture.querySelector("button")!);
  let input = fixture.querySelector<HTMLInputElement>("input")!;
  check(input.value === "世界快照" && document.activeElement === input, "Backup rename must open a focused inline input");
  await key("Escape");
  check(!fixture.querySelector("input") && renamed.length === 0 && document.activeElement === fixture.querySelector("button"),
    "Cancelling backup rename must not rename and must restore focus");
  checks++;
  await click(fixture.querySelector("button")!);
  input = fixture.querySelector<HTMLInputElement>("input")!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "合作世界快照");
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await key("Enter");
  check(renamed.length === 1 && renamed[0] === "合作世界快照" && !fixture.querySelector("input"),
    "Native Enter must save the edited backup name once");
  checks++;
}

async function preview() {
  await act(prepareBrowserLocaleCatalogs);
  document.body.style.margin = "0";
  await render(<div style={{ padding: 16 }}><LibraryHarness /></div>);
  await settleInventory();
  await open();
  const bounds = group()!.getBoundingClientRect();
  check(bounds.left >= 0 && bounds.right <= innerWidth, "Inline confirmation overflows the sidebar width");
  for (const element of group()!.querySelectorAll<HTMLElement>("button, .inline-confirm-message")) {
    const rect = element.getBoundingClientRect();
    check(rect.width > 0 && rect.left >= bounds.left - 1 && rect.right <= bounds.right + 1,
      "Confirmation text or controls overflow their real component");
  }
  const submitStyle = getComputedStyle(confirmButton());
  const cancelStyle = getComputedStyle(cancelButton());
  check(submitStyle.color !== cancelStyle.color && submitStyle.backgroundColor !== cancelStyle.backgroundColor,
    "Destructive confirmation must retain distinct danger text and background in either theme");
  // Each preview owns its focus-driven help lifecycle. Settle it before another
  // iframe takes focus or the parent captures and unmounts this preview.
  await act(async () => { (document.activeElement as HTMLElement | null)?.blur(); });
  await settleUntil(() => !document.querySelector(".configuration-field-help-tooltip"), "Preview help did not finish closing");
  check(errors.length === 0 && nativeDialogs === 0, `Preview errors: ${errors.join("; ")}`);
  document.documentElement.dataset.preview = "passed";
}

async function previews() {
  const variants = ["dark-510", "dark-300", "light-510", "light-300"];
  await render(<main style={{ display: "grid", gridTemplateColumns: "510px 300px", gap: "16px 24px", padding: 24 }}>
    {variants.map((variant) => {
      const [theme, width] = variant.split("-");
      return <section key={variant} style={{ width: Number(width) }}>
        <h2 style={{ fontSize: 14, margin: "0 0 8px" }}>{theme} · {width} px</h2>
        <iframe title={variant} src={`./inline-confirm-browser.html?preview=1&theme=${theme}`}
          style={{ display: "block", width: "100%", height: 350, border: "1px solid var(--border)" }} />
      </section>;
    })}
  </main>);
  await settleUntil(() => [...document.querySelectorAll("iframe")].every((frame) => frame.contentDocument?.documentElement.dataset.preview === "passed"),
    "Theme and sidebar-width previews did not finish");
  checks++;
  check(document.documentElement.scrollWidth <= innerWidth, "Preview matrix overflows the desktop viewport");
  checks++;
  return variants;
}

async function run() {
  await act(prepareBrowserLocaleCatalogs);
  await checkSharedAction();
  await checkPreparedAction();
  await checkLibraryAction();
  await checkRenameAction();
  const variants = await previews();
  check(errors.length === 0 && nativeDialogs === 0, `Browser errors: ${errors.join("; ")}`);
  checks++;
  return { status: "passed", checks, browser_errors: errors, native_dialogs: nativeDialogs, preview_variants: variants };
}

if (parameters.has("preview")) {
  void preview().catch((error) => { document.documentElement.dataset.preview = String(error); });
} else {
  let watchdog: ReturnType<typeof setTimeout>;
  void Promise.race([run(), new Promise<never>((_, reject) => {
    watchdog = setTimeout(() => reject(new Error(`Interaction stalled after ${checks} completed checks`)), 20_000);
  })]).finally(() => clearTimeout(watchdog))
    .catch((error) => ({ status: "failed", checks, error: `After ${checks} checks: ${error instanceof Error ? error.stack : String(error)}`, browser_errors: errors }))
    .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
}
