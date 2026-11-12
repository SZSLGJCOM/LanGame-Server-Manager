const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

const readiness = fs.readFileSync(path.join(__dirname, "../src-tauri/src/nexus_browser_readiness.js"), "utf8");
const metadata = fs.readFileSync(path.join(__dirname, "../src-tauri/src/nexus_page_extract.js"), "utf8");
const samples = [
  ["empty SPA", "<main></main>", false],
  ["metadata without body", '<link rel="canonical" href="https://www.nexusmods.com/7daystodie/mods/2264">', false],
  ["zero-size iframe", '<iframe style="width:0;height:0;border:0" srcdoc=""></iframe>', false],
  ["zero-size input", '<input style="width:0;height:0;padding:0;border:0">', false],
  ["offscreen iframe", '<iframe style="position:fixed;left:-2000px;width:300px;height:80px" srcdoc=""></iframe>', false],
  ["iframe below viewport", '<iframe style="position:fixed;top:2000px;width:300px;height:80px" srcdoc=""></iframe>', false],
  ["hidden login", '<form hidden><input type="password"></form>', false],
  ["CSS hidden login", '<form style="display:none"><input type="password"></form>', false],
  ["invisible iframe", '<iframe style="visibility:hidden" srcdoc=""></iframe>', false],
  ["transparent iframe", '<iframe style="opacity:0" srcdoc=""></iframe>', false],
  ["visible login", '<form><input type="email"><input type="password"></form>', true],
  ["visible challenge", '<main><h1>Security verification</h1><p>Verify you are human</p></main>', true],
  ["visible challenge frame", '<iframe style="width:300px;height:70px" srcdoc="<p>Verify</p>"></iframe>', true],
  ["project ready", '<main><h1>Server at Home</h1></main>', true],
  ["file selection content", '<main><button>Download files</button></main>', true],
];

function html() {
  const literal = (value) => JSON.stringify(value).replaceAll("<", "\\u003c");
  return `<!doctype html><body><script>
  (async () => {
    const nonce = new URL(location.href).searchParams.get("nonce");
    const results = [];
    const errors = [];
    addEventListener("error", event => errors.push(event.message));
    try {
      const script = ${literal(readiness)};
      for (const [name, body] of ${literal(samples)}) {
        const frame = document.createElement("iframe");
        frame.style.cssText = "width:640px;height:480px;position:fixed;left:0;top:0";
        const loaded = new Promise(resolve => { frame.onload = resolve; });
        frame.srcdoc = "<!doctype html><body>" + body;
        document.body.appendChild(frame);
        await loaded;
        const result = frame.contentWindow.eval(script);
        results.push({ name, hasContent: result.hasContent });
        frame.remove();
      }
      const frame = document.createElement("iframe");
      const loaded = new Promise(resolve => { frame.onload = resolve; });
      frame.srcdoc = '<!doctype html><body><h1>Still parsing</h1><script>window.duringParse = ' + script + '<\\/script>';
      document.body.appendChild(frame);
      await loaded;
      results.push({ name: "parsing document", hasContent: frame.contentWindow.duringParse.hasContent });
      frame.remove();
      // PLAP's failed 13762 page had an interactive verification shell with two
      // body children but no file list. Its identity is observable before any
      // generic visible-content readiness; native must wait as verification.
      const shell = document.createElement("iframe");
      const shellLoaded = new Promise(resolve => { shell.onload = resolve; });
      shell.srcdoc = '<!doctype html><head><title>Just a moment...</title></head><body><div id="challenge-running"></div><div></div>';
      document.body.append(shell); await shellLoaded;
      const shellReady = shell.contentWindow.eval(script);
      const shellPage = new shell.contentWindow.Function("location", "return " + ${literal(metadata)})(new URL("https://www.nexusmods.com/7daystodie/mods/13762"));
      const challengeShell = { hasContent: shellReady.hasContent, state: shellPage.state, url: shellPage.url };
      shell.remove();
      await fetch("/__reliability_result/" + nonce, { method:"POST", headers:{"Content-Type":"application/json"},
        body:JSON.stringify({status:"passed", results, challengeShell, browser_errors:errors}) });
    } catch (error) {
      await fetch("/__reliability_result/" + nonce, { method:"POST", headers:{"Content-Type":"application/json"},
        body:JSON.stringify({status:"failed", error:String(error)}) });
    }
  })();
  </script>`;
}

test("Nexus detects rendered website content before background automation", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ reactTransform: false, fixtureMiddleware(request, response, next) {
    if (request.url?.split("?")[0] !== "/tests/helpers/runtime-browser.html") return next();
    response.writeHead(200, { "Content-Type": "text/html; charset=utf-8" }).end(html());
  } });
  assert.equal(report.status, "passed", report.error);
  assert.deepEqual(report.results, [...samples.map(([name, , hasContent]) => ({ name, hasContent })),
    { name: "parsing document", hasContent: false }]);
  assert.deepEqual(report.challengeShell, { hasContent: false, state: "challenge", url: "https://www.nexusmods.com/7daystodie/mods/13762" });
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.browser_exited, true);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
});
