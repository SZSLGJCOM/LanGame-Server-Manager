const assert = require("node:assert/strict");
const test = require("node:test");
const net = require("node:net");
const fs = require("node:fs/promises");
const path = require("node:path");
const { execFile } = require("node:child_process");
const { promisify } = require("node:util");
const { createHash, randomUUID } = require("node:crypto");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");
const desktopCsp = require("../src-tauri/tauri.conf.json").app.security.csp;

// Both modes require explicit opt-in. Captures must come from the real Rust
// production resolver probe; this test never creates or substitutes story HTML.
// The runtime mode neither starts nor stops the selected user's service.
const runtimeRequested = Boolean(process.env.LANGAME_STEAM_RUNTIME_PID);
const expectedPid = Number(process.env.LANGAME_STEAM_RUNTIME_PID);
const probeFile = process.env.LANGAME_STORY_PROBE_FILE;
const apps = [322330, 252490];
const locales = ["zh-CN", "en-US"];

function requestRuntime(pipe, command, args) {
  return new Promise((resolve, reject) => {
    const socket = net.createConnection(pipe);
    let bytes = Buffer.alloc(0);
    let settled = false;
    const finish = (error, value) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      socket.destroy();
      if (error) reject(error); else resolve(value);
    };
    const timer = setTimeout(() => finish(new Error("Steam runtime probe timed out")), 22000);
    socket.once("error", (error) => finish(error));
    socket.once("close", () => finish(new Error("Steam runtime probe disconnected")));
    socket.once("connect", () => {
      const body = Buffer.from(JSON.stringify({ protocol: 1, command, args }));
      const header = Buffer.alloc(4);
      header.writeUInt32LE(body.length);
      socket.write(Buffer.concat([header, body]));
    });
    socket.on("data", (chunk) => {
      if (bytes.length + chunk.length > 16 * 1024 * 1024 + 4) return finish(new Error("Oversized runtime response"));
      bytes = Buffer.concat([bytes, chunk]);
      if (bytes.length < 4) return;
      const size = bytes.readUInt32LE(0);
      if (!size || size > 16 * 1024 * 1024) return finish(new Error("Invalid runtime frame"));
      if (bytes.length < size + 4) return;
      try {
        const response = JSON.parse(bytes.subarray(4, size + 4));
        if (typeof response.result?.Err === "string") return finish(new Error(response.result.Err));
        if (!Object.hasOwn(response.result ?? {}, "Ok")) throw new Error("Invalid runtime result");
        finish(null, response.result.Ok);
      } catch (error) { finish(error); }
    });
  });
}

test("real production Steam stories render their text, images and available animations", {
  skip: !runtimeRequested && !probeFile && "Set LANGAME_STEAM_RUNTIME_PID or LANGAME_STORY_PROBE_FILE to opt into production story verification",
  timeout: 270000,
}, async () => {
  assert.equal(process.platform, "win32");
  assert.ok(!(runtimeRequested && probeFile), "Select exactly one production story verification mode");
  let pipe;
  let probeSha256;
  let captured;
  if (probeFile) {
    assert.ok(path.isAbsolute(probeFile), "Production probe JSON path must be absolute");
    const stat = await fs.stat(probeFile);
    assert.ok(stat.isFile() && stat.size > 0 && stat.size <= 16 * 1024 * 1024, "Production probe JSON must be a nonempty file at most 16 MiB");
    const bytes = await fs.readFile(probeFile);
    const payload = JSON.parse(bytes.toString("utf8"));
    assert.ok(Array.isArray(payload.stories) && payload.stories.length > 0 && payload.stories.length <= 10,
      "Production probe JSON must contain between 1 and 10 stories");
    captured = new Map();
    for (const story of payload.stories) {
      assert.ok(story && Number.isSafeInteger(story.appId) && story.appId > 0 && locales.includes(story.locale),
        "Every captured story needs a positive app ID and a supported locale");
      assert.ok(typeof story.html === "string" && story.html.trim().length > 0 && Buffer.byteLength(story.html) <= 1024 * 1024,
        "Every captured story needs nonempty production HTML at most 1 MiB");
      const key = `${story.appId}:${story.locale}`;
      assert.ok(!captured.has(key), "Captured app and language pairs must be unique");
      captured.set(key, story);
    }
    probeSha256 = createHash("sha256").update(bytes).digest("hex");
  } else {
    assert.ok(Number.isSafeInteger(expectedPid) && expectedPid > 0);
    const { stdout } = await promisify(execFile)("whoami.exe", ["/user", "/fo", "csv", "/nh"], { windowsHide: true, timeout: 3000 });
    const sid = stdout.match(/S-1-5-21-\d+-\d+-\d+-\d+/)?.[0];
    assert.ok(sid, "Current Windows account SID is required");
    pipe = `\\\\.\\pipe\\LanGame.Runtime.${sid}`;
    const status = await requestRuntime(pipe, "runtime_service_status", {});
    assert.equal(status.pid, expectedPid, "Only the explicitly selected running backend may be probed");
  }
  const stories = captured ? [...captured.values()].map(({ appId, locale }) => ({ appId, locale }))
    : apps.flatMap((appId) => locales.map((locale) => ({ appId, locale })));
  const sourceMode = captured ? "captured production regional API" : "running production backend";
  const token = randomUUID();
  const requests = [];
  const fixtureMiddleware = (request, response, next) => {
    if (request.url === "/__steam_probe_config") {
      response.writeHead(200, { "Content-Type": "application/json", "Cache-Control": "no-store" });
      response.end(JSON.stringify({ token, stories, sourceMode }));
      return;
    }
    if (request.url !== "/__langame/api") return next();
    if (request.method !== "POST" || request.headers["x-langame-token"] !== token) {
      response.writeHead(403).end(); return;
    }
    let body = "";
    request.setEncoding("utf8");
    request.on("data", (chunk) => { body += chunk; if (body.length > 4096) request.destroy(); });
    request.on("end", () => {
      void (async () => {
        const { command, args } = JSON.parse(body);
        // A standalone Chromium probe has no desktop lgsm-media protocol. Keep
        // the existing bounded direct-CDN path, with no synthetic media content.
        if (command === "register_media_cache_source") return null;
        assert.equal(command, "fetch_steam_store_about");
        assert.ok(stories.some((story) => story.appId === args.appId && story.locale === args.locale));
        const started = Date.now();
        const html = captured ? captured.get(`${args.appId}:${args.locale}`).html : await requestRuntime(pipe, command, args);
        assert.equal(typeof html, "string");
        assert.ok(html.length > 0);
        requests.push({ appId: args.appId, locale: args.locale, htmlChars: html.length, elapsedMs: Date.now() - started });
        return html;
      })().then((value) => {
        response.writeHead(200, { "Content-Type": "application/json", "Cache-Control": "no-store" });
        response.end(JSON.stringify({ ok: true, value }));
      }).catch((error) => {
        response.writeHead(502, { "Content-Type": "application/json" });
        response.end(JSON.stringify({ ok: false, error: String(error) }));
      });
    });
  };
  const report = await runBrowserFixture({ fixturePath: "library-story-live-browser.html", fixtureMiddleware,
    development: false, reactTransform: false,
    fixtureTimeoutMs: stories.length * 20000 + 10000,
    contentSecurityPolicy: Object.entries(desktopCsp).map(([key, value]) => `${key} ${value}`).join("; "),
    viewport: { width: 1280, height: 900 }, screenshotPath: process.env.LANGAME_STORY_SCREENSHOT });
  assert.equal(report.status, "passed");
  assert.equal(report.stories.length, stories.length);
  assert.equal(requests.length, stories.length);
  assert.equal(report.source_mode, sourceMode);
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.browser_exited, true);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
  console.log(`STEAM_LIVE_MEDIA ${JSON.stringify({ ...report,
    ...(captured ? { probe_file: probeFile, probe_sha256: probeSha256 } : { runtime_pid: expectedPid }),
    requests, media_transport: "real direct CDN, standalone browser without desktop cache protocol" })}`);
});
