const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

const auth = fs.readFileSync(path.join(__dirname, "../src-tauri/src/nexus_browser_auth.js"), "utf8");
const desktop = value => `<div class="rj-header-wrap"><div class="quick-search-holder"><quick-search is-logged-in="${value}"></quick-search></div></div>`;
const mobile = value => `<div class="rj-user-nav-right"><div class="mobile-search-container"><mobile-quick-search is-logged-in="${value}"></mobile-quick-search></div></div>`;
const official = "https://www.nexusmods.com/7daystodie/mods/13899?tab=files&file_id=51828";
const accountUrl = "https://users.nexusmods.com/account/security";
const account = (href = "/auth/sign_out") => `<style>.d-none { display:none } .d-block { display:block }</style>
  <div id="main-content-wrapper" class="content-wrapper d-flex justify-content-between flex-column">
    <div class="top-half"><div class="jumbotron jumbotron-border-bottom"><div class="links"><div class="right-link">
      <a class="d-none d-md-block btn btn-outline-primary" href="${href}">Sign out</a>
      <a class="d-block d-md-none mobile-signout btn btn-outline-primary" href="${href}">Sign out</a>
    </div></div></div></div>
  </div>`;
const samples = [
  ["observed desktop header", desktop("true"), official, "signed_in"],
  ["explicit signed out", desktop("false"), official, "signed_out"],
  ["mobile header", mobile("true"), official, "signed_in"],
  ["matching header signals", desktop("true") + mobile("true"), official, "signed_in"],
  ["conflicting header signals", desktop("true") + mobile("false"), official, "unknown"],
  ["missing signal", "<h1>Project</h1>", official, null],
  ["invalid boolean", desktop("1"), official, null],
  ["case-sensitive boolean", desktop("TRUE"), official, null],
  ["author markup excluded", `<div id="description">${desktop("true")}</div>`, official, null],
  ["author signal cannot conflict", desktop("false") + `<div class="wysiwyg">${desktop("true")}</div>`, official, "signed_out"],
  ["standalone marker ignored", '<quick-search is-logged-in="true"></quick-search>', official, null],
  ["account names are not signals", '<div class="user-profile-menu-username">private-account</div>', official, null],
  ["login site is not proof", desktop("true"), "https://users.nexusmods.com/auth/sign_in", null],
  ["untrusted origin", desktop("true"), "https://www.nexusmods.com.attacker.invalid/", null],
  ["HTTP rejected", desktop("true"), "http://www.nexusmods.com/", null],
  ["URL credentials rejected", desktop("true"), "https://user:fixture@www.nexusmods.com/", null],
  ["nonstandard port rejected", desktop("true"), "https://www.nexusmods.com:444/", null],
  ["observed users security account", account(), accountUrl, "signed_in"],
  ["same-origin absolute account sign out", account("https://users.nexusmods.com/auth/sign_out"), accountUrl, "signed_in"],
  ["visible desktop account sign out", account() + "<style>.d-none {display:block}.d-block {display:none}</style>", accountUrl, "signed_in"],
  ["account structure on wrong host", account(), "https://users.nexusmods.com.attacker.invalid/account/security", null],
  ["account structure on WWW is not proof", account(), "https://www.nexusmods.com/account/security", null],
  ["account structure on sign-in is not proof", account(), "https://users.nexusmods.com/auth/sign_in", null],
  ["unobserved account path is not proof", account(), "https://users.nexusmods.com/account/other", null],
  ["account HTTP rejected", account(), "http://users.nexusmods.com/account/security", null],
  ["account URL credentials rejected", account(), "https://user:fixture@users.nexusmods.com/account/security", null],
  ["account nonstandard port rejected", account(), "https://users.nexusmods.com:444/account/security", null],
  ["standalone sign-out link is not proof", '<a href="/auth/sign_out">Sign out</a>', accountUrl, null],
  ["partial account structure is not proof", '<div id="main-content-wrapper"><div class="right-link"><a href="/auth/sign_out">Sign out</a></div></div>', accountUrl, null],
  ["account sign-out destination must be official", account("https://outside.invalid/auth/sign_out"), accountUrl, null],
  ["account sign-out path must match", account("/auth/sign_out/other"), accountUrl, null],
  ["account sign-out query is not observed proof", account("/auth/sign_out?other=1"), accountUrl, null],
  ["account author injection excluded", `<div id="description">${account()}</div>`, accountUrl, null],
  ["account author class excluded", `<div class="wysiwyg">${account()}</div>`, accountUrl, null],
  ["account template excluded", `<template>${account()}</template>`, accountUrl, null],
  ["account hidden ancestor excluded", `<div hidden>${account()}</div>`, accountUrl, null],
  ["account aria-hidden ancestor excluded", `<div aria-hidden="true">${account()}</div>`, accountUrl, null],
  ["account display-none ancestor excluded", `<div style="display:none">${account()}</div>`, accountUrl, null],
  ["account invisible ancestor excluded", `<div style="visibility:hidden">${account()}</div>`, accountUrl, null],
  ["account transparent ancestor excluded", `<div style="opacity:0">${account()}</div>`, accountUrl, null],
  ["account empty loading document", "<main></main>", accountUrl, null],
  ["account parsing document cannot confirm", account(), accountUrl, null, "loading"],
];

function html() {
  const literal = value => JSON.stringify(value).replaceAll("<", "\\u003c");
  return `<!doctype html><body><script>
  (async () => {
    const nonce = new URL(location.href).searchParams.get("nonce"), results = [], errors = [];
    addEventListener("error", event => errors.push(event.message));
    try {
      const script = ${literal(auth)};
      for (const [name, body, url, , readyState] of ${literal(samples)}) {
        const frame = document.createElement("iframe");
        const loaded = new Promise(resolve => { frame.onload = resolve; });
        frame.srcdoc = '<!doctype html><body>' + body + '<input type="password" value="private-password">';
        document.body.appendChild(frame);
        await loaded;
        if (readyState) Object.defineProperty(frame.contentDocument, "readyState", { get() { return readyState; } });
        Object.defineProperty(frame.contentDocument, "cookie", { get() { throw new Error("cookie read"); } });
        Object.defineProperty(frame.contentWindow, "localStorage", { get() { throw new Error("storage read"); } });
        Object.defineProperty(frame.contentWindow.HTMLInputElement.prototype, "value", { get() { throw new Error("form read"); } });
        Object.defineProperty(frame.contentWindow.HTMLElement.prototype, "innerText", { get() { throw new Error("account text read"); } });
        Object.defineProperty(frame.contentWindow.Node.prototype, "textContent", { get() { throw new Error("account text read"); } });
        const run = frame.contentWindow.Function("location", "return " + script);
        results.push({ name, state: run(new URL(url)) });
        frame.remove();
      }
      await fetch('/__reliability_result/' + nonce, { method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ status: 'passed', results, browser_errors: errors }) });
    } catch (error) {
      await fetch('/__reliability_result/' + nonce, { method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ status: 'failed', error: String(error) }) });
    }
  })();
  </script>`;
}

test("Nexus auth uses observed public header and account structures without account or credential reads", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ reactTransform: false, fixtureMiddleware(request, response, next) {
    if (request.url?.split("?")[0] !== "/tests/helpers/runtime-browser.html") return next();
    response.writeHead(200, { "Content-Type": "text/html; charset=utf-8" }).end(html());
  } });
  assert.equal(report.status, "passed", report.error);
  assert.deepEqual(report.results, samples.map(([name, , , state]) => ({ name, state })));
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.browser_exited, true);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
});
