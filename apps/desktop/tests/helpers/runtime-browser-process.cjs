const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const os = require("node:os");
const path = require("node:path");
const { spawn, execFile, execFileSync } = require("node:child_process");
const { promisify } = require("node:util");
const { randomUUID } = require("node:crypto");

const execFileAsync = promisify(execFile);
const desktopRoot = path.resolve(__dirname, "../..");
const outputLimit = 64 * 1024;

async function deadline(promise, milliseconds, label) {
  let timer;
  try {
    return await Promise.race([
      promise,
      new Promise((_, reject) => {
        timer = setTimeout(() => reject(new Error(`${label} exceeded ${milliseconds} ms`)), milliseconds);
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}

async function browserExecutable() {
  const explicit = process.env.LANGAME_RELIABILITY_BROWSER;
  const candidates = explicit ? [explicit] : [
    process.env.ProgramFiles && path.join(process.env.ProgramFiles, "Google/Chrome/Application/chrome.exe"),
    process.env["ProgramFiles(x86)"] && path.join(process.env["ProgramFiles(x86)"], "Microsoft/Edge/Application/msedge.exe"),
    process.env.LOCALAPPDATA && path.join(process.env.LOCALAPPDATA, "Google/Chrome/Application/chrome.exe"),
  ];
  for (const candidate of candidates.filter(Boolean)) {
    try {
      if ((await fs.stat(candidate)).isFile()) return path.resolve(candidate);
    } catch (error) {
      if (error.code !== "ENOENT") throw error;
    }
  }
  throw new Error("Chrome/Edge is required; set LANGAME_RELIABILITY_BROWSER to an installed executable");
}

async function browserCommand(endpoint, method, params = {}) {
  const socket = new WebSocket(endpoint);
  try {
    return await deadline(new Promise((resolve, reject) => {
      socket.addEventListener("error", () => reject(new Error(`${method} connection failed`)));
      socket.addEventListener("open", () => socket.send(JSON.stringify({ id: 1, method, params })), { once: true });
      socket.addEventListener("message", ({ data }) => {
        const message = JSON.parse(data);
        if (message.id !== 1) return;
        if (message.error) reject(new Error(JSON.stringify(message.error)));
        else resolve(message.result);
      });
    }), 3000, method);
  } finally {
    socket.close();
  }
}

async function openViewportSession(endpoint) {
  const socket = new WebSocket(endpoint);
  let sequence = 0;
  let pending = false;
  let removeListeners = () => {};
  try {
    await deadline(new Promise((resolve, reject) => {
      const opened = () => resolve();
      const failed = () => reject(new Error("Viewport DevTools connection failed"));
      socket.addEventListener("open", opened);
      socket.addEventListener("error", failed);
      socket.addEventListener("close", failed);
      removeListeners = () => {
        socket.removeEventListener("open", opened);
        socket.removeEventListener("error", failed);
        socket.removeEventListener("close", failed);
      };
    }), 3000, "Viewport DevTools connection");
  } catch (error) {
    socket.close();
    throw error;
  } finally {
    removeListeners();
  }
  return {
    async command(method, params = {}) {
      assert.equal(pending, false, "Viewport DevTools commands must remain sequential");
      assert.equal(socket.readyState, WebSocket.OPEN, "Viewport DevTools connection is closed");
      pending = true;
      const id = ++sequence;
      try {
        return await deadline(new Promise((resolve, reject) => {
          const failed = () => reject(new Error(`${method} connection closed`));
          const received = ({ data }) => {
            try {
              const message = JSON.parse(data);
              if (message.id !== id) return;
              if (message.error) reject(new Error(JSON.stringify(message.error)));
              else resolve(message.result);
            } catch (error) { reject(error); }
          };
          socket.addEventListener("message", received);
          socket.addEventListener("error", failed);
          socket.addEventListener("close", failed);
          removeListeners = () => {
            socket.removeEventListener("message", received);
            socket.removeEventListener("error", failed);
            socket.removeEventListener("close", failed);
          };
          socket.send(JSON.stringify({ id, method, params }));
        }), 3000, method);
      } catch (error) {
        socket.close();
        throw error;
      } finally {
        removeListeners();
        pending = false;
      }
    },
    close() { socket.close(); },
  };
}

async function verifyViewport(session, expected, waitForResize = false) {
  const evaluated = await session.command("Runtime.evaluate", {
    expression: waitForResize ? `new Promise(resolve => {
      const measure = () => {
        if (innerWidth !== ${expected.width} || innerHeight !== ${expected.height}) return;
        removeEventListener("resize", measure);
        resolve({ width: innerWidth, height: innerHeight });
      };
      addEventListener("resize", measure);
      measure();
    })` : "({ width: innerWidth, height: innerHeight })",
    awaitPromise: waitForResize, returnByValue: true,
  });
  assert.equal(evaluated.exceptionDetails, undefined, "CSS viewport inspection failed");
  const actual = evaluated.result?.value;
  assert.deepEqual(actual, expected, "Browser CSS viewport must match the requested dimensions");
  return actual;
}

async function browserProcessSnapshot(endpoint, pid) {
  const processes = await browserCommand(endpoint, "SystemInfo.getProcessInfo");
  assert.ok(Array.isArray(processes?.processInfo), "DevTools did not return a process snapshot");
  const pids = processes.processInfo.map((entry) => entry?.id);
  assert.ok(pids.includes(pid), "DevTools must identify the browser process owned by this test");
  assert.ok(pids.every((id) => Number.isSafeInteger(id) && id > 0), "DevTools returned an invalid process identity");
  return [...new Set(pids)];
}

function alive(pid) {
  try { process.kill(pid, 0); return true; } catch (error) {
    if (error.code === "ESRCH") return false;
    throw error;
  }
}

async function waitForBrowserProcesses(pids) {
  const expires = Date.now() + 3000;
  while (pids.some(alive)) {
    assert.ok(Date.now() < expires, "Owned browser descendants did not exit within 3000 ms");
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
}

async function closeBrowser(child, exited, endpoint) {
  if (child.exitCode !== null || child.signalCode !== null || !child.pid) return;
  if (endpoint) {
    const socket = new WebSocket(endpoint);
    try {
      await deadline(new Promise((resolve, reject) => {
        socket.addEventListener("error", () => reject(new Error("Browser.close connection failed")));
        socket.addEventListener("open", () => {
          socket.send(JSON.stringify({ id: 1, method: "Browser.close" }));
          resolve();
        }, { once: true });
      }), 2000, "Browser.close connection");
      await deadline(exited, 3000, "Browser.close");
    } catch {
      // The fallback below terminates only this still-running process and its tree.
    } finally {
      socket.close();
    }
  }
  if (child.exitCode === null && child.signalCode === null) {
    if (process.platform === "win32") {
      await execFileAsync("taskkill.exe", ["/PID", String(child.pid), "/T", "/F"], {
        windowsHide: true, timeout: 5000, maxBuffer: outputLimit,
      });
    } else {
      process.kill(-child.pid, "SIGKILL");
    }
  }
  await deadline(exited, 5000, "Owned browser process exit");
}

async function removeScratch(scratch) {
  const parent = path.resolve(os.tmpdir());
  const target = path.resolve(scratch);
  assert.equal(path.dirname(target), parent, "Scratch must be a direct child of os.tmpdir()");
  assert.ok(path.basename(target).startsWith("langame-reliability-browser-"));
  assert.equal((await fs.lstat(target)).isSymbolicLink(), false, "Scratch root must not be a link");
  await fs.rm(target, { recursive: true, force: false, maxRetries: 5, retryDelay: 100 });
  await assert.rejects(fs.stat(target), { code: "ENOENT" });
}

async function runBrowserFixture({ fixturePath = "runtime-browser.html", keyboard = false, pointer = false, screenshotPath, viewport,
  deviceScaleFactor = 1, fontSelector, development = true, reactTransform = true, contentSecurityPolicy, fixtureMiddleware,
  fixtureCleanup = false, fixtureTimeoutMs = 45000 } = {}) {
  assert.match(fixturePath, /^[a-z-]+\.html$/, "Fixture must be an HTML entry in tests/helpers");
  assert.ok(Number.isSafeInteger(fixtureTimeoutMs) && fixtureTimeoutMs > 0 && fixtureTimeoutMs <= 240000,
    "Fixture timeout must be a positive integer at most 240 seconds");
  assert.ok(Number.isFinite(deviceScaleFactor) && deviceScaleFactor >= 1 && deviceScaleFactor <= 3,
    "Device scale factor must be between 1 and 3");
  assert.ok(viewport || deviceScaleFactor === 1, "DPI emulation requires an explicit CSS viewport");
  if (fontSelector !== undefined) {
    assert.ok(viewport && typeof fontSelector === "string" && fontSelector.length > 0 && fontSelector.length <= 200,
      "Font inspection requires a viewport and a bounded DOM selector");
  }
  if (viewport) {
    for (const dimension of [viewport.width, viewport.height]) {
      assert.ok(Number.isInteger(dimension) && dimension >= 320 && dimension <= 4096, "Viewport dimensions must be bounded CSS pixels");
    }
  }
  const windowSize = viewport ?? (screenshotPath ? { width: 1280, height: 900 } : null);
  const executable = await browserExecutable();
  const scratch = await fs.mkdtemp(path.join(os.tmpdir(), "langame-reliability-browser-"));
  const nonce = randomUUID();
  let server;
  let child;
  let exited;
  let endpoint;
  let viewportSession;
  let stderr = "";
  let report;
  let failure;
  let processIds = [];
  let processSnapshotKnown = false;
  let retainedScratch = null;
  const cleanupErrors = [];
  async function pageEndpoint(waitForTarget = false) {
    assert.ok(endpoint, "Browser DevTools endpoint is not ready");
    const browserUrl = new URL(endpoint);
    const expires = Date.now() + 5000;
    do {
      const response = await fetch(`http://${browserUrl.host}/json/list`, { signal: AbortSignal.timeout(3000) });
      assert.equal(response.ok, true);
      const target = (await response.json()).find((page) => page.type === "page" && page.url.includes(nonce));
      if (target?.webSocketDebuggerUrl) return target.webSocketDebuggerUrl;
      assert.ok(waitForTarget && Date.now() < expires, "Owned fixture page was not found");
      await new Promise((resolve) => setTimeout(resolve, 25));
    } while (true);
  }
  // This is a hard bound even if a library retains an open handle after an error.
  // Normal failures run the asynchronous cleanup below before this last resort.
  const commandTimeoutMs = fixtureTimeoutMs + 35000;
  const watchdog = setTimeout(() => {
    console.error(`Browser reliability exceeded its ${commandTimeoutMs / 1000}-second command deadline`);
    if (child?.pid && child.exitCode === null && child.signalCode === null) {
      try {
        if (process.platform === "win32") {
          execFileSync("taskkill.exe", ["/PID", String(child.pid), "/T", "/F"], {
            windowsHide: true, timeout: 5000, stdio: "ignore",
          });
        } else process.kill(-child.pid, "SIGKILL");
      } catch (error) { console.error(`Owned browser termination failed: ${error}`); }
    }
    process.exit(1);
  }, commandTimeoutMs);
  watchdog.unref();
  try {
    const [{ createServer }, { default: react }] = await Promise.all([import("vite"), import("@vitejs/plugin-react-swc")]);
    let receiveReport;
    let rejectReport;
    let receiveViewportReady;
    const viewportReady = new Promise((resolve) => { receiveViewportReady = resolve; });
    const result = new Promise((resolve, reject) => { receiveReport = resolve; rejectReport = reject; });
    // Observe early middleware failures while the server is still starting.
    result.catch(() => {});
    server = await createServer({
      configFile: false,
      root: desktopRoot,
      cacheDir: path.join(scratch, "vite-cache"),
      appType: "mpa",
      logLevel: "error",
      define: { "import.meta.env.DEV": JSON.stringify(development) },
      resolve: { alias: { "@tauri-apps/api/event": path.join(__dirname, "runtime-browser-events.ts") } },
      server: { host: "127.0.0.1", port: 0, strictPort: true, hmr: false,
        ...(contentSecurityPolicy ? { headers: { "Content-Security-Policy": contentSecurityPolicy } } : {}) },
      plugins: [...(reactTransform ? [react()] : []), {
        name: "runtime-reliability-result",
        configureServer(vite) {
          if (fixtureMiddleware) vite.middlewares.use(fixtureMiddleware);
          vite.middlewares.use((request, response, next) => {
            if (request.url === `/__reliability_viewport/${nonce}`) {
              response.writeHead(200, { "Content-Type": "text/html; charset=utf-8" });
              response.end(`<!doctype html><title>Preparing test viewport</title>
                <script>fetch("/__reliability_viewport_ready/${nonce}", { method: "POST" });</script>`);
              return;
            }
            if (request.url === `/__reliability_viewport_ready/${nonce}`) {
              if (request.method !== "POST") { response.writeHead(405).end(); return; }
              receiveViewportReady();
              response.writeHead(204).end();
              return;
            }
            if (keyboard && request.url?.startsWith(`/__reliability_key/${nonce}/`)) {
              if (request.method !== "POST") { response.writeHead(405).end(); return; }
              const key = request.url.split("/").at(-1);
              const codes = { Tab: 9, Escape: 27, Enter: 13, End: 35 };
              if (!Object.hasOwn(codes, key)) { response.writeHead(400).end(); return; }
              void (async () => {
                const page = await pageEndpoint();
                const params = { key, code: key, windowsVirtualKeyCode: codes[key] };
                await browserCommand(page, "Input.dispatchKeyEvent", { ...params, type: "keyDown",
                  ...(key === "Enter" ? { text: "\r", unmodifiedText: "\r" } : {}) });
                await browserCommand(page, "Input.dispatchKeyEvent", { ...params, type: "keyUp" });
                response.writeHead(204).end();
              })().catch((error) => { rejectReport(error); response.writeHead(500).end(); });
              return;
            }
            if (pointer && request.url === `/__reliability_pointer/${nonce}`) {
              if (request.method !== "POST") { response.writeHead(405).end(); return; }
              let body = "";
              let oversized = false;
              request.setEncoding("utf8");
              request.on("error", rejectReport);
              request.on("data", (chunk) => {
                body += chunk;
                if (Buffer.byteLength(body) > 1024) {
                  oversized = true;
                  response.writeHead(413).end();
                  request.destroy();
                }
              });
              request.on("end", () => {
                if (oversized) return;
                let event;
                try { event = JSON.parse(body); } catch { response.writeHead(400).end(); return; }
                if (!event || typeof event !== "object" || Array.isArray(event)
                  || Object.keys(event).some((key) => !["type", "x", "y"].includes(key))
                  || !["move", "click"].includes(event.type)
                  || ![event.x, event.y].every((value) => Number.isFinite(value) && value >= 0 && value <= 4096)) {
                  response.writeHead(400).end(); return;
                }
                void (async () => {
                  const page = await pageEndpoint();
                  const params = { x: event.x, y: event.y, pointerType: "mouse" };
                  await browserCommand(page, "Input.dispatchMouseEvent", { ...params, type: "mouseMoved", button: "none" });
                  if (event.type === "click") {
                    await browserCommand(page, "Input.dispatchMouseEvent", {
                      ...params, type: "mousePressed", button: "left", buttons: 1, clickCount: 1,
                    });
                    await browserCommand(page, "Input.dispatchMouseEvent", {
                      ...params, type: "mouseReleased", button: "left", buttons: 0, clickCount: 1,
                    });
                  }
                  response.writeHead(204).end();
                })().catch((error) => { rejectReport(error); response.writeHead(500).end(); });
              });
              return;
            }
            if (request.url !== `/__reliability_result/${nonce}`) return next();
            if (request.method !== "POST") { response.writeHead(405).end(); return; }
            let body = "";
            let oversized = false;
            request.setEncoding("utf8");
            request.on("error", rejectReport);
            request.on("data", (chunk) => {
              body += chunk;
              if (Buffer.byteLength(body) > outputLimit) {
                oversized = true;
                rejectReport(new Error("Browser report exceeded its byte budget"));
                request.destroy();
              }
            });
            request.on("end", () => {
              if (oversized) return;
              try {
                receiveReport(JSON.parse(body));
                response.writeHead(204).end();
              } catch (error) {
                rejectReport(error);
                response.writeHead(400).end();
              }
            });
          });
        },
      }],
    });
    await deadline(server.listen(), 15000, "Fixture server startup");
    const address = server.httpServer.address();
    assert.ok(address && typeof address !== "string");
    const url = `http://127.0.0.1:${address.port}/tests/helpers/${fixturePath}?nonce=${nonce}`;
    let receiveEndpoint;
    const endpointReady = new Promise((resolve) => { receiveEndpoint = resolve; });
    child = spawn(executable, [
      // Keep the observed Edge compatibility relaunch from detaching the owned
      // browser PID and its DevTools stderr pipe from this fixture.
      ...(path.basename(executable).toLowerCase() === "msedge.exe" ? ["--edge-skip-compat-layer-relaunch"] : []),
      "--headless", "--remote-debugging-port=0", "--remote-debugging-address=127.0.0.1",
      ...(windowSize ? [`--window-size=${windowSize.width},${windowSize.height}`, "--force-device-scale-factor=1"] : []),
      `--user-data-dir=${path.join(scratch, "profile")}`, "--no-first-run", "--no-default-browser-check",
      "--disable-background-networking", viewport ? `http://127.0.0.1:${address.port}/__reliability_viewport/${nonce}` : url,
    ], { windowsHide: true, detached: process.platform !== "win32", stdio: ["ignore", "ignore", "pipe"] });
    exited = new Promise((resolve) => {
      child.once("error", (error) => resolve({ error }));
      child.once("exit", (code, signal) => resolve({ code, signal }));
    });
    child.stderr.on("data", (data) => {
      stderr = (stderr + data.toString()).slice(-outputLimit);
      endpoint ??= stderr.match(/DevTools listening on (ws:\/\/127\.0\.0\.1:\d+\/devtools\/browser\/[\w-]+)/)?.[1];
      if (endpoint) receiveEndpoint(endpoint);
    });
    if (viewport) {
      await deadline(Promise.race([
        Promise.all([endpointReady, viewportReady]),
        exited.then((exit) => { throw new Error(`Browser exited before viewport setup: ${JSON.stringify(exit)}\n${stderr}`); }),
      ]), 5000, "Browser DevTools startup");
      viewportSession = await openViewportSession(await pageEndpoint(true));
      // Window size includes browser chrome. Keep this session connected because
      // Chromium clears device metrics when its owning DevTools client detaches.
      await viewportSession.command("Emulation.setDeviceMetricsOverride", {
        width: viewport.width, height: viewport.height, deviceScaleFactor, mobile: false,
      });
      await verifyViewport(viewportSession, viewport, true);
      const navigation = await viewportSession.command("Page.navigate", { url });
      assert.equal(navigation.errorText, undefined, "Fixture navigation failed");
    }
    report = await deadline(Promise.race([
      result,
      exited.then((exit) => { throw new Error(`Browser exited before reporting: ${JSON.stringify(exit)}\n${stderr}`); }),
    ]), fixtureTimeoutMs, "Real ReactDOM reliability fixture");
    assert.equal(report.status, "passed", report.error || JSON.stringify(report));
    assert.ok(endpoint, "Browser did not publish its isolated DevTools endpoint");
    processIds = await browserProcessSnapshot(endpoint, child.pid);
    processSnapshotKnown = true;
    const version = await browserCommand(endpoint, "Browser.getVersion");
    report.browser_product = version.product;
    report.browser_executable = path.basename(executable);
    if (viewportSession) {
      report.viewport = await verifyViewport(viewportSession, viewport);
      const scale = await viewportSession.command("Runtime.evaluate", {
        expression: "devicePixelRatio", returnByValue: true,
      });
      assert.equal(scale.exceptionDetails, undefined, "DPI inspection failed");
      assert.equal(scale.result?.value, deviceScaleFactor, "Browser DPR must match the requested scale");
      report.device_scale_factor = scale.result.value;
      if (fontSelector) {
        await viewportSession.command("DOM.enable");
        await viewportSession.command("CSS.enable");
        const document = await viewportSession.command("DOM.getDocument");
        const node = await viewportSession.command("DOM.querySelector", { nodeId: document.root.nodeId, selector: fontSelector });
        assert.ok(node.nodeId, "Font inspection target must exist");
        const rendered = await viewportSession.command("CSS.getPlatformFontsForNode", { nodeId: node.nodeId });
        report.platform_fonts = rendered.fonts;
      }
    }
    if (screenshotPath) {
      assert.equal(path.isAbsolute(screenshotPath), true, "Screenshot output must be absolute");
      const repository = path.resolve(desktopRoot, "../..");
      const relative = path.relative(repository, screenshotPath);
      assert.ok(relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative), "Screenshot output must be outside the repository");
      const capture = await browserCommand(await pageEndpoint(), "Page.captureScreenshot", { format: "png" });
      await fs.writeFile(screenshotPath, Buffer.from(capture.data, "base64"));
    }
  } catch (error) {
    failure = error;
  } finally {
    let treeExited = false;
    // Capture the mounted fixture first, then let React finish its own cleanup
    // and report errors from the page and its preview frames before browser exit.
    if (fixtureCleanup && endpoint && child?.exitCode === null && child.signalCode === null) {
      try {
        const result = await browserCommand(await pageEndpoint(), "Runtime.evaluate", {
          expression: "globalThis.__reliabilityFixtureCleanup()", awaitPromise: true, returnByValue: true,
        });
        assert.equal(result.exceptionDetails, undefined, "Fixture React cleanup failed");
        const cleanup = result.result?.value;
        assert.ok(cleanup && Array.isArray(cleanup.browser_errors), "Fixture must report cleanup errors");
        assert.deepEqual(cleanup.browser_errors, [], "Fixture or preview emitted errors before cleanup");
        assert.equal(cleanup.native_dialogs, 0, "Fixture or preview invoked a native dialog");
        if (report) report.fixture_cleanup = cleanup;
      } catch (error) { cleanupErrors.push(error); }
    }
    // A failed/timed-out page may not have reached the normal snapshot step.
    // Try again before Browser.close removes our opportunity to observe its tree.
    if (!processSnapshotKnown && endpoint && child?.pid && child.exitCode === null && child.signalCode === null) {
      try {
        processIds = await browserProcessSnapshot(endpoint, child.pid);
        processSnapshotKnown = true;
      } catch (error) { cleanupErrors.push(error); }
    }
    if (viewportSession) viewportSession.close();
    if (child) {
      try { await closeBrowser(child, exited, endpoint); } catch (error) { cleanupErrors.push(error); }
    }
    try {
      if (!child?.pid) treeExited = true;
      else if (processSnapshotKnown) {
        await waitForBrowserProcesses(processIds);
        treeExited = child.exitCode !== null || child.signalCode !== null;
      }
    } catch (error) { cleanupErrors.push(error); }
    if (server) {
      try { await deadline(server.close(), 5000, "Fixture server shutdown"); } catch (error) { cleanupErrors.push(error); }
    }
    // Retain the profile if an owned process did not exit; never delete a live profile.
    if (treeExited) {
      try { await removeScratch(scratch); } catch (error) { cleanupErrors.push(error); }
    } else {
      retainedScratch = scratch;
      cleanupErrors.push(new Error(`Owned browser process-tree exit is unverified; retained fixture scratch: ${scratch}`));
    }
  }
  // This fixture has completed its bounded cleanup; its timer must not outlive
  // either a successful return or a failure caught by a subsequent test.
  clearTimeout(watchdog);
  if (failure || cleanupErrors.length) {
    throw Object.assign(new AggregateError([failure, ...cleanupErrors].filter(Boolean),
      `Browser reliability failed: ${[failure, ...cleanupErrors].filter(Boolean).map(String).join("; ")}\n${stderr}`),
    { retainedScratch, browserPid: child?.pid, browserExited: !child?.pid || child.exitCode !== null || child.signalCode !== null });
  }
  return { ...report, browser_processes_observed: processIds.length, browser_processes_remaining: 0, browser_exited: true, scratch_removed: true };
}

module.exports = { runBrowserFixture, removeScratch };
