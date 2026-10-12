const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const { setTimeout: delay } = require("node:timers/promises");

const portFileLimit = 1024;
const browserPathPrefix = "/devtools/browser/";
const browserGuidShape = "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx";

function parsePortFile(text) {
  const lines = text.split(/\r?\n/);
  if (lines.length === 1 && /^\d{0,5}\r?$/.test(text)) return { state: "incomplete" };
  if (!/^[1-9]\d{0,4}$/.test(lines[0])) return { state: "invalid", reason: "port" };
  const port = Number(lines[0]);
  if (port > 65535) return { state: "invalid", reason: "port" };
  if (lines.length > 3 || (lines.length === 3 && lines[2] !== "")) {
    return { state: "invalid", reason: "extra-lines" };
  }
  const browserPath = lines[1] ?? "";
  if (browserPathPrefix.startsWith(browserPath)) return { state: "incomplete" };
  if (!browserPath.startsWith(browserPathPrefix)) return { state: "invalid", reason: "browser-path" };
  const guid = browserPath.slice(browserPathPrefix.length);
  if (guid.length > browserGuidShape.length || [...guid].some((character, index) =>
    browserGuidShape[index] === "-" ? character !== "-" : !/[a-f0-9]/i.test(character))) {
    return { state: "invalid", reason: "browser-path" };
  }
  if (guid.length < browserGuidShape.length) return { state: "incomplete" };
  return { state: "ready", endpoint: `ws://127.0.0.1:${port}${browserPath}` };
}

async function readBrowserEndpoint(profileDirectory) {
  let handle;
  try {
    handle = await fs.open(path.join(profileDirectory, "DevToolsActivePort"), "r");
    if (!(await handle.stat()).isFile()) return { state: "invalid", reason: "not-file" };
    const bytes = Buffer.alloc(portFileLimit + 1);
    const { bytesRead } = await handle.read(bytes, 0, bytes.length, 0);
    if (bytesRead > portFileLimit) return { state: "invalid", reason: "oversized" };
    return parsePortFile(bytes.toString("utf8", 0, bytesRead));
  } catch (error) {
    if (error.code === "ENOENT") return { state: "missing" };
    return { state: "read-error", code: error.code ?? "UNKNOWN" };
  } finally {
    await handle?.close();
  }
}

async function pollBrowserEndpoint(profileDirectory, { deadlineMs, signal, onObservation } = {}) {
  assert.ok(Number.isFinite(deadlineMs), "Browser endpoint polling requires an absolute startup deadline");
  let observation;
  function remainingTime() {
    signal?.throwIfAborted();
    const remaining = deadlineMs - performance.now();
    if (remaining <= 0) {
      throw Object.assign(new Error("Browser DevTools port file exceeded the startup deadline"),
        { code: "ETIMEDOUT", observation });
    }
    return remaining;
  }
  for (;;) {
    remainingTime();
    observation = await readBrowserEndpoint(profileDirectory);
    remainingTime();
    onObservation?.(observation);
    remainingTime();
    if (observation.state === "ready") return observation.endpoint;
    // Chromium writes this small file after listening. A concurrent read may
    // see missing or partial bytes; every observation shares the caller's budget.
    try {
      await delay(Math.min(50, remainingTime()), undefined, { signal });
    } catch (error) {
      signal?.throwIfAborted();
      throw error;
    }
  }
}

module.exports = { readBrowserEndpoint, pollBrowserEndpoint };
