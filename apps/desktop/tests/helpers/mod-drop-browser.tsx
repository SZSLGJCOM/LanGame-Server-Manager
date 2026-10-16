import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { readInstanceDetails } from "../../src/api";
import { buildMockModuleDetails } from "../../src/api-mock/module-details";
import { preventUnhandledDropNavigation } from "../../src/desktop-file-drop";
import type { InstanceDetails, ManualModInventoryResult, ModuleDetails } from "../../src/types";
import { createModWorkbenchFixtureIpc, ModWorkbenchBrowserHost } from "./mod-workbench-browser-support";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "en-US");
const fixture = document.getElementById("fixture")!;
fixture.style.cssText = "height:100vh;padding:16px;box-sizing:border-box;display:flex;flex-direction:column";
const root = createRoot(fixture);
const modules = ["abioticfactor", "arksurvivalascended", "astroneer", "corekeeper", "enshrouded", "humanitz",
  "minecraft", "necesse", "rust", "satisfactory", "sevendaystodie", "sonsoftheforest", "valheim", "vrising", "windrose"];
const checks: string[] = [], errors: string[] = [], covered: string[] = [];
let details: InstanceDetails, moduleDetails: ModuleDetails, inventory: ManualModInventoryResult;
let epoch = 0, stages = 0, references = 0, saves = 0, inventoryReads = 0;
let lastReferences: string[] = [];
let referenceFailure: string | null = null;
let expectedSourcePaths = ["C:\\Fixture\\mod.zip", "C:\\Fixture\\Folder\u3000"];
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
function check(condition: unknown, label: string): asserts condition {
  if (!condition) throw new Error(`${moduleDetails?.summary.id}: ${label}`);
  checks.push(label);
}
async function settle(predicate: () => boolean) {
  const deadline = performance.now() + 5000;
  while (!predicate()) {
    check(performance.now() < deadline, "Drop operation completes within the fixture deadline");
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
async function mount() {
  epoch++;
  await act(async () => root.render(<ModWorkbenchBrowserHost details={details} moduleDetails={moduleDetails}
    epoch={epoch} onSaved={() => { saves++; }} />));
}
async function drop(transfer: DataTransfer, allowed = true, effectAllowed = "copyLink", expectedEffect = allowed ? "copy" : "none") {
  const zone = fixture.querySelector<HTMLElement>(".mw-dropzone")!;
  check(Boolean(zone), "Drop zone is present");
  // Supply the external source's operation mask. Constructed DragEvents do not
  // model the OLE source or its cursor; this remains a React DOM regression.
  Object.defineProperty(transfer, "effectAllowed", { configurable: true, value: effectAllowed });
  Object.defineProperty(transfer, "dropEffect", { configurable: true, writable: true, value: "none" });
  const entered = new DragEvent("dragenter", { bubbles: true, cancelable: true, dataTransfer: transfer });
  const over = new DragEvent("dragover", { bubbles: true, cancelable: true, dataTransfer: transfer });
  const dropped = new DragEvent("drop", { bubbles: true, cancelable: true, dataTransfer: transfer });
  await act(async () => { zone.dispatchEvent(entered); });
  check(entered.defaultPrevented && transfer.dropEffect === expectedEffect
    && zone.classList.contains("mw-dropzone--active") === allowed, "Dragenter feedback respects the source mask and eligibility");
  await act(async () => { zone.dispatchEvent(over); });
  check(over.defaultPrevented && zone.classList.contains("mw-dropzone--active") === allowed,
    "Drag feedback matches current write eligibility");
  check(transfer.dropEffect === expectedEffect, `Drag feedback respects the source's ${effectAllowed} mask`);
  await act(async () => { zone.dispatchEvent(dropped); });
  check(dropped.defaultPrevented, "Handled drops do not navigate the page");
  check(transfer.dropEffect === expectedEffect, "Drop feedback agrees with dragover");
}
function stageResult() {
  return { instance_id: details.summary.id, module_id: details.summary.module_id, source_label: "Fixture",
    target_label: "Mods", target_path: inventory.target_path, affected_root_names: [], items: [], copied_file_count: 1, copied_total_bytes: 8 };
}
async function run() {
  await act(prepareBrowserLocaleCatalogs);
  const template = await readInstanceDetails("srv-dst-terminal-error");
  mockWindows("mod-drop-fixture");
  const base = createModWorkbenchFixtureIpc(() => ({ details, moduleDetails, inventory, catalog: [], cached: new Set() }), {
    check, save: (saved) => { details = saved; }
  });
  mockIPC(async (command, payload) => {
    const args = payload as { instanceId: string; references: string[]; sourcePaths: string[] };
    if (command === "read_manual_mod_inventory") inventoryReads++;
    if (["stage_manual_mod_files", "install_manual_mod_references", "resolve_manual_mod_references"].includes(command)) {
      check(args.instanceId === details.summary.id, "Drop mutation stays scoped to its current instance");
      if (command === "stage_manual_mod_files") {
        check(args.sourcePaths?.length === expectedSourcePaths.length &&
          args.sourcePaths.every((path, index) => path === expectedSourcePaths[index]),
          "Native paths preserve the selected files, directories and significant trailing Unicode spaces");
        stages++; return stageResult();
      }
      references++; lastReferences = args.references;
      if (referenceFailure) {
        const message = referenceFailure; referenceFailure = null;
        throw new Error(message);
      }
      if (command === "install_manual_mod_references") return stageResult();
      return { instance_id: details.summary.id, module_id: details.summary.module_id, source_label: "Fixture",
        setting_key: moduleDetails.mods!.enablement!.setting_key, setting_label: "Mod IDs", items: [], resolved_ids: ["424242"] };
    }
    return base(command, payload);
  }, { shouldMockEvents: true });
  Object.assign(window, { isTauri: true });
  const cleanup = preventUnhandledDropNavigation();
  try {
    for (const id of modules) {
      moduleDetails = buildMockModuleDetails(id);
      details = { ...template, summary: { ...template.summary, id: `drop-${id}`, module_id: id, status: "Stopped", active_process_count: 0 },
        active_run: null, settings_json: "{}" };
      inventory = { instance_id: details.summary.id, module_id: id, source_label: "Fixture", target_label: "Mods",
        target_path: `C:\\Fixture\\${id}\\Mods`, target_exists: false, items: [] };
      const readsBefore = inventoryReads;
      await mount(); await settle(() => inventoryReads > readsBefore && Boolean(fixture.querySelector(".mw-dropzone")));
      const file = new File(["fixture"], "mod.zip");
      Object.defineProperty(file, "path", { value: "C:\\Fixture\\mod.zip" });
      const fileDrop = new DataTransfer(); fileDrop.items.add(file);
      const folder = new File([], "Folder\u3000");
      Object.defineProperty(folder, "path", { value: "C:\\Fixture\\Folder\u3000" });
      fileDrop.items.add(folder);
      fileDrop.setData("text/plain", "https://example.invalid/incidental-file-text");
      const previousStage = stages, previousReferences = references;
      await drop(fileDrop); await settle(() => stages === previousStage + 1);
      check(references === previousReferences, "File drop wins over incidental URL payload");
      const supportsReferences = Boolean(moduleDetails.mods?.enablement || moduleDetails.mods?.source?.provider === "thunderstore");
      if (supportsReferences) for (const [type, text] of [
        ["text/plain", "424242"], ["text/uri-list", "https://example.invalid/mod/424242"],
        ["text/html", '<a href="https://example.invalid/mod/424242">Fixture</a>']
      ]) {
        const data = new DataTransfer(); data.setData(type, text);
        const previous = references;
        await settle(() => !fixture.querySelector('[aria-disabled="true"].mw-dropzone'));
        await drop(data); await settle(() => references === previous + 1);
        check(lastReferences.length === 1 && lastReferences[0] === (type === "text/plain" ? "424242" : "https://example.invalid/mod/424242"),
          `${id}: ${type} arrives at its declared reference provider`);
      }
      if (supportsReferences) {
        const link = new DataTransfer(); link.setData("text/uri-list", "https://example.invalid/mod/515151");
        const previous = references;
        await settle(() => !fixture.querySelector('[aria-disabled="true"].mw-dropzone'));
        await drop(link, true, "link", "link"); await settle(() => references === previous + 1);
        check(lastReferences[0] === "https://example.invalid/mod/515151", "A link-only source reaches the reference provider");
        if (moduleDetails.mods?.source?.provider === "thunderstore") {
          const packageUrl = `${moduleDetails.mods.source.url}p/FixtureAuthor/FixturePlugin/`;
          for (const format of ["text/uri-list", "text/html"]) {
            const protocol = "ror2mm://v1/install/thunderstore.io/FixtureAuthor/FixturePlugin/1.2.3/";
            const button = new DataTransfer();
            button.setData(format, format === "text/html" ? `<a href="${protocol}">Install with Mod Manager</a>` : protocol);
            const before = references, stagedBefore = stages;
            await settle(() => !fixture.querySelector('[aria-disabled="true"].mw-dropzone'));
            await drop(button, true, "link", "link");
            await settle(() => references === before + 1);
            check(stages === stagedBefore && lastReferences.length === 1
              && lastReferences[0] === "https://thunderstore.io/package/FixtureAuthor/FixturePlugin/1.2.3/",
            `${id}: Mod Manager button ${format} reaches the shared pinned package installer`);
          }
          for (const [name, type, mask, format] of [
            ["Jotunn.url", "application/internet-shortcut", "link", "text/uri-list"],
            ["Jotunn.url", "", "copyLink", "text/plain"],
            ["icon.png", "image/png", "copyLink", "text/uri-list"]
          ]) {
            const mixed = new DataTransfer();
            mixed.items.add(new File(["browser link artifact"], name, { type }));
            mixed.setData(format, packageUrl);
            if (format === "text/uri-list") mixed.setData("text/plain", "https://example.invalid/incidental-thumbnail.png");
            mixed.setData("text/html", `<a href="${packageUrl}"><img src="https://example.invalid/icon.png"></a>`);
            const before = references, stagedBefore = stages;
            await settle(() => !fixture.querySelector('[aria-disabled="true"].mw-dropzone'));
            await drop(mixed, true, mask, mask === "link" ? "link" : "copy");
            await settle(() => references === before + 1);
            check(stages === stagedBefore && lastReferences.length === 1 && lastReferences[0] === packageUrl,
              `Browser ${name} with a package URL reaches Thunderstore without importing its virtual file`);
          }
          const htmlLink = new DataTransfer();
          htmlLink.setData("text/html", `<a href="${packageUrl}"><img src="https://example.invalid/icon.png"></a>`);
          const before = references;
          await settle(() => !fixture.querySelector('[aria-disabled="true"].mw-dropzone'));
          await drop(htmlLink); await settle(() => references === before + 1);
          check(lastReferences.length === 1 && lastReferences[0] === packageUrl,
            "HTML-only package links exclude thumbnail URLs from the installation request");
          {
            const directory = new File([], "Directory.url");
            Object.defineProperty(directory, "path", { value: "C:\\Fixture\\Directory.url" });
            const directoryDrop = new DataTransfer(); directoryDrop.items.add(directory);
            Object.defineProperty(directoryDrop, "items", { value: [{ kind: "file", type: "",
              getAsFile: () => directory, webkitGetAsEntry: () => ({ isDirectory: true }) }] });
            const image = new File(["image"], "icon.png", { type: "image/png" });
            Object.defineProperty(image, "path", { value: "C:\\Fixture\\icon.png" });
            const packageDrop = new DataTransfer(); packageDrop.items.add(file); packageDrop.items.add(image);
            for (const [selection, paths] of [
              [directoryDrop, ["C:\\Fixture\\Directory.url"]],
              [packageDrop, ["C:\\Fixture\\mod.zip", "C:\\Fixture\\icon.png"]]
            ] as const) {
              expectedSourcePaths = [...paths]; selection.setData("text/uri-list", packageUrl);
              const referencesBefore = references, stagesBefore = stages;
              await settle(() => !fixture.querySelector('[aria-disabled="true"].mw-dropzone'));
              await drop(selection);
              check(references === referencesBefore, "Local directories and packages retain file import despite accompanying URLs or images");
              await settle(() => stages === stagesBefore + 1);
            }
            expectedSourcePaths = ["C:\\Fixture\\mod.zip", "C:\\Fixture\\Folder\u3000"];
            referenceFailure = "Fixture dependency download failed";
            const retryLink = new DataTransfer(); retryLink.setData("text/uri-list", packageUrl);
            await settle(() => !fixture.querySelector('[aria-disabled="true"].mw-dropzone'));
            await drop(retryLink);
            await settle(() => Boolean(fixture.textContent?.includes("Mod installation failed:")));
            check(fixture.textContent?.includes("Fixture dependency download failed"), `${id}: dependency preparation errors appear as actionable installation failures`);
            const beforeRetry = references;
            await settle(() => !fixture.querySelector('[aria-disabled="true"].mw-dropzone'));
            await drop(retryLink); await settle(() => references === beforeRetry + 1);
            check(!fixture.textContent?.includes("Mod installation failed:"), `${id}: a later installation can recover after dependency preparation fails`);
          }
        }
        await settle(() => !fixture.querySelector('[aria-disabled="true"].mw-dropzone'));
        const writesBefore = stages + references + saves;
        await drop(link, false, "move");
        await drop(link, false, "none");
        const filesOnly = new DataTransfer(); filesOnly.items.add(file);
        await drop(filesOnly, false, "link");
        check(stages + references + saves === writesBefore, "Move-only, none and link-only Files make no mutation");
        if (id === "arksurvivalascended") {
          const normalDetails = details;
          details = { ...details, settings_json: JSON.stringify({ custom_launch_flags: "-mods=424242" }) };
          await mount();
          await drop(link, false, "link"); await drop(fileDrop, false);
          check(stages + references + saves === writesBefore, "Raw Mod flags keep both reference and file drops locked");
          details = normalDetails; await mount();
        }
      }
      await settle(() => !fixture.querySelector('[aria-disabled="true"].mw-dropzone'));
      details = { ...details, summary: { ...details.summary, status: "Running", active_process_count: 1 } };
      await mount();
      const writesBefore = stages + references + saves;
      await drop(fileDrop, false);
      check(stages + references + saves === writesBefore, `${id}: running servers refuse drops`);
      covered.push(id);
    }
    check(errors.length === 0, "No console errors or unhandled rejections");
    return { status: "passed", checks, covered, stages, references, saves, browser_errors: errors };
  } finally { cleanup(); }
}
void run().catch((error) => ({ status: "failed", checks, covered, error: String(error), browser_errors: errors }))
  .finally(() => { Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }); })
  .then((report) => fetch(`/__reliability_result/${new URLSearchParams(location.search).get("nonce")}`, { method: "POST", body: JSON.stringify(report) }));
