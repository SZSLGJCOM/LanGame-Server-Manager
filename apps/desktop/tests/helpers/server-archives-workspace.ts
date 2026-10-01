import type { InstanceArchiveDetails } from "../../src/storage-management-types";
import type { captureNormalWorkspace } from "./server-archives-preview";

interface Hooks {
  fixture: HTMLElement;
  check: (condition: unknown, description: string, diagnostics?: () => unknown) => asserts condition;
  click: (selector: string) => Promise<void>;
  dispatchKey: (selector: string, key: string) => Promise<void>;
  waitFor: (selector: string) => Promise<void>;
  selectArchive: (id: string) => Promise<void>;
  details: (id: string) => InstanceArchiveDetails;
  writes: string[];
  nativeCommands: string[];
  nativeCalls: { command: string; args: { instanceId?: string; instance_id?: string } }[];
  normalSnapshot: ReturnType<typeof captureNormalWorkspace>;
  phases: string[];
}

const workspace = (id: string) => `.archived-instance-workspace[data-archive-id="${id}"]`;
const tab = (id: string, key: string) => `${workspace(id)} .server-detail-tab[id$="-${key}"]`;
const content = (id: string, key: string) => `${workspace(id)} [data-archive-workspace-tab="${key}"]`;

export async function runArchiveWorkspaceAssertions(hooks: Hooks) {
  const { fixture, check, phases } = hooks;
  function element<T extends HTMLElement = HTMLElement>(selector: string): T {
    const found = fixture.querySelector<T>(selector);
    check(found, `Archived workspace element is present: ${selector}`);
    return found;
  }
  function tabs(id: string) {
    return [...element(workspace(id)).querySelectorAll<HTMLElement>('.server-detail-tabs [role="tab"]')].map((node) => ({
      key: node.id.split("-").at(-1), label: node.querySelector(".server-detail-tab-label")?.textContent,
      icon: node.querySelector(".server-detail-tab-icon")?.innerHTML }));
  }
  async function selectTab(id: string, key: string) {
    await hooks.click(tab(id, key)); await hooks.waitFor(content(id, key));
    check(element(tab(id, key)).getAttribute("aria-selected") === "true", `Archived ${key} tab owns its selected state`);
    check(element(tab(id, key)).getAttribute("aria-controls") === element(content(id, key)).id
      && element(content(id, key)).getAttribute("aria-labelledby") === element(tab(id, key)).id,
      `Archived ${key} tab and panel retain their accessible relationship`);
  }
  function readOnly(id: string) {
    const root = element(workspace(id));
    check([...root.querySelectorAll<HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement>('input,textarea,select')]
      .filter((control) => !control.matches('[type="search"],.configuration-search input,.mw-search-input,.dst-mod-shard-field select,.player-join-address select'))
      .every((control) => control.disabled || ("readOnly" in control && control.readOnly))
      && !root.querySelector('[contenteditable="true"]'), "Archived retained data contains no editable or live form controls");
    check([...root.querySelectorAll<HTMLButtonElement>('button[type="submit"]')].every((button) => button.disabled),
      "Shared archived forms disable every mutation submission");
    check(!root.querySelector(".archive-configuration-preview,.archived-instance-retained-data"),
      "Archived details reuse normal workbenches without independent archive page renderers");
  }
  const beforeWrites = hooks.writes.length, beforeNative = hooks.nativeCommands.length;
  check(JSON.stringify(tabs("a")) === JSON.stringify(hooks.normalSnapshot.tabs)
    && tabs("a").map((entry) => entry.key).join("|") === "runtime|settings|mods|players|maintenance|gm",
    "Archived instances use the normal six-tab order, labels and icons");
  const archiveRoot = element(workspace("a"));
  const tabBounds = element(`${workspace("a")} .server-detail-subheader`).getBoundingClientRect();
  const panelBounds = element(".server-detail-panel").getBoundingClientRect();
  const archiveTabLayout = { top: tabBounds.top - panelBounds.top, left: tabBounds.left - panelBounds.left,
    width: tabBounds.width, height: tabBounds.height };
  check(Boolean(hooks.normalSnapshot.tabLayout) && Object.entries(archiveTabLayout)
    .every(([key, value]) => Math.abs(value - hooks.normalSnapshot.tabLayout![key as keyof typeof archiveTabLayout]) <= 1)
    && archiveRoot.firstElementChild?.classList.contains("server-detail-subheader")
    && !archiveRoot.querySelector(".archive-configuration-preview__header")
    && !archiveRoot.textContent?.includes("Archived · Read only; restore to edit"),
    "Archive tabs share the normal workspace's top position and boundaries without an added archive header");
  check(!element<HTMLButtonElement>(tab("a", "runtime")).disabled
    && !element<HTMLButtonElement>(tab("a", "settings")).disabled && !element<HTMLButtonElement>(tab("a", "mods")).disabled
    && !element<HTMLButtonElement>(tab("a", "players")).disabled && !element<HTMLButtonElement>(tab("a", "maintenance")).disabled
    && element<HTMLButtonElement>(tab("a", "gm")).disabled, "Applicable archived read-only tabs remain usable while live tools are disabled");
  phases.push("shared tabs and archive capabilities");

  await selectTab("a", "runtime");
  await hooks.dispatchKey(tab("a", "runtime"), "ArrowRight");
  check(document.activeElement === element(tab("a", "settings")) && element(tab("a", "settings")).getAttribute("aria-selected") === "true",
    "Keyboard arrows select and focus applicable archived tabs");
  await hooks.dispatchKey(tab("a", "settings"), "End");
  check(document.activeElement === element(tab("a", "maintenance")), "Archived keyboard navigation skips disabled live tools");
  await hooks.dispatchKey(tab("a", "maintenance"), "Home");
  check(document.activeElement === element(tab("a", "runtime")), "Archived Home navigation selects the first retained-data tab");
  phases.push("tab keyboard navigation");

  await selectTab("a", "runtime");
  const source = hooks.details("a");
  check(element(`${workspace("a")} .server-runtime-console-card .server-runtime-console`).textContent === source.log.text
    && element(`${workspace("a")} .server-runtime-console-title`).textContent?.includes("LanGameCMD"),
    "Archived Runtime reuses the normal LanGameCMD console and displays its own retained log text");
  check(element<HTMLInputElement>(`${workspace("a")} .server-runtime-console-command-input`).disabled
    && !element(content("a", "runtime")).textContent?.includes("Normal world"), "The shared archived console disables command submission and excludes another server's data");
  readOnly("a"); phases.push("retained runtime");

  await selectTab("a", "players");
  check(element(content("a", "players")).querySelector(".player-access-workbench .player-center-member-layout")
    && element<HTMLButtonElement>(`${workspace("a")} .player-center-refresh-button`).disabled,
    "Archived Players reuses the normal player center and disables online refresh");
  await hooks.click(`${workspace("a")} .player-center-list-tab[data-list-key="whitelist_entries"]`);
  check(element(`${workspace("a")} .player-access-roster-list`).textContent?.includes("ARCHIVE_PLAYER_a"),
    "The shared player center displays the archive's saved whitelist");
  readOnly("a");
  await hooks.click(`${workspace("a")} .player-center-list-tab[data-list-key="operator_entries"]`);
  check(element(`${workspace("a")} .player-access-roster-list`).textContent?.includes("ARCHIVE_ADMIN_a"),
    "The shared player center displays the archive's saved administrator entries");
  readOnly("a"); phases.push("saved player access");

  await selectTab("a", "maintenance");
  const maintenance = element(`${workspace("a")} .maintenance-workspace`);
  check([...maintenance.querySelectorAll<HTMLElement>("[data-maintenance-section]")]
    .map((section) => section.dataset.maintenanceSection).join("|") === "backups|save-policy|runtime|storage|broadcast",
    "Archived Maintenance reuses the normal saves, policy, runtime, storage and broadcast categories");
  const backup = element(`${workspace("a")} .server-file-backup-row`);
  check(backup.querySelector(".server-file-backup-name")?.textContent === "ARCHIVE_BACKUP_a"
    && backup.querySelector(".server-file-backup-meta")?.textContent?.includes("4 KB")
    && backup.querySelector(".server-file-backup-meta")?.textContent?.includes("3 files"),
    "Shared backup history displays the archived name, formatted byte size and file count");
  check([...backup.querySelectorAll<HTMLButtonElement>("button")].every((button) => button.disabled),
    "Archived backup restore, rename and delete controls use normal labels and remain disabled");
  async function maintenanceSection(id: string) {
    const toggle = maintenance.querySelector<HTMLButtonElement>(".configuration-workspace__navigation-toggle");
    if (toggle && toggle.getBoundingClientRect().width > 0 && toggle.getAttribute("aria-expanded") !== "true") {
      await hooks.click(`${workspace("a")} .maintenance-workspace .configuration-workspace__navigation-toggle`);
    }
    await hooks.click(`${workspace("a")} .maintenance-workspace [data-configuration-section-id="${id}"] > button`);
    check(!element(`${workspace("a")} [data-maintenance-section="${id}"]`).hidden, `Shared archived maintenance selects its ${id} category`);
  }
  await maintenanceSection("save-policy");
  check(!element<HTMLInputElement>(`${workspace("a")} .server-managed-backup-policy input[type="checkbox"]`).checked
    && element<HTMLInputElement>(`${workspace("a")} .server-managed-backup-policy input[type="number"]`).value === "7",
    "Normal save-policy controls preserve the archived stop-backup setting and retention count");
  readOnly("a");
  await maintenanceSection("runtime");
  check(element<HTMLInputElement>(`${workspace("a")} .server-autostart-policy-editor input`).checked
    && !element<HTMLInputElement>(`${workspace("a")} .server-runtime-recovery-editor input[name="enabled"]`).checked
    && element<HTMLInputElement>(`${workspace("a")} .server-runtime-recovery-editor input[name="maxRestarts"]`).value === "2"
    && element<HTMLInputElement>(`${workspace("a")} .server-runtime-recovery-editor input[name="waitSeconds"]`).value === "0"
    && element<HTMLInputElement>(`${workspace("a")} .server-resource-policy input[name="cpu"]`).value === "25"
    && element<HTMLInputElement>(`${workspace("a")} .server-resource-policy input[name="memory"]`).value === "3072"
    && element<HTMLInputElement>(`${workspace("a")} .server-resource-policy input[name="reserve"]`).value === "0",
    "Normal runtime-policy controls preserve saved autostart, recovery, memory and zero-valued limits");
  readOnly("a"); phases.push("saved maintenance and backups");

  await selectTab("a", "mods");
  check(element(content("a", "mods")).querySelector(".mw-workbench-main")
    && element(content("a", "mods")).textContent?.includes("No Mods are configured for this instance.")
    && !element(content("a", "mods")).textContent?.includes("ARCHIVE_MOD_b"),
    "An archive without stored mod references shows its real empty state without another archive's configuration");
  await hooks.selectArchive("b"); await hooks.waitFor(`${workspace("b")}[data-archive-configuration-state="ready"]`);
  check(element(tab("b", "mods")).getAttribute("aria-selected") === "true", "Selecting another archive preserves the selected Mods workspace");
  await selectTab("b", "mods");
  await hooks.waitFor(`${workspace("b")} .mw-entry-row-select`);
  check(element(`${workspace("b")} .mw-entry-list`).textContent?.includes("22334455"),
    "The shared Mod workbench displays saved workshop references from the archived game's configuration");
  await hooks.click(`${workspace("b")} .mw-entry-row-select`);
  check(element(`${workspace("b")} .mw-selected-config-panel`).textContent?.includes("ARCHIVE_MOD_b")
    || [...element(`${workspace("b")} .mw-selected-config-panel`).querySelectorAll<HTMLInputElement | HTMLTextAreaElement>("input,textarea")]
      .some((control) => control.value.includes("ARCHIVE_MOD_b")),
    "The shared Mod configuration panel preserves the archive's saved per-mod configuration");
  check([...element(content("b", "mods")).querySelectorAll<HTMLButtonElement>(".mw-entry-enabled-toggle,.mw-entry-remove-button")]
    .every((button) => button.disabled), "The normal Mod enable and remove controls remain disabled for archives");
  readOnly("b");
  const gm = element<HTMLButtonElement>(tab("b", "gm"));
  const help = gm.closest(".server-detail-tab-help");
  check(gm.disabled && help?.getAttribute("aria-describedby")
    && /restore/i.test(document.getElementById(help.getAttribute("aria-describedby")!)?.textContent ?? ""),
    "Game tools supported by the archived game explain that the instance must be restored before use");
  phases.push("saved mods and disabled tools");

  await hooks.selectArchive("e"); await hooks.waitFor(`${workspace("e")}[data-archive-configuration-state="ready"]`);
  check(element<HTMLButtonElement>(tab("e", "mods")).disabled && element<HTMLButtonElement>(tab("e", "gm")).disabled,
    "Archived tab applicability follows the selected archive's game rather than the normal instance's game");
  check(element(tab("e", "runtime")).getAttribute("aria-selected") === "true", "An archive without a supported Mods workspace falls back to Runtime");
  readOnly("e");
  const readCommands = new Set(["read_instance_archive_details", "read_module_details", "read_module_configuration_icons"]);
  check(hooks.writes.length === beforeWrites && hooks.nativeCommands.slice(beforeNative).every((command) => readCommands.has(command)),
    "All archived tabs stay read-only without normal-instance reads, online queries or write callbacks");
  check(hooks.nativeCalls.slice(beforeNative).filter((call) => call.command === "read_module_configuration_icons")
    .every((call) => !call.args.instanceId && !call.args.instance_id),
    "Archived configuration icons read only module resources without targeting a restored or active instance ID");
  phases.push("archive data isolation and no live writes");
  await hooks.selectArchive("a"); await hooks.waitFor(`${workspace("a")}[data-archive-configuration-state="ready"]`);
  check(element(tab("a", "mods")).getAttribute("aria-selected") === "true", "Revisiting an archive restores Mods after the unsupported archive's Runtime fallback");
}
