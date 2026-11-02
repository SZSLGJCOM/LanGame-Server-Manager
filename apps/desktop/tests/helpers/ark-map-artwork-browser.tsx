import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import React, { act, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { invokeMock } from "../../src/api-mock";
import { I18nProvider, useI18n, type LocaleCode } from "../../src/i18n";
import { GMToolsWorkbench } from "../../src/views/servers/GMToolsWorkbench";
import { ArkToolsInstallationProvider } from "../../src/components/ArkToolsInstallationContext";
import { InstanceSettingsSaveProvider } from "../../src/views/settings/InstanceSettingsSaveContext";
import type { BootstrapResponse, InstanceDetails } from "../../src/types";
import type { ArkMapScope, ArkMapSnapshot } from "../../src/ark-world-map";
import "../../src/app.css";
import "../../src/views/servers/workbench.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN"); document.documentElement.dataset.theme = "dark";
const fixture = document.getElementById("fixture")!, root = createRoot(fixture);
const nonce = new URLSearchParams(location.search).get("nonce");
const errors: string[] = [], requested: string[] = [];
addEventListener("error", event => errors.push(event.message));
addEventListener("unhandledrejection", event => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
let checks = 0, details: InstanceDetails, mapName = "TheIsland_WP", projection = true;
let failImage = false, failMirrors = false, holdImage = false, releaseImage: (() => void) | undefined;
let changeLocale: ((locale: LocaleCode) => void) | undefined;
function LocaleControl() { changeLocale = useI18n().setLocale; return null; }
function originalSource(value: string) {
  const url = new URL(value, location.href);
  return url.hostname === 'lgsm-media.localhost' ? new URL(url.searchParams.get('url')!) : url;
}
const imageSrc = Object.getOwnPropertyDescriptor(HTMLImageElement.prototype, "src")!;
Object.defineProperty(HTMLImageElement.prototype, "src", { configurable: true, get: imageSrc.get,
  set(value: string) {
    const source = originalSource(value);
    if (!['ark.wiki.gg', 'raw.githubusercontent.com', 'api.gitcode.com'].includes(source.hostname)) { imageSrc.set!.call(this, value); return; }
    requested.push(value);
    if (failImage || failMirrors && source.hostname !== 'ark.wiki.gg') { queueMicrotask(() => this.dispatchEvent(new Event('error'))); return; }
    const load = () => imageSrc.set!.call(this, `/__ark_artwork/${encodeURIComponent(source.pathname.split('/').at(-1)!)}`);
    if (holdImage) { releaseImage = load; return; }
    load();
  }
});
Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: { invoke: async (command: string, args: { input: ArkMapScope }) => {
  if (command === "read_ark_tools_install_progress") return null;
  if (command === "read_ark_tools_status") return { installed: true, connected: true, canUninstall: true, issue: null };
  if (command === "read_ark_map") return { ...args.input, mapName, sampledAtUnixMs: Date.now(),
    projection: projection ? { minX: -400000, maxX: 400000, minY: -400000, maxY: 400000 } : null, artwork: null,
    players: [{ playerId: 123, name: "地图验收玩家", accountId: "fixture-account", team: 1000, x: -90000, y: 60000, z: 900 }]
  } satisfies ArkMapSnapshot;
  throw new Error(`Unexpected IPC: ${command}`);
} } });
function check(value: unknown, message: string): asserts value { if (!value) throw new Error(message); checks++; }
function element<T extends Element = HTMLElement>(selector: string): T { const node = fixture.querySelector<T>(selector); if (!node) throw new Error(`Missing ${selector}`); return node; }
async function settle(predicate: () => boolean, message: string) {
  const until = performance.now() + 6000;
  while (!predicate()) { if (performance.now() > until) throw new Error(`${message}: ${fixture.textContent?.slice(0, 300)}`);
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 15)); }); }
}
const image = () => fixture.querySelector('svg image');
const imageName = () => image()?.getAttribute('href')?.split('/').at(-1);
const artworkSettled = () => fixture.querySelector('.ark-map-canvas')?.getAttribute('aria-busy') === 'false';
function layoutGeometry(game: 'ark' | 'dst') {
  return {
    navigation: element('.gmt-tools-sidebar').getBoundingClientRect(),
    map: element(game === 'ark' ? '.ark-map-canvas' : '.dst-map-empty').getBoundingClientRect(),
    rail: element(game === 'ark' ? '.ark-map-rail' : '.dst-map-sidebar').getBoundingClientRect(),
    tool: element(game === 'ark' ? '.ark-world-tool-grid button.ark-spawn-tool' : 'button.dst-gm-tool').getBoundingClientRect()
  };
}
function checkArkLayout(reference: ReturnType<typeof layoutGeometry>, label: string) {
  const actual = layoutGeometry('ark'), region = element('.ark-world-map').getBoundingClientRect();
  for (const part of ['navigation', 'map', 'rail'] as const) {
    check((['left', 'top', 'width', 'height'] as const).every(key => Math.abs(actual[part][key] - reference[part][key]) <= 1),
    `${label}: ${part} must align with the real DST workbench in the same host: ${JSON.stringify({ actual: actual[part].toJSON(), expected: reference[part].toJSON() })}`);
  }
  check(Math.abs(actual.map.width - actual.map.height) <= 1
    && Math.abs(actual.map.top - region.top) <= 1 && Math.abs(actual.map.left - region.left) <= 1,
    `${label}: the map must be square and flush with the top-left corner`);
  if (region.width >= region.height + 236) check(Math.abs(actual.map.height - region.height) <= 1,
    `${label}: the square map fills the available height when it fits beside DST's rail`);
  check(Math.abs(actual.rail.right - region.right) <= 1 && Math.abs(actual.rail.height - region.height) <= 1,
    `${label}: the control rail fills the remaining region and aligns with its edges`);
  const mapChoice = element('.ark-map-toolbar select').getBoundingClientRect(), refresh = element('.ark-map-toolbar button').getBoundingClientRect();
  check(mapChoice.right < refresh.left && refresh.top >= mapChoice.top && refresh.bottom <= mapChoice.bottom,
    `${label}: map selection and refresh stay on the same row without reducing catalog space`);
  const cells = [...fixture.querySelectorAll('.ark-map-rail .ark-spawn-tool, .ark-map-rail .ark-spawn-setting')]
    .map(node => node.getBoundingClientRect()).filter(box => box.width > 0);
  check(cells.length > 0 && cells.every(box => Math.abs(box.width - reference.tool.width) <= 1 && Math.abs(box.height - reference.tool.height) <= 1),
    `${label}: every action and input cell uses the same fixed height and four-column width as DST`);
}
async function render(nextMap = mapName, edition = details.summary.module_id) {
  mapName = nextMap;
  details = { ...details, summary: { ...details.summary, module_id: edition },
    settings_json: JSON.stringify({ map_name: mapName, rcon_enabled: true, admin_password: 'fixture-admin-password' }) };
  await act(async () => root.render(<StrictMode><I18nProvider><ArkToolsInstallationProvider><InstanceSettingsSaveProvider><LocaleControl /><main style={{ padding: 16, height: '100vh', boxSizing: 'border-box' }}>
    <GMToolsWorkbench details={details} /></main></InstanceSettingsSaveProvider></ArkToolsInstallationProvider></I18nProvider></StrictMode>));
}
async function run() {
  await act(prepareBrowserLocaleCatalogs);
  const evidence = await (await fetch('/__ark_artwork_mode')).json();
  const bootstrap = await invokeMock<BootstrapResponse>('bootstrap', { includeSystemSnapshot: false });
  const instance = bootstrap.state.instances.find(value => value.module_id === 'minecraft')!;
  const base = await invokeMock<InstanceDetails>('read_instance_details_from_storage', { instanceId: instance.id, instance_id: instance.id });
  details = { ...base, summary: { ...base.summary, id: 'dst-layout-reference', module_id: 'dontstarve', status: 'Stopped', active_process_count: 0 }, active_run: null };
  await render();
  await settle(() => Boolean(fixture.querySelector('.dst-map-empty')), 'DST geometry reference');
  const referenceLayout = layoutGeometry('dst');
  details = { ...base, summary: { ...base.summary, id: 'ark-artwork-fixture', module_id: 'arksurvivalascended', status: 'Stopped', active_process_count: 0 }, active_run: null };
  failImage = true;
  await render(); await settle(artworkSettled, 'Stopped map source failure');
  check(!element<HTMLButtonElement>('.ark-map-toolbar button').disabled, 'A failed artwork request can be retried without starting the server');
  failImage = false;
  await act(async () => element<HTMLButtonElement>('.ark-map-toolbar button').click());
  await settle(() => imageName() === 'The_Island_map_ASA.jpg', 'Stopped maps recover known artwork after network recovery'); checks++;
  checkArkLayout(referenceLayout, 'ASA');
  check(requested[0].startsWith('http://lgsm-media.localhost/') && originalSource(requested[0]).hostname === 'api.gitcode.com',
    'Chinese artwork initially uses the domestic mirror through the desktop cache');
  check(image()?.getAttribute('width') === '1000' && image()?.getAttribute('x') === '0', 'ASA native minimap covers GPS 0–100');
  check(element<HTMLAnchorElement>('.ark-map-source a').href === 'https://ark.wiki.gg/wiki/File:The_Island_map_ASA.jpg', 'Artwork has an accurate clickable source');
  check(element('.ark-map-canvas').getAttribute('data-live') === 'false' && !fixture.querySelector('[data-player-id]')
    && !/地图未运行|启动后读取|地图底图|GPS|Lat \/ Lon/.test(fixture.textContent ?? ''),
    'A stopped server shows artwork without invented players or explanatory text on the map or roster');
  check([...fixture.querySelectorAll('.map-zoom-controls, .map-zoom-controls button, .map-zoom-controls select, .ark-map-source')]
    .every(node => getComputedStyle(node).backgroundColor === 'rgba(0, 0, 0, 0)'), 'Map controls and attribution have transparent backgrounds');
  check([...fixture.querySelectorAll('.map-zoom-controls button')].every(node => {
    const box = node.getBoundingClientRect(); return box.width === 26 && box.height === 26;
  }) && element('.map-zoom-select').getBoundingClientRect().width === 84
    && Boolean(fixture.querySelector('.map-zoom-controls [aria-label="适应窗口"] svg')),
  'ARK reuses the compact DST zoom selector and icon-only fit button');
  for (const [locale, host] of [['en-US', 'raw.githubusercontent.com'], ['zh-CN', 'api.gitcode.com']] as const) {
    const before = requested.length;
    await act(async () => changeLocale!(locale));
    await settle(() => requested.length > before && imageName() === 'The_Island_map_ASA.jpg', 'Locale change reloads through the requested route');
    check(originalSource(requested[before]).hostname === host && new URL(requested[before]).searchParams.get('locale') === locale,
      `${locale} selects the expected source and passes its preference to the native cache`);
    check(element('.ark-map-roster-list').textContent === '', `${locale} omits stopped-map reading instructions`);
  }
  const stoppedRequests = requested.length;
  details = { ...details, summary: { ...details.summary, status: 'Running', active_process_count: 1 },
    active_run: { run_id: 7, processes: [{ process_key: 'main', run_id: 7, status: 'Running' }] } };
  await render(); await settle(() => Boolean(fixture.querySelector('[data-player-id]')), 'Live players overlay the real artwork');
  check(requested.length === stoppedRequests, 'Receiving player snapshots never redownloads artwork');
  check(element('[data-player-id]').getAttribute('transform') === 'translate(387.5 575)', 'Image display preserves the native player coordinate transform');
  await render('TheIsland', 'arksurvivalevolved'); await settle(() => imageName() === 'The_Island_Topographic_Map.jpg', 'ASE selects its own image');
  checkArkLayout(referenceLayout, 'ASE');
  check(image()?.getAttribute('x') === '72' && image()?.getAttribute('y') === '72' && image()?.getAttribute('width') === '856', 'ASE topography uses the verified 7.2–92.8 GPS rectangle');
  holdImage = true;
  await render('ScorchedEarth_WP', 'arksurvivalascended'); await settle(() => Boolean(releaseImage), 'Hold the old map image request');
  check(!image(), 'Changing maps removes the previous terrain before the replacement arrives');
  const release = releaseImage!; holdImage = false;
  await render('TheCenter_WP'); await settle(() => imageName() === 'The_Center_map_ASA.jpg', 'The next map loads independently');
  await act(async () => { release(); await new Promise(resolve => setTimeout(resolve, 80)); });
  check(imageName() === 'The_Center_map_ASA.jpg', 'A late old-map image cannot replace the current map');
  failMirrors = true;
  const beforeFallback = requested.length;
  await render('Aberration_WP'); await settle(() => imageName() === 'Aberration_map_ASA.jpg', 'Exhausted mirror cache falls back to Wiki');
  const fallbackRequests = requested.slice(beforeFallback);
  check(originalSource(fallbackRequests.at(-1)!).hostname === 'ark.wiki.gg', 'Wiki is the final independent image source');
  check(fallbackRequests.every(value => originalSource(value).hostname === 'ark.wiki.gg' || new URL(value).hostname === 'lgsm-media.localhost'),
    'Failed mirror cache cannot bypass integrity checks through direct mirror image loads');
  failMirrors = false;
  failImage = true;
  await render('Ragnarok_WP'); await settle(artworkSettled, 'Failed sources fall back to grid');
  check(!image() && requested.filter(value => value.includes('Ragnarok_map_ASA')).length <= 4, 'Unavailable artwork has bounded attempts and no broken image');
  failImage = false;
  await act(async () => element<HTMLButtonElement>('.ark-map-toolbar button').click());
  await settle(() => imageName() === 'Ragnarok_map_ASA.jpg', 'Explicit refresh recovers an unavailable source'); checks++;
  const beforeUnknown = requested.length;
  await render('CustomMap_WP'); await settle(artworkSettled, 'Unknown map grid');
  check(!image() && requested.length === beforeUnknown, 'Unknown maps never guess a URL or reuse another map image');
  projection = false;
  await render('TheIsland_WP'); await settle(() => [...fixture.querySelectorAll('.ark-map-grid-label text')]
    .some(node => Math.abs(Number(node.textContent)) > 100), 'World XY view');
  check(!image(), 'An explicit missing projection cannot place native XY on a GPS image');
  projection = true;
  await render('TheIsland', 'arksurvivalevolved'); await settle(() => imageName() === 'The_Island_Topographic_Map.jpg', 'Restore a mapped edition');
  await render('TheIsland_WP', 'arksurvivalascended'); await settle(() => imageName() === 'The_Island_map_ASA.jpg', 'Final ASA terrain');
  await act(async () => element<HTMLButtonElement>('.ark-map-roster-list button').click());
  const mapCanvas = element('.ark-map-surface');
  const itemPanel = element<HTMLButtonElement>('[data-configuration-section-id="ark_give_item_to_player"] button');
  if (!itemPanel.getClientRects().length) await act(async () => element<HTMLButtonElement>('.configuration-workspace__navigation-toggle').click());
  await act(async () => itemPanel.click());
  check(element('.ark-map-surface') === mapCanvas && imageName() === 'The_Island_map_ASA.jpg',
    'Changing the spawn panel in the left navigation preserves the loaded map');
  check(!element<HTMLButtonElement>('.gmt-form button[type="submit"]').disabled
    && !fixture.querySelector('.gmt-form .ark-map-use-player'), 'The selected player is immediately ready for item delivery');
  const actionBox = element('.gmt-form button[type="submit"]').getBoundingClientRect();
  const listBox = element('.gmt-form .gmt-ark-catalog-results').getBoundingClientRect();
  check(listBox.height >= 52 && listBox.bottom < actionBox.top && actionBox.bottom <= innerHeight - 16,
    'Real artwork remains visible beside the compact catalog and fixed item actions');
  check(document.documentElement.scrollWidth <= innerWidth + 1, 'Artwork and source link have no horizontal overflow');
  checkArkLayout(referenceLayout, 'ASA with the item panel');
  check(errors.length === 0, `Browser errors: ${errors.join('; ')}`);
  return { status: 'passed', checks, browser_errors: errors, artwork_evidence: evidence.real ? 'downloaded-wiki-images' : 'controlled-image-boundary', image_requests: requested.length };
}
void run().catch(error => ({ status: 'failed', checks, error: error instanceof Error ? error.stack : String(error), browser_errors: errors }))
  .finally(() => Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }))
  .then(report => fetch(`/__reliability_result/${nonce}`, { method: 'POST', body: JSON.stringify(report) }));
