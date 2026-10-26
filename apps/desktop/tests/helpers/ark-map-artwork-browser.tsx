import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import React, { act, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { invokeMock } from "../../src/api-mock";
import { I18nProvider, useI18n, type LocaleCode } from "../../src/i18n";
import { GMToolsWorkbench } from "../../src/views/servers/GMToolsWorkbench";
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
  if (command === "read_ark_tools_status") return { installed: true, connected: true, issue: null };
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
async function render(nextMap = mapName, edition = details.summary.module_id) {
  mapName = nextMap;
  details = { ...details, summary: { ...details.summary, module_id: edition },
    settings_json: JSON.stringify({ map_name: mapName, rcon_enabled: true, admin_password: 'fixture-admin-password' }) };
  await act(async () => root.render(<StrictMode><I18nProvider><LocaleControl /><main style={{ padding: 16, height: '100vh', boxSizing: 'border-box' }}>
    <GMToolsWorkbench details={details} /></main></I18nProvider></StrictMode>));
}
async function run() {
  await act(prepareBrowserLocaleCatalogs);
  const evidence = await (await fetch('/__ark_artwork_mode')).json();
  const bootstrap = await invokeMock<BootstrapResponse>('bootstrap', { includeSystemSnapshot: false });
  const instance = bootstrap.state.instances.find(value => value.module_id === 'minecraft')!;
  const base = await invokeMock<InstanceDetails>('read_instance_details_from_storage', { instanceId: instance.id, instance_id: instance.id });
  details = { ...base, summary: { ...base.summary, id: 'ark-artwork-fixture', module_id: 'arksurvivalascended', status: 'Stopped', active_process_count: 0 }, active_run: null };
  failImage = true;
  await render(); await settle(() => fixture.textContent?.includes('坐标网格 · 底图不可用') ?? false, 'Stopped map source failure');
  check(!element<HTMLButtonElement>('.ark-map-toolbar button').disabled, 'A failed artwork request can be retried without starting the server');
  failImage = false;
  await act(async () => element<HTMLButtonElement>('.ark-map-toolbar button').click());
  await settle(() => imageName() === 'The_Island_map_ASA.jpg', 'Stopped maps recover known artwork after network recovery'); checks++;
  check(requested[0].startsWith('http://lgsm-media.localhost/') && originalSource(requested[0]).hostname === 'api.gitcode.com',
    'Chinese artwork initially uses the domestic mirror through the desktop cache');
  check(image()?.getAttribute('width') === '1000' && image()?.getAttribute('x') === '0', 'ASA native minimap covers GPS 0–100');
  check(element<HTMLAnchorElement>('.ark-map-source a').href === 'https://ark.wiki.gg/wiki/File:The_Island_map_ASA.jpg', 'Artwork has an accurate clickable source');
  check(fixture.textContent?.includes('地图未运行'), 'Artwork does not invent live players on a stopped server');
  for (const [locale, host] of [['en-US', 'raw.githubusercontent.com'], ['zh-CN', 'api.gitcode.com']] as const) {
    const before = requested.length;
    await act(async () => changeLocale!(locale));
    await settle(() => requested.length > before && imageName() === 'The_Island_map_ASA.jpg', 'Locale change reloads through the requested route');
    check(originalSource(requested[before]).hostname === host && new URL(requested[before]).searchParams.get('locale') === locale,
      `${locale} selects the expected source and passes its preference to the native cache`);
  }
  const stoppedRequests = requested.length;
  details = { ...details, summary: { ...details.summary, status: 'Running', active_process_count: 1 },
    active_run: { run_id: 7, processes: [{ process_key: 'main', run_id: 7, status: 'Running' }] } };
  await render(); await settle(() => Boolean(fixture.querySelector('[data-player-id]')), 'Live players overlay the real artwork');
  check(requested.length === stoppedRequests, 'Receiving player snapshots never redownloads artwork');
  check(element('[data-player-id]').getAttribute('transform') === 'translate(387.5 575)', 'Image display preserves the native player coordinate transform');
  await render('TheIsland', 'arksurvivalevolved'); await settle(() => imageName() === 'The_Island_Topographic_Map.jpg', 'ASE selects its own image');
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
  await render('Ragnarok_WP'); await settle(() => fixture.textContent?.includes('坐标网格 · 底图不可用') ?? false, 'Failed sources fall back to grid');
  check(!image() && requested.filter(value => value.includes('Ragnarok_map_ASA')).length <= 4, 'Unavailable artwork has bounded attempts and no broken image');
  failImage = false;
  await act(async () => element<HTMLButtonElement>('.ark-map-toolbar button').click());
  await settle(() => imageName() === 'Ragnarok_map_ASA.jpg', 'Explicit refresh recovers an unavailable source'); checks++;
  const beforeUnknown = requested.length;
  await render('CustomMap_WP'); await settle(() => fixture.textContent?.includes('坐标网格 · 底图不可用') ?? false, 'Unknown map grid');
  check(!image() && requested.length === beforeUnknown, 'Unknown maps never guess a URL or reuse another map image');
  projection = false;
  await render('TheIsland_WP'); await settle(() => fixture.textContent?.includes('原生 X / Y') ?? false, 'World XY view');
  check(!image(), 'An explicit missing projection cannot place native XY on a GPS image');
  projection = true;
  await render('TheIsland', 'arksurvivalevolved'); await settle(() => imageName() === 'The_Island_Topographic_Map.jpg', 'Restore a mapped edition');
  await render('TheIsland_WP', 'arksurvivalascended'); await settle(() => imageName() === 'The_Island_map_ASA.jpg', 'Final ASA terrain');
  await act(async () => element<HTMLButtonElement>('.ark-map-roster-list button').click());
  check(document.documentElement.scrollWidth <= innerWidth + 1, 'Artwork and source link have no horizontal overflow');
  check(errors.length === 0, `Browser errors: ${errors.join('; ')}`);
  return { status: 'passed', checks, browser_errors: errors, artwork_evidence: evidence.real ? 'downloaded-wiki-images' : 'controlled-image-boundary', image_requests: requested.length };
}
void run().catch(error => ({ status: 'failed', checks, error: error instanceof Error ? error.stack : String(error), browser_errors: errors }))
  .finally(() => Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }))
  .then(report => fetch(`/__reliability_result/${nonce}`, { method: 'POST', body: JSON.stringify(report) }));
