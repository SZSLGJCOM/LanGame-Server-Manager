const assert = require("node:assert/strict");
const test = require("node:test");
const fs = require("node:fs/promises");
const os = require("node:os");
const path = require("node:path");
const { readBrowserEndpoint, pollBrowserEndpoint } = require("./helpers/runtime-browser-endpoint.cjs");

const guid = "61b979ad-a1e1-45ab-8bd3-8d6679927a8f";
const browserPath = `/devtools/browser/${guid}`;

async function profile(t) {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), "langame-endpoint-test-"));
  t.after(() => fs.rm(root, { recursive: true, force: true }));
  return { root, portFile: path.join(root, "DevToolsActivePort") };
}

test("the owned profile publishes a loopback endpoint from both supported line endings", async (t) => {
  const { root, portFile } = await profile(t);
  assert.deepEqual(await readBrowserEndpoint(root), { state: "missing" });
  for (const [port, ending] of [[1, "\n"], [65535, "\r\n"]]) {
    await fs.writeFile(portFile, `${port}${ending}${browserPath}${ending}`);
    assert.deepEqual(await readBrowserEndpoint(root), {
      state: "ready", endpoint: `ws://127.0.0.1:${port}${browserPath}`,
    });
  }
});

test("invalid port files never provide an endpoint or an external host", async (t) => {
  const { root, portFile } = await profile(t);
  for (const contents of [
    `0\n${browserPath}`, `65536\n${browserPath}`, `12x\n${browserPath}`,
    `1234\nws://example.com${browserPath}`, `1234\n/devtools/page/${guid}`,
    `1234\n${browserPath}?other=1`, `1234\n${browserPath}\nextra`,
    `1234\n/devtools/browser/../${guid}`, "x".repeat(1025),
  ]) {
    await fs.writeFile(portFile, contents);
    const observation = await readBrowserEndpoint(root);
    assert.equal(observation.state, "invalid", contents.slice(0, 100));
    assert.equal(observation.endpoint, undefined);
  }
});

test("polling observes a real file created and completed after startup", { timeout: 5000 }, async (t) => {
  const { root, portFile } = await profile(t);
  const controller = new AbortController();
  t.after(() => controller.abort());
  const missing = Promise.withResolvers();
  const incomplete = Promise.withResolvers();
  const observed = [];
  const pending = pollBrowserEndpoint(root, {
    deadlineMs: performance.now() + 4000,
    signal: controller.signal,
    onObservation(observation) {
      observed.push(observation.state);
      if (observation.state === "missing") missing.resolve();
      if (observation.state === "incomplete") incomplete.resolve();
    },
  });
  await missing.promise;
  await fs.writeFile(portFile, `12345\n/devtools/browser/${guid.slice(0, 8)}`);
  await incomplete.promise;
  await fs.appendFile(portFile, guid.slice(8));
  assert.equal(await pending, `ws://127.0.0.1:12345${browserPath}`);
  assert.equal(observed.at(-1), "ready");
  controller.abort();
});

test("a malformed file can be replaced within the same deadline without exposing its contents", async (t) => {
  const { root, portFile } = await profile(t);
  await fs.writeFile(portFile, "invalid endpoint contents");
  const observed = Promise.withResolvers();
  const controller = new AbortController();
  t.after(() => controller.abort());
  const pending = pollBrowserEndpoint(root, {
    deadlineMs: performance.now() + 4000, signal: controller.signal,
    onObservation: (observation) => observed.resolve(observation),
  });
  assert.deepEqual(await observed.promise, { state: "invalid", reason: "port" });
  await fs.writeFile(portFile, `12345\n${browserPath}`);
  assert.equal(await pending, `ws://127.0.0.1:12345${browserPath}`);
});

test("cancellation during a pending poll stops observation and retains its reason", async (t) => {
  const { root } = await profile(t);
  const controller = new AbortController();
  const reason = new Error("stderr supplied the endpoint");
  let observations = 0;
  await assert.rejects(pollBrowserEndpoint(root, {
    deadlineMs: performance.now() + 4000, signal: controller.signal,
    onObservation() {
      observations++;
      queueMicrotask(() => controller.abort(reason));
    },
  }), (error) => error === reason);
  assert.equal(observations, 1);
});

test("an already cancelled poll cannot publish an existing file", async (t) => {
  const { root, portFile } = await profile(t);
  await fs.writeFile(portFile, `12345\n${browserPath}`);
  const reason = new Error("browser exited");
  await assert.rejects(pollBrowserEndpoint(root, {
    deadlineMs: performance.now() + 4000, signal: AbortSignal.abort(reason),
    onObservation: () => assert.fail("cancelled polling observed a file"),
  }), (error) => error === reason);
});

test("cancellation by the observer takes precedence over a ready port file", async (t) => {
  const { root, portFile } = await profile(t);
  await fs.writeFile(portFile, `12345\n${browserPath}`);
  const controller = new AbortController();
  const reason = new Error("the startup owner stopped waiting");
  await assert.rejects(pollBrowserEndpoint(root, {
    deadlineMs: performance.now() + 4000, signal: controller.signal,
    onObservation() { controller.abort(reason); },
  }), (error) => error === reason);
});

test("missing files expire at the supplied deadline with their last observed state", async (t) => {
  const { root } = await profile(t);
  let now = 0;
  t.mock.method(performance, "now", () => now);
  let lastObservation;
  await assert.rejects(pollBrowserEndpoint(root, {
    deadlineMs: 100,
    onObservation: (observation) => { lastObservation = observation; now = 100; },
  }), (error) => error.code === "ETIMEDOUT" && error.observation === lastObservation);
  assert.deepEqual(lastObservation, { state: "missing" });
  await assert.rejects(pollBrowserEndpoint(root, {
    deadlineMs: performance.now() - 1,
    onObservation: () => assert.fail("expired polling observed a file"),
  }), { code: "ETIMEDOUT" });
});

test("another profile's ready file cannot satisfy this profile's poll", async (t) => {
  const first = await profile(t);
  const second = await profile(t);
  await fs.writeFile(first.portFile, `12345\n${browserPath}`);
  let now = 0;
  t.mock.method(performance, "now", () => now);
  await assert.rejects(pollBrowserEndpoint(second.root, {
    deadlineMs: 100, onObservation: () => { now = 100; },
  }), (error) => error.code === "ETIMEDOUT" && error.observation.state === "missing");
});
