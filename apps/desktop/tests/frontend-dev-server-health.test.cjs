const assert = require("node:assert/strict");
const { spawn, spawnSync } = require("node:child_process");
const fs = require("node:fs");
const http = require("node:http");
const os = require("node:os");
const path = require("node:path");
const test = require("node:test");
const { pathToFileURL } = require("node:url");

const root = path.resolve(__dirname, "..", "..", "..");
const healthScriptPath = path.join(root, "scripts", "check_frontend_dev_server.mjs");
const stopScriptPath = path.join(root, "scripts", "stop_frontend_dev_server.ps1");

async function loadHealthModule() {
  return import(`${pathToFileURL(healthScriptPath).href}?test=${Date.now()}`);
}

async function withFixtureServer(responder, callback) {
  const server = http.createServer(responder);
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const address = server.address();
  try {
    return await callback(`http://127.0.0.1:${address.port}`);
  } finally {
    await new Promise((resolve, reject) => {
      server.close((error) => error ? reject(error) : resolve());
    });
  }
}

function send(response, status, type, body) {
  response.writeHead(status, { "content-type": type });
  response.end(body);
}

async function waitForHttp(url, timeoutMs = 3000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(url);
      if (response.ok) { return; }
    } catch {}
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
  throw new Error(`Fixture server did not listen at ${url}`);
}

test("Vite health check follows lazy modules and retries an outdated optimized dependency", async () => {
  const { waitForViteModuleGraph } = await loadHealthModule();
  let optimizedDependencyRequests = 0;

  await withFixtureServer((request, response) => {
    const url = new URL(request.url, "http://fixture.invalid");
    if (url.pathname === "/") {
      return send(
        response,
        200,
        "text/html",
        '<script type="module" src="/@vite/client"></script><script type="module" src="/src/main.tsx"></script>',
      );
    }
    if (url.pathname === "/@vite/client") {
      return send(response, 200, "text/javascript", "export const createHotContext = () => {}; ");
    }
    if (url.pathname === "/src/main.tsx") {
      return send(response, 200, "text/javascript", 'import App from "/src/App.tsx";');
    }
    if (url.pathname === "/src/App.tsx") {
      return send(response, 200, "text/javascript", 'const View = import("/src/views/ServerWorkspaceView.tsx");');
    }
    if (url.pathname === "/src/views/ServerWorkspaceView.tsx") {
      return send(
        response,
        200,
        "text/javascript",
        'import "/node_modules/.vite/deps/@tauri-apps_api_event.js?v=stale";',
      );
    }
    if (url.pathname === "/node_modules/.vite/deps/@tauri-apps_api_event.js") {
      optimizedDependencyRequests += 1;
      if (optimizedDependencyRequests === 1) {
        return send(response, 504, "text/plain", "Outdated Optimize Dep");
      }
      return send(response, 200, "text/javascript", "export const listen = () => {}; ");
    }
    return send(response, 404, "text/plain", "missing fixture module");
  }, async (baseUrl) => {
    const result = await waitForViteModuleGraph({
      baseUrl,
      attempts: 2,
      retryDelayMs: 10,
    });
    assert.equal(result.attempt, 2);
    assert.ok(result.moduleCount >= 5);
    assert.equal(optimizedDependencyRequests, 2);
  });
});

test("Vite health check reports the exact persistent nested module failure", async () => {
  const { waitForViteModuleGraph } = await loadHealthModule();

  await withFixtureServer((request, response) => {
    const url = new URL(request.url, "http://fixture.invalid");
    if (url.pathname === "/") {
      return send(
        response,
        200,
        "text/html",
        '<script type="module" src="/@vite/client"></script><script type="module" src="/src/main.tsx"></script>',
      );
    }
    if (url.pathname === "/@vite/client") {
      return send(response, 200, "text/javascript", "export const createHotContext = () => {}; ");
    }
    if (url.pathname === "/src/main.tsx") {
      return send(response, 200, "text/javascript", 'import "/node_modules/.vite/deps/broken.js?v=1";');
    }
    if (url.pathname === "/src/views/ServerWorkspaceView.tsx") {
      return send(response, 200, "text/javascript", "export default function View() {}");
    }
    return send(response, 504, "text/plain", "Outdated Optimize Dep");
  }, async (baseUrl) => {
    await assert.rejects(
      waitForViteModuleGraph({ baseUrl, attempts: 2, retryDelayMs: 10 }),
      (error) => {
        assert.match(error.message, /HTTP 504/);
        assert.match(error.message, /node_modules\/\.vite\/deps\/broken\.js/);
        assert.match(error.message, /Outdated Optimize Dep/);
        return true;
      },
    );
  });
});

test("Vite health check rejects a missing module rewritten to the SPA HTML fallback", async () => {
  const { waitForViteModuleGraph } = await loadHealthModule();

  await withFixtureServer((request, response) => {
    const url = new URL(request.url, "http://fixture.invalid");
    const indexHtml =
      '<script type="module" src="/@vite/client"></script>' +
      '<script type="module" src="/src/main.tsx"></script>';
    if (url.pathname === "/") {
      return send(response, 200, "text/html", indexHtml);
    }
    if (url.pathname === "/@vite/client") {
      return send(response, 200, "text/javascript", "export const createHotContext = () => {}; ");
    }
    if (url.pathname === "/src/main.tsx") {
      return send(response, 200, "text/javascript", 'import "/src/missing.tsx";');
    }
    if (url.pathname === "/src/views/ServerWorkspaceView.tsx") {
      return send(response, 200, "text/javascript", "export default function View() {}");
    }
    return send(response, 200, "text/html", indexHtml);
  }, async (baseUrl) => {
    await assert.rejects(
      waitForViteModuleGraph({ baseUrl, attempts: 1 }),
      /Expected a JavaScript module.*src\/missing\.tsx.*text\/html/,
    );
  });
});

test("Vite health check rejects an unresolved runtime placeholder in the dev client", async () => {
  const { waitForViteModuleGraph } = await loadHealthModule();

  await withFixtureServer((request, response) => {
    const url = new URL(request.url, "http://fixture.invalid");
    if (url.pathname === "/") {
      return send(
        response,
        200,
        "text/html",
        '<script type="module" src="/@vite/client"></script><script type="module" src="/src/main.tsx"></script>',
      );
    }
    if (url.pathname === "/@vite/client") {
      return send(
        response,
        200,
        "text/javascript",
        "const forwardConsole = __SERVER_FORWARD_CONSOLE__; export { forwardConsole };",
      );
    }
    if (url.pathname === "/src/main.tsx") {
      return send(response, 200, "text/javascript", "export default function main() {}");
    }
    if (url.pathname === "/src/views/ServerWorkspaceView.tsx") {
      return send(response, 200, "text/javascript", "export default function View() {}");
    }
    return send(response, 404, "text/plain", "missing fixture module");
  }, async (baseUrl) => {
    await assert.rejects(
      waitForViteModuleGraph({ baseUrl, attempts: 1 }),
      /Unresolved Vite client runtime placeholder __SERVER_FORWARD_CONSOLE__.*does not match the installed Vite client/,
    );
  });
});

test("frontend restart refuses to terminate an unrelated listener", async () => {
  const frontendDir = path.join(root, "apps", "desktop");
  await withFixtureServer(
    (_request, response) => send(response, 200, "text/plain", "unrelated service"),
    async (baseUrl) => {
      const result = spawnSync("powershell", [
        "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", stopScriptPath,
        "-FrontendDir", frontendDir, "-FrontendUrl", baseUrl,
      ], { encoding: "utf8", timeout: 15000 });
      assert.equal(result.status, 2,
        `${result.stdout}\n${result.stderr}\nProcess error: ${result.error?.message ?? "none"}\nSignal: ${result.signal ?? "none"}`);
      assert.match(`${result.stdout}\n${result.stderr}`, /refusing to stop/i);
      assert.equal((await fetch(baseUrl)).status, 200);
    },
  );
});

test("frontend restart terminates only the matching Vite listener", async () => {
  const tempRoot = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), "langame-vite-stop-")));
  const viteScript = path.join(tempRoot, "node_modules", "vite", "bin", "vite.js");
  fs.mkdirSync(path.dirname(viteScript), { recursive: true });
  fs.writeFileSync(viteScript, [
    "const http = require('node:http');",
    "const port = Number(process.argv[process.argv.indexOf('--port') + 1]);",
    "http.createServer((_request, response) => response.end('fixture vite')).listen(port, '127.0.0.1');",
  ].join("\n"));

  const portProbe = http.createServer();
  await new Promise((resolve, reject) => {
    portProbe.once("error", reject);
    portProbe.listen(0, "127.0.0.1", resolve);
  });
  const port = portProbe.address().port;
  await new Promise((resolve) => portProbe.close(resolve));
  const baseUrl = `http://127.0.0.1:${port}`;
  const viteProcess = spawn(
    process.execPath,
    [viteScript, "--host", "127.0.0.1", "--port", String(port), "--strictPort"],
    { stdio: "ignore" },
  );
  const viteExit = new Promise((resolve) => viteProcess.once("exit", resolve));

  try {
    await waitForHttp(baseUrl);
    const result = spawnSync("powershell", [
      "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", stopScriptPath,
      "-FrontendDir", tempRoot, "-FrontendUrl", baseUrl,
    ], { encoding: "utf8", timeout: 10000 });
    assert.equal(result.status, 0,
      `${result.stdout}\n${result.stderr}\nProcess error: ${result.error?.message ?? "none"}\nSignal: ${result.signal ?? "none"}`);
    await Promise.race([
      viteExit,
      new Promise((_, reject) => setTimeout(() => reject(new Error("Vite fixture was not stopped")), 3000)),
    ]);
  } finally {
    if (viteProcess.exitCode === null) { viteProcess.kill("SIGKILL"); }
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
});
