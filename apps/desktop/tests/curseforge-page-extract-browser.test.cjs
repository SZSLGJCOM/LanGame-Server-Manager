const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

const extractor = fs.readFileSync(path.join(__dirname, "../src-tauri/src/curseforge_page_extract.js"), "utf8");
const projectUrl = "https://www.curseforge.com/ark-survival-ascended/mods/devkitlivemodtesting";
const details = (id) => [
  "<section><h3>Details</h3><dl><dt>Downloads</dt><dd>485</dd>",
  "<dt>Created</dt><dd><time>1 year ago</time></dd><dt>Updated</dt><dd><time>1 year ago</time></dd>",
  '<dt>Project ID</dt><dd><span>' + id + '</span><button aria-label="Copy ID"></button></dd>',
  "<dt>License</dt><dd>All Rights Reserved</dd></dl></section>",
].join("");
const page = (body, canonical = projectUrl, title = "DevKitLiveModTesting - ARK Survival Ascended Mods - CurseForge") => [
  "<!doctype html><html><head><title>" + title + "</title>",
  canonical === null ? "" : '<link rel="canonical" href="' + canonical + '">',
  "</head><body><main><h1>DevKitLiveModTesting</h1>", body, "</main></body></html>",
].join("");
const samples = [
  { name: "public project details", html: page(details("1346144") + [
    '<article class="project-description"><p>File 6614383 for mod 1346144, game 83374.</p>',
    "<dl><dt>Created</dt><dd>today</dd><dt>Updated</dt><dd>today</dd><dt>Project ID</dt><dd>999999</dd></dl></article>",
    '<a href="' + projectUrl + '/files/6614383">devkitlivemodtesting-windowsserver 1.zip</a>',
    '<aside><h3>More from iPoNet</h3><a href="/ark-survival-ascended/mods/other-mod">Other Mod</a>',
    "<dl><dt>Downloads</dt><dd>773344</dd><dt>Project ID</dt><dd>112233</dd></dl></aside>",
  ].join("")) },
  { name: "file page keeps project identity", url: projectUrl + "/files/6614383", html: page(details("1346144")) },
  { name: "query tokens stay private", url: projectUrl + "?__cf_chl_tk=synthetic-secret#section",
    html: page(details("1346144"), projectUrl + "?tracking=synthetic-secret") },
  { name: "definition groups", html: page([
    "<dl><div><dt>Downloads</dt><dd>485</dd></div><div><dt>License</dt><dd>All Rights Reserved</dd></div>",
    "<div><dt>Project ID</dt><dd>1346144</dd></div></dl>",
  ].join("")) },
  { name: "duplicate responsive metadata", html: page(details("1346144") + details("1346144")) },
  { name: "unrelated numeric content", html: page([
    '<p>Project ID 112233</p><a href="/projects/1346144">Project</a>',
    '<a href="' + projectUrl + '/files/6614383">File</a><dl><dt>Downloads</dt><dd>1346144</dd></dl>',
    '<script type="application/ld+json">{"id":1346144,"projectId":999999,"slug":"another-project"}</script>',
  ].join("")) },
  { name: "missing canonical", html: page(details("1346144"), null) },
  { name: "mismatched canonical", html: page(details("1346144"), "https://www.curseforge.com/ark-survival-ascended/mods/other-mod") },
  { name: "foreign canonical", html: page(details("1346144"), "https://example.com/ark-survival-ascended/mods/devkitlivemodtesting") },
  { name: "wrong game", url: "https://www.curseforge.com/minecraft/mc-mods/devkitlivemodtesting", html: page(details("1346144")) },
  { name: "conflicting project ids", html: page(details("1346144") + details("112233")) },
  { name: "zero id", html: page(details("0")) },
  { name: "overflow id", html: page(details("4294967296")) },
  { name: "file label is not project id", html: page(details("6614383").replace("Project ID", "File ID")) },
  { name: "missing definition value", html: page(details("1346144")
    .replace('<dd><span>1346144</span><button aria-label="Copy ID"></button></dd>', "<dt>License</dt>")) },
  { name: "cloudflare title", html: page("<div>Checking your browser before proceeding.</div>", null, "Just a moment...") },
  { name: "localized cloudflare title", html: page("", null, "请稍候…").replace("<h1>DevKitLiveModTesting</h1>", "<h1>www.curseforge.com</h1>") },
  { name: "cloudflare challenge element", html: page('<form id="challenge-form"><input name="cf-turnstile-response" value="synthetic-secret"></form>') },
  { name: "too many metadata lists", html: page(details("1346144") + "<dl></dl>".repeat(128)) },
  { name: "missing project heading", html: page(details("1346144")).replace("<h1>DevKitLiveModTesting</h1>", "") },
  { name: "conflicting project headings", html: page(details("1346144") + "<h1>Another project</h1>") },
];

// Reuse the isolated browser lifecycle and report endpoint with a plain DOM
// fixture, so the extraction is tested without a DOM emulation dependency.
function fixtureHtml() {
  const json = (value) => JSON.stringify(value).replaceAll("<", "\\u003c");
  return [
    '<!doctype html><meta charset="utf-8"><script>',
    "(async () => {",
    'const nonce = new URL(location.href).searchParams.get("nonce");',
    "const send = fetch.bind(window); const errors = [];",
    'addEventListener("error", (event) => errors.push(event.message));',
    'addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));',
    "try {",
    'const extract = new Function("document", "location", "return " + ' + json(extractor) + ");",
    'const parse = (html) => new DOMParser().parseFromString(html, "text/html");',
    "const run = (document, url) => {",
    'Object.defineProperty(document, "cookie", { get() { throw new Error("Extractor read cookies"); } });',
    "const previousFetch = window.fetch;",
    'window.fetch = () => { throw new Error("Extractor made a network request"); };',
    "try { return extract(document, { href: url }); } finally { window.fetch = previousFetch; }",
    "};",
    "const results = " + json(samples) + ".map((sample) => ({ name: sample.name,",
    "result: run(parse(sample.html), sample.url ?? " + json(projectUrl) + ") }));",
    "const hydrated = parse(" + json(page('<div id="metadata"></div>')) + ");",
    "const before = run(hydrated, " + json(projectUrl) + ");",
    "const fragment = parse(" + json(details("1346144")) + ");",
    'hydrated.getElementById("metadata").append(...fragment.body.childNodes);',
    "const after = extract(hydrated, { href: " + json(projectUrl) + " });",
    'await send("/__reliability_result/" + nonce, { method: "POST", headers: { "Content-Type": "application/json" },',
    'body: JSON.stringify({ status: "passed", results, hydration: { before, after }, browser_errors: errors }) });',
    "} catch (error) {",
    'await send("/__reliability_result/" + nonce, { method: "POST", headers: { "Content-Type": "application/json" },',
    'body: JSON.stringify({ status: "failed", error: String(error) }) });',
    "}",
    "})();",
    "</script>",
  ].join("\n");
}

test("official project metadata extraction uses real DOM and rejects ambiguous identities", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ reactTransform: false, fixtureMiddleware(request, response, next) {
    if (request.url?.split("?")[0] !== "/tests/helpers/runtime-browser.html") return next();
    response.writeHead(200, { "Content-Type": "text/html; charset=utf-8" }).end(fixtureHtml());
  } });
  const results = new Map(report.results.map(({ name, result }) => [name, result]));
  for (const name of ["public project details", "file page keeps project identity", "query tokens stay private",
    "definition groups", "duplicate responsive metadata"]) {
    assert.equal(results.get(name).state, "ready", name);
    assert.equal(results.get(name).projectId, "1346144", name);
    assert.equal(results.get(name).title, "DevKitLiveModTesting", name);
    assert.equal(results.get(name).canonicalUrl, projectUrl, name);
  }
  assert.equal(results.get("query tokens stay private").url, projectUrl);
  for (const name of ["unrelated numeric content", "missing canonical", "file label is not project id", "missing project heading"]) {
    assert.equal(results.get(name).state, "pending", name);
    assert.equal(results.get(name).projectId, null, name);
  }
  for (const name of ["mismatched canonical", "foreign canonical", "wrong game", "conflicting project ids", "zero id",
    "overflow id", "missing definition value", "too many metadata lists", "conflicting project headings"]) {
    assert.equal(results.get(name).state, "invalid", name);
    assert.equal(results.get(name).projectId, null, name);
  }
  for (const name of ["cloudflare title", "localized cloudflare title", "cloudflare challenge element"]) {
    assert.equal(results.get(name).state, "challenge", name);
    assert.equal(results.get(name).projectId, null, name);
  }
  assert.equal(report.hydration.before.state, "pending");
  assert.equal(report.hydration.after.state, "ready");
  assert.equal(report.hydration.after.projectId, "1346144");
  assert.equal(report.results.length, samples.length);
  for (const { result } of report.results) {
    assert.deepEqual(Object.keys(result).sort(), ["canonicalUrl", "projectId", "state", "title", "url"]);
    assert.ok(JSON.stringify(result).length < 5000);
    assert.doesNotMatch(JSON.stringify(result), /synthetic-secret|cf-turnstile-response|<html>/);
  }
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.browser_exited, true);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
});
