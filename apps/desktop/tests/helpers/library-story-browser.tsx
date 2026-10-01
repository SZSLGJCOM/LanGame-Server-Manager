import React, { act, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { I18nProvider, useI18n } from "../../src/i18n";
import { LibraryStoryPanel } from "../../src/views/library/LibraryStoryPanel";
import { sanitizeSteamAboutHtml } from "../../src/views/library/steam-about-html";
import type { ModuleStoreEntry } from "../../src/store-media";
import "../../src/app.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN");
sessionStorage.setItem("langameLanToken", "synthetic-story-test-only");
document.documentElement.dataset.theme = "dark";
const nonce = new URLSearchParams(location.search).get("nonce");
const fixture = document.getElementById("fixture")!;
const root = createRoot(fixture);
const retrySelector = ".library-story-panel .shell-activity-notice-actions button";
const errors: string[] = [];
const scenarios: string[] = [];
let checks = 0;
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
const nativeFetch = window.fetch.bind(window);
const nativeNow = Date.now;
let clockOffset = 0;
Date.now = () => nativeNow() + clockOffset;

interface PendingRequest { appId: number; locale: string; completed: boolean; resolve: (response: Response) => void }
const requests: PendingRequest[] = [];
window.fetch = (input, init) => {
  const url = new URL(input instanceof Request ? input.url : String(input), location.href);
  if (url.pathname !== "/__langame/api") return nativeFetch(input, init);
  const payload = JSON.parse(String(init?.body)) as { command: string; args: { appId: number; locale: string } };
  check(payload.command === "fetch_steam_store_about", `Unexpected API command: ${payload.command}`);
  check(new Headers(init?.headers).get("X-LanGame-Token") === "synthetic-story-test-only", "Real LAN transport did not attach its session token");
  return new Promise<Response>((resolve) => requests.push({ appId: payload.args.appId, locale: payload.args.locale, completed: false, resolve }));
};

function check(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
  checks++;
}
function find<T extends Element>(selector: string): T {
  const result = fixture.querySelector<T>(selector);
  if (!result) throw new Error(`Missing element: ${selector}`);
  return result;
}
async function waitFor(predicate: () => boolean, message: string) {
  const deadline = performance.now() + 5000;
  while (!predicate()) {
    if (performance.now() >= deadline) throw new Error(`Timed out: ${message}`);
    await act(async () => { await new Promise<void>((resolve) => setTimeout(resolve, 10)); });
  }
}
function Harness({ appId, show }: { appId: number; show: boolean }) {
  const { locale, setLocale } = useI18n();
  const chinese = locale === "zh-CN";
  const entry: ModuleStoreEntry = {
    storeSource: "steam", storeAppId: appId, storeName: chinese ? "测试游戏" : "Fixture game",
    shortDescription: "", aboutParagraphs: [chinese ? `本地游戏概览 ${appId}` : `Local game overview ${appId}`],
    genres: [], categories: [], developers: [], publishers: [], releaseDate: "", storeUrl: "", officialLinks: [],
    screenshots: [], trailers: [], coverUrl: null
  };
  return <main style={{ width: "min(1040px, calc(100% - 64px))", margin: "32px auto" }}>
    <h1>{chinese ? "游戏详情" : "Game details"}</h1>
    <nav aria-label="Fixture language" style={{ display: "flex", gap: 12, marginBottom: 24 }}>
      <button id="chinese" className="secondary-button" onClick={() => setLocale("zh-CN")}>中文</button>
      <button id="english" className="secondary-button" onClick={() => setLocale("en-US")}>English</button>
      <button id="keyboard-start" className="secondary-button">{chinese ? "键盘检查起点" : "Keyboard check start"}</button>
    </nav>
    {show ? <LibraryStoryPanel storeEntry={entry} storyParagraphs={[]} /> : null}
  </main>;
}
async function render(appId: number, show = true) {
  await act(async () => { root.render(<StrictMode><I18nProvider><Harness appId={appId} show={show} /></I18nProvider></StrictMode>); });
  await waitFor(() => Boolean(fixture.querySelector("#english")), "translated component mounted");
}
async function request(appId: number, locale: string) {
  await waitFor(() => requests.some((entry) => !entry.completed && entry.appId === appId && entry.locale === locale), `request ${appId}/${locale}`);
  return requests.find((entry) => !entry.completed && entry.appId === appId && entry.locale === locale)!;
}
async function respond(pending: PendingRequest, value: string | null, failure = false) {
  pending.completed = true;
  await act(async () => { pending.resolve(new Response(JSON.stringify(failure
    ? { ok: false, error: "fixture store unavailable" } : { ok: true, value }), {
    status: failure ? 503 : 200, headers: { "Content-Type": "application/json" }
  })); });
}
async function click(selector: string) {
  await act(async () => { find<HTMLElement>(selector).click(); });
}
async function key(value: "Tab" | "Enter") {
  await act(async () => {
    const response = await nativeFetch(`/__reliability_key/${nonce}/${value}`, { method: "POST" });
    if (!response.ok) throw new Error(`Keyboard dispatch failed: ${value}`);
  });
}
function loading(appId: number) {
  check(find(".library-story-panel").getAttribute("aria-busy") === "true", "Pending request is not marked busy");
  check(Boolean(find("[role=status]").textContent?.includes("Steam")), "Loading status is missing");
  check(Boolean(find(".library-story-copy").textContent?.includes(String(appId))), "Loading must keep the current local overview visible");
  check(!fixture.querySelector(retrySelector), "Loading exposed a duplicate retry");
}
async function official(text: string) {
  await waitFor(() => Boolean(fixture.querySelector(".library-story-html")?.textContent?.includes(text)), `official story ${text}`);
  check(!fixture.querySelector(".library-story-panel--steam-fallback"), "Successful HTML retained the fallback panel");
}

async function prepareMp4() {
  // Generate a tiny real clip locally: the regression needs a playable alternate,
  // but must not depend on the public CDN or commit binary fixtures.
  const mimeType = "video/mp4;codecs=avc1.42001E";
  check(MediaRecorder.isTypeSupported(mimeType), "Browser must support recording the MP4 fallback fixture");
  const canvas = document.createElement("canvas");
  canvas.width = 64; canvas.height = 64;
  const context = canvas.getContext("2d")!;
  const stream = canvas.captureStream(0);
  const track = stream.getVideoTracks()[0] as CanvasCaptureMediaStreamTrack;
  const recorder = new MediaRecorder(stream, { mimeType });
  const chunks: Blob[] = [];
  const startedAt = performance.now();
  const recordingEvents: { event: string; elapsed: number; bytes?: number }[] = [];
  let frame = 0;
  let draw = 0;
  let timeout: ReturnType<typeof setTimeout> | undefined;
  const diagnostics = () => JSON.stringify({ frame, recordingEvents,
    track: { readyState: track.readyState, muted: track.muted } });
  recorder.onstart = () => recordingEvents.push({ event: "start", elapsed: performance.now() - startedAt });
  recorder.ondataavailable = (event) => {
    recordingEvents.push({ event: "data", elapsed: performance.now() - startedAt, bytes: event.data.size });
    chunks.push(event.data);
    if (event.data.size > 0 && frame >= 3 && recorder.state === "recording") recorder.stop();
  };
  const finished = new Promise<Blob>((resolve, reject) => {
    recorder.onstop = () => {
      recordingEvents.push({ event: "stop", elapsed: performance.now() - startedAt });
      resolve(new Blob(chunks, { type: mimeType }));
    };
    recorder.onerror = () => reject(new Error(`MP4 fixture recording failed: ${diagnostics()}`));
    timeout = setTimeout(() => reject(new Error(`MP4 fixture did not produce encoded frames: ${diagnostics()}`)), 5000);
  });
  const drawFrame = () => {
    context.fillStyle = frame++ % 2 ? "#2fc9a5" : "#1862c4";
    context.fillRect(0, 0, 64, 64);
    track.requestFrame();
    draw = requestAnimationFrame(drawFrame);
  };
  try {
    // Stop after the encoder produces data, not a wall-clock delay that can
    // expire before the browser has started consuming captured frames.
    recorder.start(100);
    draw = requestAnimationFrame(drawFrame);
    const clip = await finished;
    check(clip.size > 0, `MP4 fixture is empty: ${diagnostics()}`);
    const response = await nativeFetch("/__story_animation.mp4", { method: "POST", body: clip });
    check(response.ok, "Could not configure the local MP4 fixture");
    return { frames: frame, bytes: clip.size, events: recordingEvents };
  } finally {
    cancelAnimationFrame(draw); clearTimeout(timeout);
    if (recorder.state !== "inactive") recorder.stop();
    for (const track of stream.getTracks()) track.stop();
  }
}

async function run() {
  // Steam appdetails currently represents animated extras as typed WebM + MP4 sources.
  const mediaTemplate = document.createElement("template");
  mediaTemplate.innerHTML = sanitizeSteamAboutHtml('<video autoplay muted loop playsinline><source src="https://publisher.invalid/story.webm" type="video/webm; codecs=vp9"><source src="https://publisher.invalid/story.mp4" type="video/mp4"></video>');
  const animation = mediaTemplate.content.querySelector("video")!;
  check(animation.querySelectorAll("source").length === 2, "Steam animation lost a format before media setup");
  check(animation.querySelector("source")?.type === "video/webm; codecs=vp9", "Sanitizer removed the codec declaration required for browser capability selection");
  check(animation.querySelectorAll("source")[1].type === "video/mp4", "Sanitizer removed the MP4 alternative type");
  check(animation.defaultMuted && animation.autoplay && animation.loop, "Sanitizer lost Steam animation playback attributes");
  scenarios.push("video-source-types");
  await Promise.all([import("../../src/i18n-messages"), import("../../src/i18n-messages-zh-cn")]);
  const mp4Fixture = await prepareMp4();
  await render(901000);
  await respond(await request(901000, "zh-CN"), `<h2>动画格式恢复</h2><video autoplay muted loop playsinline><source src="${location.origin}/__story_animation.webm" type="video/webm; codecs=vp9"><source src="${location.origin}/__story_animation.mp4" type="video/mp4"></video>`);
  await official("动画格式恢复");
  const recoveredVideo = find<HTMLVideoElement>(".library-story-html video");
  await waitFor(() => recoveredVideo.currentSrc.endsWith("/__story_animation.mp4") && recoveredVideo.currentTime > 0.02, "failed WebM recovered to a playing MP4");
  check(recoveredVideo.muted && !recoveredVideo.paused, "Recovered Steam animation must autoplay muted");
  check(!recoveredVideo.querySelector("source[src]"), "Managed playback left child sources that can revive after cancellation");
  scenarios.push("video-format-recovery");
  await render(901001);
  check(recoveredVideo.paused && !recoveredVideo.hasAttribute("src"), "Leaving the story did not release its active animation");
  const first = await request(901001, "zh-CN");
  loading(901001);
  scenarios.push("loading");
  await respond(first, null, true);
  await waitFor(() => Boolean(fixture.querySelector(retrySelector)), "Chinese retry");
  check(find("[role=alert]").textContent?.includes("加载失败"), "Chinese request failure was not explicit");
  check(find("[role=alert]").textContent?.includes("当前显示本地概览"), "Chinese fallback is not identified as local");
  check(find(retrySelector).textContent === "重试", "Chinese retry is not localized");
  find<HTMLElement>("#keyboard-start").focus();
  await key("Tab");
  check(document.activeElement === fixture.querySelector(retrySelector), "Retry is not reachable by native Tab");
  await key("Enter");
  const retry = await request(901001, "zh-CN");
  loading(901001);
  scenarios.push("zh-retry-keyboard");
  await respond(retry, '<div style="width:99999px" onclick="globalThis.storyUnsafe=true"><h2>官方故事 901001</h2><p>Steam 官方完整描述。</p><script>globalThis.storyUnsafe=true</script><iframe src="https://invalid.example/"></iframe><a href="javascript:globalThis.storyUnsafe=true">unsafe link</a><img src="javascript:globalThis.storyUnsafe=true" onerror="globalThis.storyUnsafe=true"></div>');
  await official("官方故事 901001");
  check(!fixture.querySelector(".library-story-html script, .library-story-html iframe, .library-story-html [onclick], .library-story-html [onerror], .library-story-html [style]"), "Unsafe markup survived sanitization");
  check(!fixture.querySelector(".library-story-html a")?.hasAttribute("href"), "Unsafe link scheme survived sanitization");
  check(!fixture.querySelector(".library-story-html img")?.hasAttribute("src"), "Unsafe image scheme survived sanitization");
  check(!("storyUnsafe" in globalThis), "Untrusted HTML executed script");
  scenarios.push("html-sanitization");

  const beforeCache = requests.length;
  await render(901001, false); clockOffset += 9 * 60 * 1000;
  await render(901001); await official("官方故事 901001");
  check(requests.length === beforeCache, "Successful remount did not reuse the ten-minute cache");
  await render(901001, false); clockOffset += 61 * 1000;
  await render(901001);
  await respond(await request(901001, "zh-CN"), "<h2>刷新后的官方故事</h2>");
  await official("刷新后的官方故事");
  check(requests.length === beforeCache + 1, "Expired story did not issue exactly one new request");
  scenarios.push("success-cache-expiry");

  await render(901001, false); clockOffset += 11 * 60 * 1000;
  await render(901001);
  await respond(await request(901001, "zh-CN"), null, true);
  await official("刷新后的官方故事");
  await waitFor(() => Boolean(fixture.querySelector(retrySelector)), "Saved-story retry");
  check(find("[role=status]").textContent?.includes("上次成功保存"), "Saved story must be identified as previously saved content");
  await click(retrySelector);
  await respond(await request(901001, "zh-CN"), "<h2>网络恢复后的官方故事</h2>");
  await official("网络恢复后的官方故事");
  check(!fixture.querySelector(retrySelector), "Successful refresh retained the saved-story warning");
  scenarios.push("saved-story-recovery");

  let retainedStory = "网络恢复后的官方故事";
  for (const [scenario, emptyResponse] of [
    ["null-saved-story-recovery", null],
    ["sanitized-empty-saved-story-recovery", "<script>globalThis.storyUnsafe=true</script>"]
  ] as const) {
    await render(901001, false); clockOffset += 11 * 60 * 1000;
    await render(901001);
    await respond(await request(901001, "zh-CN"), emptyResponse);
    await official(retainedStory);
    await waitFor(() => Boolean(fixture.querySelector(retrySelector)), "Unavailable-story retry");
    check(find("[role=status]").textContent?.includes("上次成功保存"), "Unavailable response must identify the saved description");
    check(!("storyUnsafe" in globalThis), "Sanitized-empty response executed script");
    await click(retrySelector);
    check(find(".library-story-panel").getAttribute("aria-busy") === "true", "Saved story retry is not busy");
    check(find(".library-story-html").textContent?.includes(retainedStory), "Retry hid the saved description");
    retainedStory = `${scenario} 官方内容已恢复`;
    await respond(await request(901001, "zh-CN"), `<h2>${retainedStory}</h2>`);
    await official(retainedStory);
    check(!fixture.querySelector(retrySelector), "Recovered description retained the saved-content warning");
    scenarios.push(scenario);
  }

  await click("#english");
  loading(901001);
  await respond(await request(901001, "en-US"), null, true);
  await waitFor(() => Boolean(fixture.querySelector(retrySelector)), "English retry");
  check(find("[role=alert]").textContent?.includes("Could not load the Steam store description"), "English request failure was not localized");
  check(find("[role=alert]").textContent?.includes("Showing the local overview"), "English fallback is not identified as local");
  check(find(retrySelector).textContent === "Retry", "English retry is not localized");
  await click(retrySelector);
  await respond(await request(901001, "en-US"), "<h2>Official English story</h2>");
  await official("Official English story");
  scenarios.push("en-retry");

  await render(901002);
  await respond(await request(901002, "en-US"), null);
  await waitFor(() => Boolean(fixture.querySelector(retrySelector)), "Empty-response retry");
  check(find("[role=status]").textContent?.includes("unavailable right now"), "Empty response incorrectly reported success");
  await click(retrySelector);
  await respond(await request(901002, "en-US"), "<h2>Recovered empty response</h2>");
  await official("Recovered empty response");
  scenarios.push("null-retry");

  await render(901003);
  const oldGame = await request(901003, "en-US");
  await render(901004);
  const newGame = await request(901004, "en-US");
  await respond(oldGame, "<h2>OBSOLETE GAME</h2>");
  loading(901004);
  check(!fixture.textContent?.includes("OBSOLETE GAME"), "Late previous-game response replaced the current view");
  await respond(newGame, "<h2>Current game English</h2>");
  await official("Current game English");
  scenarios.push("obsolete-game");

  await click("#chinese");
  const oldLocale = await request(901004, "zh-CN");
  await click("#english"); await official("Current game English");
  await respond(oldLocale, "<h2>OBSOLETE LOCALE</h2>");
  check(fixture.textContent?.includes("Current game English") && !fixture.textContent?.includes("OBSOLETE LOCALE"), "Late previous-language response replaced the selected language");
  scenarios.push("obsolete-locale");

  await render(901006);
  const oldNull = await request(901006, "en-US");
  await render(901007);
  await respond(oldNull, null);
  await respond(await request(901007, "en-US"), "<h2>Other game</h2>");
  await render(901006);
  await respond(await request(901006, "en-US"), "<h2>Recovered after leaving the page</h2><p>The official Steam description is available again.</p>");
  await official("Recovered after leaving the page");
  scenarios.push("obsolete-null");

  check(document.documentElement.scrollWidth <= innerWidth, "Story layout overflows the desktop viewport");
  check(find(".library-story-panel").getBoundingClientRect().width > 500, "Story panel has no usable desktop width");
  check(requests.every((entry) => entry.completed), "Fixture left an unresolved API response");
  check(errors.length === 0, `Browser errors: ${errors.join("; ")}`);
  return { status: "passed", checks, scenarios, requests: requests.length, mp4_fixture: mp4Fixture,
    viewport: { width: innerWidth, height: innerHeight }, browser_errors: errors };
}

let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => {
  watchdog = setTimeout(() => reject(new Error(`Story browser test stalled after ${checks} checks`)), 25000);
})]).catch((error: unknown) => ({ status: "failed", checks, scenarios,
  error: error instanceof Error ? error.stack : String(error), browser_errors: errors }))
  .finally(() => { clearTimeout(watchdog); Date.now = nativeNow; window.fetch = nativeFetch; })
  .then((report) => nativeFetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
