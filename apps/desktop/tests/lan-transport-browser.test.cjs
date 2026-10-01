const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

const tauri = JSON.parse(fs.readFileSync(path.join(__dirname, "../src-tauri/tauri.conf.json"), "utf8"));
const desktopPolicy = Object.entries(tauri.app.security.csp).map(([name, value]) => `${name} ${value}`).join("; ");
const lanSource = fs.readFileSync(path.join(__dirname, "../src-tauri/src/lan_host.rs"), "utf8");
const policyDeclaration = lanSource.match(/const LAN_CONTENT_SECURITY_POLICY: &str = concat!\(([\s\S]*?)\n\);/);
assert.ok(policyDeclaration, "Current LAN response policy was not found");
const lanPolicy = [...policyDeclaration[1].matchAll(/"(?:\\.|[^"\\])*"/g)].map(([value]) => JSON.parse(value)).join("");

for (const [policyName, contentSecurityPolicy] of [["desktop", desktopPolicy], ["LAN", lanPolicy]]) {
  test(`real browser enforces ${policyName} CSP and authenticated production transport`, { timeout: 90000 }, async () => {
    const requests = [];
    const importedModules = [];
    const fixtureMiddleware = (request, response, next) => {
      importedModules.push(request.url);
      if (request.url !== "/__langame/api") return next();
      let body = "";
      request.setEncoding("utf8");
      request.on("data", (chunk) => {
        body += chunk;
        if (Buffer.byteLength(body) > 4096) request.destroy(new Error("Fixture request exceeded its limit"));
      });
      request.on("error", () => { if (!response.headersSent) response.writeHead(400).end(); });
      request.on("end", () => {
        const payload = JSON.parse(body);
        requests.push({ url: request.url, headers: request.headers, payload });
        const authenticated = request.headers["x-langame-token"] === "synthetic-test-only";
        const failed = payload.command === "start_instance_process";
        response.writeHead(!authenticated ? 401 : failed ? 503 : 200, { "Content-Type": "application/json" });
        response.end(JSON.stringify(!authenticated ? { ok: false, error: "fixture authentication required" }
          : failed ? { ok: false, error: "fixture backend unavailable" }
            : { ok: true, value: { source: "isolated-http-fixture" } }));
      });
    };
    const report = await runBrowserFixture({ fixturePath: "lan-transport-browser.html",
      development: false, reactTransform: false, contentSecurityPolicy, fixtureMiddleware });
    assert.equal(report.checks, 12);
    assert.equal(report.production_transport, true);
    assert.equal(report.blob_worker_replied, true);
    assert.equal(report.inline_script_blocked, true);
    assert.equal(report.browser_exited, true);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.scratch_removed, true);
    assert.deepEqual(requests.map(({ payload }) => payload.command), ["bootstrap", "read_instance_details_from_storage", "start_instance_process"]);
    assert.equal(requests[0].headers["x-langame-token"], undefined);
    for (const request of requests) {
      assert.equal(request.headers.cookie, undefined, "Management fetch must omit browser cookies");
      assert.ok(!request.url.includes("synthetic-test-only"));
      assert.ok(!JSON.stringify(request.payload).includes("synthetic-test-only"));
      assert.ok(!request.headers.referer?.includes("synthetic-test-only"));
    }
    for (const request of requests.slice(1)) assert.equal(request.headers["x-langame-token"], "synthetic-test-only");
    assert.ok(!importedModules.some((url) => url.includes("api-mock")), "Production failure loaded the preview module");
    console.log(`LAN_TRANSPORT_BROWSER ${JSON.stringify({ policy: policyName, ...report })}`);
  });
}
