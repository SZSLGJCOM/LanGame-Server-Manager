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
const workshopModules = ["barotrauma", "conanexiles", "palworld", "squad", "terraria"];
const checks: string[] = [], errors: string[] = [], covered: string[] = [], workshopCovered: string[] = [];
const versionPolicyCovered: string[] = [];
const nexusRequests: string[] = [];
const nexusCovered: { module: string; local_imports: number; rejected_inputs: string[] }[] = [];
let details: InstanceDetails, moduleDetails: ModuleDetails, inventory: ManualModInventoryResult;
let epoch = 0, stages = 0, references = 0, saves = 0, inventoryReads = 0;
let lastReferences: string[] = [];
let lastVersionPolicy: string | undefined;
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
    if (command.includes("nexus")) {
      nexusRequests.push(command);
      throw new Error(`Nexus local imports must not call ${command}`);
    }
    const args = payload as { instanceId: string; references: string[]; sourcePaths: string[]; thunderstoreVersionPolicy?: string };
    if (command === "read_manual_mod_inventory") inventoryReads++;
    if (["stage_manual_mod_files", "install_manual_mod_references", "resolve_manual_mod_references"].includes(command)) {
      check(args.instanceId === details.summary.id, "Drop mutation stays scoped to its current instance");
      if (command === "stage_manual_mod_files") {
        check(args.sourcePaths?.length === expectedSourcePaths.length &&
          args.sourcePaths.every((path, index) => path === expectedSourcePaths[index]),
          "Native paths preserve the selected files, directories and significant trailing Unicode spaces");
        stages++; return stageResult();
      }
      references++; lastReferences = args.references; lastVersionPolicy = args.thunderstoreVersionPolicy;
      check(command !== "resolve_manual_mod_references" || !Object.hasOwn(args, "thunderstoreVersionPolicy"),
        "Non-Thunderstore reference resolution has no Thunderstore version policy");
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
      check(Boolean(fixture.querySelector(".mw-manual-pane > .mw-dropzone")) && !fixture.querySelector(".mw-config-pane"),
        "Non-Workshop file installation starts in the independent Store drop zone");
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
      if (moduleDetails.mods?.source?.provider === "nexus") {
        const source = new URL(moduleDetails.mods.source.url);
        const rejectedInputs: string[] = [];
        for (const [kind, format, value] of [
          ["id", "text/plain", "123"],
          ["http", "text/uri-list", `${source.href}/mods/123`],
          ["nxm", "text/uri-list", `nxm://${source.pathname.slice(1)}/mods/123/files/456?key=fixture&expires=2000000000&user_id=1`]
        ]) {
          const data = new DataTransfer(); data.setData(format, value);
          await settle(() => !fixture.querySelector('[aria-disabled="true"].mw-dropzone'));
          const beforeReferences = references, beforeImports = stages, beforeSaves = saves;
          await drop(data, false);
          check(nexusRequests.length === 0 && references === beforeReferences && stages === beforeImports && saves === beforeSaves,
            `${id}: Nexus ${kind} drops are rejected without API requests, reference resolution, imports or settings writes`);
          check(!fixture.querySelector("[data-nexus-file-review],[data-nexus-install]"),
            `${id}: rejected Nexus ${kind} drops do not open file selection`);
          rejectedInputs.push(kind);
        }
        nexusCovered.push({ module: id, local_imports: stages - previousStage, rejected_inputs: rejectedInputs });
      }
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
          const policy = fixture.querySelector<HTMLButtonElement>(".mw-version-policy-btn");
          check(policy?.dataset.versionPolicy === "follow_latest", `${id}: online installation defaults to following latest`);
          check(policy.textContent?.trim() === "Follow latest" && policy.getAttribute("aria-label") === "Update policy: Follow latest"
            && !policy.hasAttribute("aria-pressed"), "Update policy button exposes its current value in a concise accessible name");
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
              && lastReferences[0] === "https://thunderstore.io/package/FixtureAuthor/FixturePlugin/1.2.3/"
              && lastVersionPolicy === "follow_latest",
            `${id}: Mod Manager button ${format} keeps this download's version but explicitly follows latest for future starts`);
          }
          await settle(() => !policy.disabled);
          await act(async () => {
            policy.focus(); check(document.activeElement === policy, "Version policy is keyboard-focusable");
            policy.click();
          });
          check(policy.getAttribute("data-version-policy") === "pinned" && policy.textContent?.trim() === "Fixed version"
            && policy.getAttribute("aria-label") === "Update policy: Fixed version", "The button preserves the user's explicit pin selection");
          for (const view of ["My Mods", "Store"]) {
            const tab = [...fixture.querySelectorAll<HTMLButtonElement>('[role="tab"]')].find((item) => item.textContent === view);
            check(tab, `The ${view} tab is available beside the version policy`);
            await act(async () => tab.click());
            check(fixture.querySelector(".mw-version-policy-btn") === policy && policy.getAttribute("data-version-policy") === "pinned",
              `${id}: ${view} retains the same version button and explicit pin choice`);
          }
          const input = fixture.querySelector<HTMLInputElement>(".mw-reference-input:not(select)");
          const install = [...fixture.querySelectorAll<HTMLButtonElement>(".mw-reference-row button")]
            .find((button) => button.textContent?.trim() === "Install");
          check(input && install, "Reference entry and real install button are available");
          const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
          check(setter, "Native reference input setter exists");
          const reference = "ror2mm://v1/install/thunderstore.io/FixtureAuthor/FixturePlugin/1.2.3/";
          await act(async () => { setter.call(input, reference); input.dispatchEvent(new Event("input", { bubbles: true })); });
          const beforePinned = references;
          await act(async () => { install.click(); });
          await settle(() => references === beforePinned + 1 && !policy.disabled);
          check(lastVersionPolicy === "pinned" && lastReferences[0] === "https://thunderstore.io/package/FixtureAuthor/FixturePlugin/1.2.3/",
            `${id}: explicitly pinning sends pinned intent through the real input, handler and API`);
          await act(async () => { policy.click(); });
          check(policy.getAttribute("data-version-policy") === "follow_latest" && policy.getAttribute("aria-label") === "Update policy: Follow latest",
            "A second button click restores following latest before installation");
          await act(async () => { setter.call(input, reference); input.dispatchEvent(new Event("input", { bubbles: true })); });
          const beforeLatest = references;
          await act(async () => { install.click(); });
          await settle(() => references === beforeLatest + 1 && !policy.disabled);
          check(lastVersionPolicy === "follow_latest", `${id}: returning to follow latest is an explicit independent install intent`);
          versionPolicyCovered.push(id);
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
      if (moduleDetails.mods?.source?.provider === "thunderstore") {
        check(fixture.querySelector<HTMLButtonElement>(".mw-version-policy-btn")?.disabled,
          "Running instances cannot change the installation version policy");
      }
      const writesBefore = stages + references + saves;
      await drop(fileDrop, false);
      check(stages + references + saves === writesBefore, `${id}: running servers refuse drops`);
      covered.push(id);
    }
    for (const id of workshopModules) {
      moduleDetails = buildMockModuleDetails(id);
      check(moduleDetails.summary.id === id && moduleDetails.workshop?.provider === "steam" && Boolean(moduleDetails.mods?.manual_staging),
        "Workshop drag installation is covered only for this game's actual staging contract");
      details = { ...template, summary: { ...template.summary, id: `drop-workshop-${id}`, module_id: id,
        status: "Stopped", active_process_count: 0 }, active_run: null, settings_json: "{}" };
      inventory = { instance_id: details.summary.id, module_id: id, source_label: "Steam Workshop", target_label: "Mods",
        target_path: `C:\\Fixture\\${id}\\Mods`, target_exists: false, items: [] };
      const readsBefore = inventoryReads;
      await mount();
      const myMods = [...fixture.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.textContent?.trim() === "My Mods");
      check(myMods && !myMods.disabled, "Workshop My Mods navigation is available");
      await act(async () => { myMods.click(); });
      await settle(() => inventoryReads > readsBefore && Boolean(fixture.querySelector(".mw-config-pane .mw-detail-list-col.mw-dropzone")));
      check(!fixture.querySelector(".mw-manual-pane,.mw-reference-row"), "Workshop My Mods keeps file import within its inventory list");
      const file = new File(["fixture"], "mod.zip");
      Object.defineProperty(file, "path", { value: "C:\\Fixture\\mod.zip" });
      const folder = new File([], "Folder\u3000");
      Object.defineProperty(folder, "path", { value: "C:\\Fixture\\Folder\u3000" });
      const transfer = new DataTransfer(); transfer.items.add(file); transfer.items.add(folder);
      transfer.setData("text/plain", "https://example.invalid/incidental-file-text");
      const stagesBefore = stages, referencesBefore = references, savesBefore = saves;
      await drop(transfer);
      await settle(() => stages === stagesBefore + 1 && !fixture.querySelector('[aria-disabled="true"].mw-dropzone'));
      check(references === referencesBefore && saves === savesBefore,
        `${id}: My Mods stages local files without turning incidental text into a reference or settings mutation`);
      check(Boolean(fixture.querySelector(".mw-config-pane .mw-dropzone")), "Successful Workshop file import stays in My Mods");
      const link = new DataTransfer(); link.setData("text/uri-list", "https://steamcommunity.com/sharedfiles/filedetails/?id=424242");
      await drop(link, false, "link");
      check(stages === stagesBefore + 1 && references === referencesBefore && saves === savesBefore,
        "Workshop My Mods local import does not gain an unsupported reference installation path");
      details = { ...details, summary: { ...details.summary, status: "Running", active_process_count: 1 } };
      await mount();
      const runningMyMods = [...fixture.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.textContent?.trim() === "My Mods");
      check(runningMyMods, "Running Workshop instance retains My Mods navigation");
      await act(async () => { runningMyMods.click(); });
      await drop(transfer, false);
      check(stages === stagesBefore + 1 && references === referencesBefore && saves === savesBefore,
        `${id}: running Workshop instances refuse My Mods file drops`);
      workshopCovered.push(id);
    }
    check(nexusRequests.length === 0, "Nexus file workflows never dispatch account, review or download requests");
    check(errors.length === 0, "No console errors or unhandled rejections");
    return { status: "passed", checks, covered, workshop_covered: workshopCovered,
      version_policy_covered: versionPolicyCovered, nexus_covered: nexusCovered, nexus_requests: nexusRequests,
      stages, references, saves, browser_errors: errors };
  } finally { cleanup(); }
}
void run().catch((error) => ({ status: "failed", checks, covered, workshop_covered: workshopCovered, error: String(error), browser_errors: errors }))
  .finally(() => { Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }); })
  .then((report) => fetch(`/__reliability_result/${new URLSearchParams(location.search).get("nonce")}`, { method: "POST", body: JSON.stringify(report) }));
