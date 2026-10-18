import React, { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { I18nProvider, useI18n, type LocaleCode } from "../../src/i18n";
import { createPlayerCenterState, derivePlayerCenterViewModel, reducePlayerCenterState } from "../../src/domain/live-player-state";
import { OnlinePlayersView } from "../../src/views/servers/player-center/OnlinePlayersView";
import { usePlayerAccess } from "../../src/views/servers/player-center/use-player-access";
import { ManualPlayerActions } from "../../src/views/servers/player-center/ManualPlayerActions";
import { readPlayerAccessRosterCapabilities } from "../../src/views/servers/player-center/player-access-roster-model";
import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import type { InstanceDetails, ModuleDetails, ModulePlayerActionDetails, RuntimeLivePlayerSnapshot } from "../../src/types";
import "../../src/app.css";
import "../../src/views/servers/workbench.css";

// Worst case for the Players list header: 7 Days to Die declares five persistent rosters (the most of any module),
// so the strip holds 在线成员 + 5 rosters + 手动查询 = 7 tabs next to the status chips and the refresh action.
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "en-US");
document.documentElement.dataset.theme = "dark";
const errors: string[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
const nonce = new URLSearchParams(location.search).get("nonce");
const now = 1_000_000;
const sevenDaysRosterFields = {
  "admin_users": {
    "type": "array",
    "title": "Admin Users",
    "description": "Structured native admin user entries rendered into serveradmin.xml.",
    "default": [],
    "x-lsgm-section": "access",
    "x-lsgm-order": 110,
    "items": {
      "type": "object",
      "required": [
        "platform",
        "userid"
      ],
      "properties": {
        "platform": {
          "type": "string",
          "title": "Account platform",
          "description": "Platform namespace used by 7DTD for this account ID.",
          "enum": [
            "Steam",
            "EOS",
            "XBL",
            "PSN"
          ],
          "default": "Steam",
          "x-lsgm-player-access-identity": true
        },
        "userid": {
          "type": "string",
          "title": "Platform user ID",
          "description": "The account ID recorded by 7DTD logs for the selected platform.",
          "pattern": "^[A-Za-z0-9._:-]{1,64}$",
          "x-lsgm-player-access-platform-field": "platform",
          "x-lsgm-player-access-identity": true
        },
        "name": {
          "type": "string",
          "title": "Display name",
          "maxLength": 128
        },
        "permission_level": {
          "type": "integer",
          "title": "Permission level",
          "minimum": 0,
          "maximum": 1000,
          "default": 0
        }
      }
    },
    "x-lsgm-player-access-kind": "admin",
    "x-lsgm-player-access-codec": "object_identity",
    "x-lsgm-player-access-sync": {
      "mode": "restart"
    },
    "x-lsgm-source": "serveradmin_xml",
    "x-lsgm-source-key": "adminTools/users/user",
    "x-lsgm-source-surface": "generated_roster"
  },
  "admin_groups": {
    "type": "array",
    "title": "Admin Groups",
    "description": "Structured native Steam group admin entries rendered into serveradmin.xml.",
    "default": [],
    "x-lsgm-section": "access",
    "x-lsgm-order": 111,
    "items": {
      "type": "object",
      "required": [
        "steam_id"
      ],
      "properties": {
        "steam_id": {
          "type": "string",
          "title": "Steam group ID",
          "pattern": "^[0-9]{17,20}$",
          "x-lsgm-player-access-identity": true
        },
        "name": {
          "type": "string",
          "title": "Display name",
          "maxLength": 128
        },
        "permission_level_default": {
          "type": "integer",
          "title": "Default member permission level",
          "minimum": 0,
          "maximum": 1000,
          "default": 1000
        },
        "permission_level_mod": {
          "type": "integer",
          "title": "Moderator permission level",
          "minimum": 0,
          "maximum": 1000,
          "default": 0
        }
      }
    },
    "x-lsgm-player-access-kind": "admin",
    "x-lsgm-player-access-codec": "object_identity",
    "x-lsgm-player-access-sync": {
      "mode": "restart"
    },
    "x-lsgm-source": "serveradmin_xml",
    "x-lsgm-source-key": "adminTools/users/group",
    "x-lsgm-source-surface": "generated_roster"
  },
  "whitelist_users": {
    "type": "array",
    "title": "Whitelisted Users",
    "description": "Structured native whitelist user entries rendered into serveradmin.xml.",
    "default": [],
    "x-lsgm-section": "access",
    "x-lsgm-order": 112,
    "items": {
      "type": "object",
      "required": [
        "platform",
        "userid"
      ],
      "properties": {
        "platform": {
          "type": "string",
          "title": "Account platform",
          "description": "Platform namespace used by 7DTD for this account ID.",
          "enum": [
            "Steam",
            "EOS",
            "XBL",
            "PSN"
          ],
          "default": "Steam",
          "x-lsgm-player-access-identity": true
        },
        "userid": {
          "type": "string",
          "title": "Platform user ID",
          "description": "The account ID recorded by 7DTD logs for the selected platform.",
          "pattern": "^[A-Za-z0-9._:-]{1,64}$",
          "x-lsgm-player-access-platform-field": "platform",
          "x-lsgm-player-access-identity": true
        },
        "name": {
          "type": "string",
          "title": "Display name",
          "maxLength": 128
        }
      }
    },
    "x-lsgm-player-access-kind": "allow",
    "x-lsgm-player-access-codec": "object_identity",
    "x-lsgm-player-access-sync": {
      "mode": "restart"
    },
    "x-lsgm-source": "serveradmin_xml",
    "x-lsgm-source-key": "adminTools/whitelist/user",
    "x-lsgm-source-surface": "generated_roster"
  },
  "whitelist_groups": {
    "type": "array",
    "title": "Whitelisted Groups",
    "description": "Structured native whitelist Steam group entries rendered into serveradmin.xml.",
    "default": [],
    "x-lsgm-section": "access",
    "x-lsgm-order": 113,
    "items": {
      "type": "object",
      "required": [
        "steam_id"
      ],
      "properties": {
        "steam_id": {
          "type": "string",
          "title": "Steam group ID",
          "pattern": "^[0-9]{17,20}$",
          "x-lsgm-player-access-identity": true
        },
        "name": {
          "type": "string",
          "title": "Display name",
          "maxLength": 128
        }
      }
    },
    "x-lsgm-player-access-kind": "allow",
    "x-lsgm-player-access-codec": "object_identity",
    "x-lsgm-player-access-sync": {
      "mode": "restart"
    },
    "x-lsgm-source": "serveradmin_xml",
    "x-lsgm-source-key": "adminTools/whitelist/group",
    "x-lsgm-source-surface": "generated_roster"
  },
  "blacklist_entries": {
    "type": "array",
    "title": "Blacklist Entries",
    "description": "Structured native blacklist entries rendered into serveradmin.xml.",
    "default": [],
    "x-lsgm-section": "access",
    "x-lsgm-order": 114,
    "items": {
      "type": "object",
      "required": [
        "platform",
        "userid"
      ],
      "properties": {
        "platform": {
          "type": "string",
          "title": "Account platform",
          "description": "Platform namespace used by 7DTD for this account ID.",
          "enum": [
            "Steam",
            "EOS",
            "XBL",
            "PSN"
          ],
          "default": "Steam",
          "x-lsgm-player-access-identity": true
        },
        "userid": {
          "type": "string",
          "title": "Platform user ID",
          "description": "The account ID recorded by 7DTD logs for the selected platform.",
          "pattern": "^[A-Za-z0-9._:-]{1,64}$",
          "x-lsgm-player-access-platform-field": "platform",
          "x-lsgm-player-access-identity": true
        },
        "name": {
          "type": "string",
          "title": "Display name",
          "maxLength": 128
        },
        "unbandate": {
          "type": "string",
          "title": "Unban date",
          "description": "Local server date or date and time in YYYY-MM-DD or YYYY-MM-DD HH:mm:ss format. The default is an effectively permanent ban.",
          "format": "date-or-local-datetime",
          "pattern": "^[0-9]{4}-(0[1-9]|1[0-2])-([0-2][0-9]|3[01])( ([01][0-9]|2[0-3]):[0-5][0-9]:[0-5][0-9])?$",
          "default": "9999-12-31"
        },
        "reason": {
          "type": "string",
          "title": "Reason",
          "maxLength": 256,
          "default": "LanGame"
        }
      }
    },
    "x-lsgm-player-access-kind": "block",
    "x-lsgm-player-access-codec": "object_identity",
    "x-lsgm-player-access-live-target": "platform_userid",
    "x-lsgm-player-access-sync": {
      "mode": "direct",
      "remove_action_id": "unban_player",
      "consume_action_ids": [
        "unban_player"
      ]
    },
    "x-lsgm-source": "serveradmin_xml",
    "x-lsgm-source-key": "adminTools/blacklist/blacklisted",
    "x-lsgm-source-surface": "generated_roster"
  }
};
const kick: ModulePlayerActionDetails = { id: "kick", kind: "kick", label: "Kick player", label_zh_cn: "踢出玩家", command_template: "kick {{target}}", target_required: true, destructive: true };
const manualActions: ModulePlayerActionDetails[] = [
  { id: "manual-ban", kind: "ban", label: "Ban known account", label_zh_cn: "封禁指定账号", command_template: "ban {{target}}", target_required: true, destructive: true }
];
const moduleDetails: ModuleDetails = {
  summary: { id: "sevendaystodie", name: "7 Days to Die", version: "1", install_state: "Installed", supported_platforms: ["windows"] },
  default_ports: [], runtime: { player_actions: [kick] },
  schema_json: JSON.stringify({ type: "object", properties: sevenDaysRosterFields })
};
const details: InstanceDetails = {
  summary: { id: "tabs-server", name: "Tabs server", module_id: "sevendaystodie", status: "Running", active_process_count: 1, autostart: false, bind_ip: "127.0.0.1" },
  ports: [], settings_json: JSON.stringify(Object.fromEntries(Object.keys(sevenDaysRosterFields).map((key) => [key, []]))),
  config_file_path: "", saves_path: "", backup_uses_declared_saves_path: false, auto_backup_on_stop: false, backup_retention_count: 3
};
function snapshot(count: number): RuntimeLivePlayerSnapshot {
  return { snapshot_id: `tabs-${count}`, instance_id: "tabs-server", status: "ready", source: "runtime_action",
    observed_at_unix_ms: now - 1000, expires_at_unix_ms: now + 30_000, complete: true,
    // Every status chip the header can show, so the actions are at their widest.
    truncated: true, stale: true, current_players: count, max_players: 100, issue: null,
    entries: Array.from({ length: count }, (_, index) => ({ player_key: `player-${index}`, display_name: `Player ${index + 1}`,
      identifiers: [{ kind: "steam_id", value: String(76561198000000001n + BigInt(index)), stable: true }],
      available_action_ids: ["kick"], ping_ms: 24 + index, session_started_at_unix_ms: now - 120_000, role: null, attributes: [] })) };
}
let setFixtureLocale: (locale: LocaleCode) => void;
function Fixture() {
  const { locale, t, setLocale } = useI18n();
  setFixtureLocale = setLocale;
  const [state, setState] = useState(() => createPlayerCenterState({ instanceId: "tabs-server", snapshot: snapshot(60) }));
  const access = usePlayerAccess({ details, moduleDetails, onApplyPlayerAccessMutation: async () => { throw new Error("Fixture is read-only"); } });
  const presentation = derivePlayerCenterViewModel(state, now).livePlayers;
  // The real application shell around the servers page, so widths follow the window like in LGSM.
  return <div className="shell-frame" data-theme="dark"><div className="shell-main shell-main--console">
    <div aria-hidden="true" style={{ height: "88px" }} />
    <main className="shell-content-scroll"><div className="shell-content-body">
      <div className="page-grid workspace-page servers-page servers-page--no-metrics">
        <section className="workspace-section-grid server-layout-grid">
          <section className="server-list-panel" aria-label="Servers" />
          <section className="server-detail-panel"><div className="detail-stack detail-stack--server">
            <div className="server-detail-subheader" style={{ minHeight: "42px" }} />
            <div className="server-detail-scroll server-detail-scroll--players" role="tabpanel" aria-label="Players">
              <section className="player-access-workbench">
                <OnlinePlayersView declaredActions={[kick]} error={null} loading={false} playerActionIds={["kick"]}
                  locale={locale} now={now} onRefresh={async () => {}} presentation={presentation} runtime={null} runtimeAvailable
                  snapshot={state.snapshot} t={t} rosterCapabilities={readPlayerAccessRosterCapabilities(moduleDetails)}
                  onSelectPlayer={(playerKey) => setState((current) => reducePlayerCenterState(current, { type: "player-selected", playerKey }))}
                  manualActions={(active) => <ManualPlayerActions moduleId="sevendaystodie" actions={manualActions} disabled={!active}
                    instanceId="tabs-server" locale={locale} t={t} />}
                  access={access} />
              </section>
            </div>
          </div></section>
        </section>
      </div>
    </div></main>
    <div aria-hidden="true" style={{ height: "32px" }} />
  </div></div>;
}
const failures: string[] = [];
let checks = 0;
function expect(condition: unknown, message: string) { if (condition) checks++; else failures.push(message); }
function select<T extends Element>(selector: string): T {
  const element = document.querySelector<T>(selector); if (!element) throw new Error(`Missing element: ${selector}`); return element;
}
async function frames(count = 2) {
  for (let index = 0; index < count; index++) await act(async () => { await new Promise<void>((resolve) => requestAnimationFrame(() => resolve())); });
}
async function waitFor(condition: () => boolean, label: string) {
  const deadline = performance.now() + 5000;
  while (!condition()) {
    if (performance.now() > deadline) throw new Error(`Timed out waiting for ${label}`);
    await act(async () => { await new Promise<void>((resolve) => setTimeout(resolve, 10)); });
  }
}
const rect = (element: Element) => { const box = element.getBoundingClientRect(); return { top: box.top, bottom: box.bottom, left: box.left, right: box.right, width: box.width, height: box.height }; };
const same = (a: ReturnType<typeof rect>, b: ReturnType<typeof rect>) => Math.abs(a.top - b.top) <= 0.5 && Math.abs(a.left - b.left) <= 0.5
  && Math.abs(a.width - b.width) <= 0.5 && Math.abs(a.height - b.height) <= 0.5;
function scrolled(root: Element) {
  return [root, ...root.querySelectorAll("*")].filter((element) => element.scrollTop > 0)
    .map((element) => (element as HTMLElement).className || element.tagName);
}
function verticalOwners(root: Element) {
  return [root, ...root.querySelectorAll<HTMLElement>("*")].filter((element) => element.getClientRects().length > 0
    && ["auto", "scroll"].includes(getComputedStyle(element).overflowY) && element.scrollHeight > element.clientHeight + 1);
}
function measureHeader(locale: string) {
  const header = select<HTMLElement>(".player-center-pane-header");
  const strip = select<HTMLElement>(".player-center-list-tabs");
  const tabs = [...strip.querySelectorAll<HTMLElement>('[role="tab"]')];
  const refresh = select<HTMLElement>(".player-center-refresh-button");
  const tools = header.querySelector<HTMLElement>(".player-center-pane-tools") ?? refresh;
  const box = rect(header), stripBox = rect(strip), refreshBox = rect(refresh), toolsBox = rect(tools);
  const centre = (value: ReturnType<typeof rect>) => value.top + value.height / 2;
  const tabTops = new Set(tabs.map((tab) => Math.round(tab.getBoundingClientRect().top)));
  const member = select<HTMLElement>(".player-center-member-header");
  const result = { locale, window: innerWidth, workspace_width: Math.round(select<HTMLElement>(".player-access-workbench").getBoundingClientRect().width),
    list_pane_width: Math.round(select<HTMLElement>(".player-center-list-pane").getBoundingClientRect().width),
    tab_count: tabs.length, tab_titles: tabs.map((tab) => tab.textContent?.trim()),
    header_height: Math.round(box.height), member_header_height: Math.round(member.getBoundingClientRect().height),
    tab_rows: tabTops.size, actions_same_row: Math.abs(centre(stripBox) - centre(refreshBox)) <= 4 && refreshBox.bottom <= stripBox.bottom + 6,
    strip_overflows: strip.scrollWidth > strip.clientWidth + 1, overflow_start: strip.dataset.overflowStart ?? null, overflow_end: strip.dataset.overflowEnd ?? null,
    tools_width: Math.round(toolsBox.width), strip_width: Math.round(stripBox.width) };
  expect(tabs.length === 7, `${locale}@${innerWidth}: expected the 7DTD worst case of 7 tabs, got ${tabs.length}`);
  expect(result.actions_same_row, `${locale}@${innerWidth}: refresh action wrapped below the tabs`);
  expect(result.tab_rows === 1, `${locale}@${innerWidth}: tabs wrapped onto ${result.tab_rows} rows`);
  expect(refreshBox.right <= box.right + 0.5 && toolsBox.left >= stripBox.right - 0.5, `${locale}@${innerWidth}: actions overlap the tabs or leave the header`);
  expect(header.scrollWidth <= header.clientWidth + 1, `${locale}@${innerWidth}: header overflows horizontally`);
  expect(result.header_height === result.member_header_height, `${locale}@${innerWidth}: list header (${result.header_height}px) and 玩家管理 header (${result.member_header_height}px) differ in height`);
  expect(Math.abs(box.top - member.getBoundingClientRect().top) <= 0.5, `${locale}@${innerWidth}: list header and 玩家管理 header tops differ`);
  expect(!result.strip_overflows || result.overflow_end === "true" || result.overflow_start === "true", `${locale}@${innerWidth}: hidden tabs have no overflow affordance`);
  expect(strip.getAttribute("role") === "tablist" && Boolean(strip.getAttribute("aria-label")), `${locale}@${innerWidth}: tab strip lost its accessible name`);
  const observed = header.querySelector<HTMLElement>(".player-center-observed-chip");
  expect(observed && observed.scrollWidth <= observed.clientWidth && getComputedStyle(observed).flexShrink === "0",
    `${locale}@${innerWidth}: the observed-time tag is truncated or can shrink`);
  const headerStyle = getComputedStyle(header);
  const headerContent = header.clientWidth - parseFloat(headerStyle.paddingLeft) - parseFloat(headerStyle.paddingRight);
  const stripMinimum = Math.min(headerContent * 0.4, 240) - 1; // .player-center-list-tabs min-width: min(40%, 240px)
  expect(stripBox.width >= stripMinimum && tabs[0].getBoundingClientRect().right > stripBox.left + 40,
    `${locale}@${innerWidth}: the tab strip (${Math.round(stripBox.width)}px) fell below its minimum or shows no tab`);
  for (const chip of header.querySelectorAll<HTMLElement>(".player-center-pane-tools > .page-chip:not(.player-center-observed-chip)")) {
    expect(chip.scrollWidth <= chip.clientWidth + 1 || Boolean(chip.title), `${locale}@${innerWidth}: a truncated status tag has no title`);
  }
  for (const tab of tabs) expect(tab.scrollWidth <= tab.clientWidth + 1, `${locale}@${innerWidth}: tab "${tab.textContent}" is clipped inside itself`);
  return result;
}
async function keyboardReachesEveryTab(locale: string) {
  const strip = select<HTMLElement>(".player-center-list-tabs");
  const tabs = () => [...strip.querySelectorAll<HTMLButtonElement>('[role="tab"]')];
  // A tab wider than the strip (narrowest window, widest status chips) counts as in view when its start is visible.
  const inView = (tab: HTMLElement) => { const a = tab.getBoundingClientRect(), b = strip.getBoundingClientRect();
    return a.left >= b.left - 0.5 && (a.right <= b.right + 0.5 || a.width >= strip.clientWidth - 6); };
  const key = async (value: string) => { await act(async () => { document.activeElement?.dispatchEvent(new KeyboardEvent("keydown", { key: value, bubbles: true })); }); await frames(); };
  tabs()[0].focus();
  await key("End");
  const last = tabs().at(-1)!;
  expect(document.activeElement === last && last.getAttribute("aria-selected") === "true" && last.tabIndex === 0, `${locale}@${innerWidth}: End did not move roving focus to the last tab`);
  expect(inView(last), `${locale}@${innerWidth}: the selected last tab is not scrolled into view`);
  const stripStyle = getComputedStyle(strip), outline = 2 + 1; // .player-center-list-tab:focus-visible outline + offset
  expect(parseFloat(stripStyle.paddingTop) >= outline && parseFloat(stripStyle.paddingLeft) >= outline,
    `${locale}@${innerWidth}: the clipped tab strip has no room for the focus ring`);
  await key("ArrowRight");
  expect(document.activeElement === tabs()[0] && inView(tabs()[0]), `${locale}@${innerWidth}: ArrowRight did not wrap to a visible first tab`);
  for (let index = 1; index < tabs().length; index++) { await key("ArrowRight"); expect(inView(tabs()[index]), `${locale}@${innerWidth}: tab ${index + 1} stayed hidden after ArrowRight`); }
  await key("Home");
  expect(document.activeElement === tabs()[0] && inView(tabs()[0]) && tabs().filter((tab) => tab.tabIndex === 0).length === 1, `${locale}@${innerWidth}: Home lost roving focus`);
  (document.activeElement as HTMLElement | null)?.blur();
}
async function scrollOwnership() {
  const workspace = select<HTMLElement>(".server-detail-scroll--players");
  const listHeader = select<HTMLElement>(".player-center-pane-header"), memberHeader = select<HTMLElement>(".player-center-member-header");
  const columnHeader = select<HTMLElement>(".player-center-player-table thead th");
  const table = select<HTMLElement>(".player-center-table-scroll");
  const before = { list: rect(listHeader), member: rect(memberHeader), columns: rect(columnHeader) };
  const alpha = (color: string) => { const match = color.match(/rgba?\(([^)]+)\)/); if (!match) return Number.NaN;
    const parts = match[1].split(/[ ,/]+/).filter(Boolean); return parts.length > 3 ? Number(parts[3]) : 1; };
  const headerCells = [...document.querySelectorAll<HTMLElement>(".player-center-player-table thead th")];
  const backgrounds = headerCells.map((cell) => getComputedStyle(cell).backgroundColor);
  expect(headerCells.length > 0 && backgrounds.every((color) => alpha(color) === 1), `Sticky table header is not opaque: ${backgrounds.join(" | ")}`);
  expect(headerCells.every((cell) => parseFloat(getComputedStyle(cell).borderBottomWidth) >= 1), "Sticky table header lost its separator");
  // Both themes: the header matches the pane surface token and stays opaque.
  const probe = document.createElement("div"); probe.style.background = "var(--theme-card-muted)"; document.body.append(probe);
  for (const theme of ["dark", "light"]) {
    document.documentElement.dataset.theme = theme;
    const cell = getComputedStyle(headerCells[0]).backgroundColor;
    expect(cell === getComputedStyle(probe).backgroundColor && alpha(cell) === 1, `${theme}: sticky header ${cell} does not match the opaque pane surface`);
  }
  document.documentElement.dataset.theme = "dark"; probe.remove();
  table.scrollTop = 600; await frames();
  expect(table.scrollTop > 0, "List body did not scroll with 60 players");
  expect(same(before.list, rect(listHeader)) && same(before.columns, rect(columnHeader)) && same(before.member, rect(memberHeader)),
    "Scrolling the list body moved the list header, table column header or 玩家管理 header");
  const moved = scrolled(workspace);
  expect(moved.length === 1 && moved[0].includes("player-center-table-scroll"), `Scrolling the list moved other containers: ${moved.join(", ")}`);
  // Edges: list header, column header and list body share the list pane's inner edges.
  const pane = rect(select(".player-center-list-pane"));
  const body = rect(table);
  expect(Math.abs(before.list.left - body.left) <= 1.5 && Math.abs(before.list.right - body.right) <= 1.5
    && Math.abs(before.columns.left - body.left) <= 1.5 && body.left >= pane.left && body.right <= pane.right,
    "List header, column header and list body edges are misaligned");
  // 玩家管理: its title stays put while the action stack scrolls; the column has a single scroll owner.
  await act(async () => { select<HTMLElement>(".player-center-player-table tbody tr:nth-child(12) .player-center-player-select").click(); }); await frames();
  const controls = select<HTMLElement>(".player-center-controls");
  const owners = verticalOwners(controls);
  const memberBefore = rect(memberHeader), listBefore = rect(listHeader);
  // Tall windows may fit every action; the designated body is still the only element allowed to scroll.
  const actionBody = select<HTMLElement>(".player-center-total-controls");
  expect(owners.length <= 1 && owners.every((owner) => owner === actionBody) && getComputedStyle(actionBody).overflowY === "auto"
    && getComputedStyle(controls).overflowY !== "auto" && !actionBody.contains(memberHeader),
    `玩家管理 column scroll owners: ${owners.map((owner) => owner.className).join(", ") || "none"}`);
  const owner = owners[0];
  if (owner) {
    owner.scrollTop = owner.scrollHeight; await frames();
    expect(owner.scrollTop > 0 && owner !== controls && !owner.contains(memberHeader), "玩家管理 scrolls its title together with the actions");
    expect(same(memberBefore, rect(memberHeader)) && same(listBefore, rect(listHeader)), "Scrolling 玩家管理 moved a header");
    const memberRect = rect(memberHeader), controlsRect = rect(controls), ownerRect = rect(owner);
    expect(Math.abs(memberRect.left - ownerRect.left) <= 1.5 && Math.abs(memberRect.right - ownerRect.right) <= 1.5
      && ownerRect.left >= controlsRect.left && ownerRect.right <= controlsRect.right + 0.5, "玩家管理 header and body edges are misaligned");
    owner.scrollTop = 0;
  }
  expect(workspace.scrollTop === 0 && document.scrollingElement!.scrollTop === 0, "The page or tab scrolled instead of a list body");
  table.scrollTop = 240; await frames();
}
async function run() {
  await act(prepareBrowserLocaleCatalogs);
  const root = createRoot(select("#fixture"));
  await act(async () => root.render(<I18nProvider><Fixture /></I18nProvider>));
  await waitFor(() => Boolean(document.querySelector(".player-center-refresh-button")), "player header");
  const measurements = [];
  for (const locale of ["en-US", "zh-CN"] as LocaleCode[]) {
    await act(async () => { setFixtureLocale(locale); });
    await waitFor(() => select('.player-center-list-tabs [data-list-key="online"]').textContent?.includes(locale === "zh-CN" ? "在线" : "Online") ?? false, `${locale} catalog`);
    await frames(3);
    measurements.push(measureHeader(locale));
    await keyboardReachesEveryTab(locale);
  }
  await scrollOwnership();
  await frames();
  expect(errors.length === 0, `Browser errors: ${errors.join("; ")}`);
  // "status" reports that the fixture ran to completion (so the screenshot is still captured); the layout verdict is "failures".
  return { status: "passed", failures, checks, measurements, browser_errors: errors };
}
let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => { watchdog = setTimeout(() => reject(new Error(`Player tabs header fixture stalled after ${checks} checks`)), 25_000); })])
  .finally(() => clearTimeout(watchdog)).catch((error) => ({ status: "failed", failures, checks, error: String(error), browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
