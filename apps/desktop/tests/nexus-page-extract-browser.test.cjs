const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

const extractor = fs.readFileSync(path.join(__dirname, "../src-tauri/src/nexus_page_extract.js"), "utf8");
const projectUrl = "https://www.nexusmods.com/7daystodie/mods/9559";
const iconUrl = "https://staticdelivery.nexusmods.com/mods/1059/images/9559/9559-1770000000-123456789.png";
const title = "Shin Server Tools";
const page = ({ canonical = projectUrl, socialTitle = title + " at 7 Days to Die Nexus - Mods and community",
  heading = title, image = iconUrl, extraHead = "", body = "", documentTitle = title } = {}) => [
  "<!doctype html><html><head><title>" + documentTitle + "</title>",
  canonical === null ? "" : '<link rel="canonical" href="' + canonical + '">',
  socialTitle === null ? "" : '<meta property="og:title" content="' + socialTitle + '">',
  image === null ? "" : '<meta property="og:image" content="' + image + '">', extraHead,
  '</head><body><nav><h1>Please log in</h1></nav><main><div id="pagetitle">',
  heading === null ? "" : "<h1>" + heading + "</h1>", "</div>", body, "</main></body></html>",
].join("");
const samples = [
  { name: "public project", html: page(), state: "ready", icon: iconUrl },
  { name: "modern project path", url: projectUrl.replace("/7daystodie/", "/games/7daystodie/"), html: page(), state: "ready", icon: iconUrl },
  { name: "modern canonical path", html: page({ canonical: projectUrl.replace("/7daystodie/", "/games/7daystodie/") }), state: "ready", icon: iconUrl },
  { name: "files tab", url: projectUrl + "?tab=files&file_id=88", html: page(), state: "ready", icon: iconUrl },
  { name: "file path", url: projectUrl + "/files/88", html: page(), state: "ready", icon: iconUrl },
  { name: "files path", url: projectUrl + "/files", html: page(), state: "ready", icon: iconUrl },
  { name: "query secrets removed", url: projectUrl + "?key=synthetic-test-only#private",
    html: page({ canonical: projectUrl + "?tracking=synthetic-test-only" }), state: "ready", icon: iconUrl },
  { name: "social title without heading", html: page({ heading: null }), state: "ready", icon: iconUrl },
  { name: "project heading without social title", html: page({ socialTitle: null }), state: "ready", icon: iconUrl },
  { name: "modern social title suffix", html: page({ socialTitle: title + " - Nexus Mods" }), state: "ready", icon: iconUrl },
  ...["Server at Home", "Server at Home at Night"].flatMap((name) => [
    ...[name, name + " at 7 Days to Die Nexus - Mods and community", name + " - Nexus Mods"].flatMap((socialTitle) =>
      [name, null].map((heading) => ({ name: `at inside title: ${socialTitle}, heading ${heading !== null}`,
        html: page({ heading, socialTitle }), state: "ready", icon: iconUrl, expectedTitle: name }))),
    { name: `at inside heading without OG: ${name}`, html: page({ heading: name, socialTitle: null }),
      state: "ready", icon: iconUrl, expectedTitle: name },
  ]),
  { name: "literal Nexus in title is not a site suffix", html: page({ heading: null, socialTitle: "Home at the Nexus" }),
    state: "ready", icon: iconUrl, expectedTitle: "Home at the Nexus" },
  { name: "heading validates bare legacy suffix", html: page({ heading: "Server at Home", socialTitle: "Server at Home at 7 Days to Die Nexus" }),
    state: "ready", icon: iconUrl, expectedTitle: "Server at Home" },
  { name: "at title conflicts with unrelated heading", html: page({ heading: "Another project", socialTitle: "Server at Home at 7 Days to Die Nexus - Mods and community" }), state: "invalid" },
  { name: "at title does not swallow arbitrary suffix", html: page({ heading: "Server at Home", socialTitle: "Server at Home and another project" }), state: "invalid" },
  { name: "conflicting suffixed at titles", html: page({ heading: null, socialTitle: "Server at Home at 7 Days to Die Nexus - Mods and community",
    extraHead: '<meta property="og:title" content="Server at Night at 7 Days to Die Nexus - Mods and community">' }), state: "invalid" },
  { name: "duplicate canonical identity", html: page({ extraHead: '<link rel="canonical" href="' + projectUrl + '/">' }), state: "ready", icon: iconUrl },
  { name: "matching social URL", html: page({ extraHead: '<meta property="og:url" content="' + projectUrl + '">' }), state: "ready", icon: iconUrl },
  { name: "ignore author description heading", html: page({ body: '<article id="mod_description"><h1>Another project</h1></article>' }), state: "ready", icon: iconUrl },
  { name: "missing image", html: page({ image: null }), state: "ready", icon: null },
  { name: "body images are not project icon", html: page({ image: null, body: '<aside><img src="' + iconUrl + '"><meta property="og:image" content="' + iconUrl + '"></aside>' }), state: "ready", icon: null },
  { name: "author avatar is not project icon", html: page({ image: "https://avatars.nexusmods.com/1234/100" }), state: "ready", icon: null },
  { name: "foreign icon", html: page({ image: "https://evil.invalid/icon.png" }), state: "ready", icon: null },
  { name: "signed image", html: page({ image: iconUrl + "?token=synthetic-test-only" }), state: "ready", icon: null },
  { name: "credential image", html: page({ image: iconUrl.replace("https://", "https://user:fixture@") }), state: "ready", icon: null },
  { name: "download is not icon", html: page({ image: "https://cf-files.nexusmods.com/1059/88/package.zip?key=synthetic-test-only" }), state: "ready", icon: null },
  { name: "static archive is not icon", html: page({ image: iconUrl.replace(".png", ".zip") }), state: "ready", icon: null },
  { name: "conflicting icons", html: page({ extraHead: '<meta property="og:image:secure_url" content="' + iconUrl.replace("123456789", "987654321") + '">' }), state: "ready", icon: null },
  { name: "duplicate icon", html: page({ extraHead: '<meta property="og:image:secure_url" content="' + iconUrl + '">' }), state: "ready", icon: iconUrl },
  { name: "missing canonical uses current project", html: page({ canonical: null }), state: "ready", icon: iconUrl, canonical: projectUrl },
  { name: "missing canonical strips query secrets", url: projectUrl + "?tab=files&file_id=88&key=synthetic-test-only#private",
    html: page({ canonical: null }), state: "ready", icon: iconUrl, canonical: projectUrl },
  { name: "missing canonical on modern file path", url: projectUrl.replace("/7daystodie/", "/games/7daystodie/") + "/files/88",
    html: page({ canonical: null }), state: "ready", icon: iconUrl, canonical: projectUrl.replace("/7daystodie/", "/games/7daystodie/") + "/files/88" },
  { name: "missing canonical still checks social identity", html: page({ canonical: null,
    extraHead: '<meta property="og:url" content="' + projectUrl.replace("9559", "9560") + '">' }), state: "invalid" },
  { name: "missing canonical cannot provide a title", html: page({ canonical: null, socialTitle: null, heading: null }), state: "pending" },
  { name: "missing canonical cannot make login a project", html: page({ canonical: null, socialTitle: "Please log in", heading: "Please log in" }), state: "pending" },
  { name: "missing title", html: page({ socialTitle: null, heading: null }), state: "pending" },
  { name: "login title", html: page({ socialTitle: "Please log in", heading: "Please log in" }), state: "pending" },
  { name: "different canonical game", html: page({ canonical: projectUrl.replace("7daystodie", "enshrouded") }), state: "invalid" },
  { name: "different canonical mod", html: page({ canonical: projectUrl.replace("9559", "9560") }), state: "invalid" },
  { name: "conflicting canonicals", html: page({ extraHead: '<link rel="canonical" href="' + projectUrl.replace("9559", "9560") + '">' }), state: "invalid" },
  { name: "different social identity", html: page({ extraHead: '<meta property="og:url" content="' + projectUrl.replace("9559", "9560") + '">' }), state: "invalid" },
  { name: "foreign canonical", html: page({ canonical: projectUrl.replace("www.nexusmods.com", "evil.invalid") }), state: "invalid" },
  { name: "conflicting social titles", html: page({ extraHead: '<meta property="og:title" content="Another project">' }), state: "invalid" },
  { name: "heading disagrees with social title", html: page({ heading: "Another project" }), state: "invalid" },
  { name: "ambiguous headings without social title", html: page({ socialTitle: null, body: "<h1>Another project</h1>" }), state: "invalid" },
  { name: "foreign current host", url: projectUrl.replace("www.nexusmods.com", "www.nexusmods.com.evil.invalid"), html: page(), state: "invalid" },
  { name: "http project", url: projectUrl.replace("https:", "http:"), html: page(), state: "invalid" },
  { name: "credential project", url: projectUrl.replace("https://", "https://user:fixture@"), html: page(), state: "invalid" },
  { name: "custom port", url: projectUrl.replace(".com/", ".com:8443/"), html: page(), state: "invalid" },
  { name: "zero id", url: projectUrl.replace("9559", "0"), html: page(), state: "invalid" },
  { name: "overflow id", url: projectUrl.replace("9559", "18446744073709551616"), html: page(), state: "invalid" },
  { name: "u64 id stays exact", url: projectUrl.replace("9559", "18446744073709551615"), html: page({ canonical: projectUrl.replace("9559", "18446744073709551615") }), state: "ready", icon: iconUrl, modId: "18446744073709551615" },
  { name: "too many headings", html: page({ body: "<h1>ignored</h1>".repeat(16) }), state: "invalid" },
  { name: "oversized title", html: page({ socialTitle: "x".repeat(1025) }), state: "invalid" },
  { name: "challenge title", html: page({ documentTitle: "Just a moment...", canonical: null }), state: "challenge" },
  { name: "challenge form", html: page({ body: '<form id="challenge-form"><input name="cf-turnstile-response" value="synthetic-test-only"></form>' }), state: "challenge" },
];

function fixtureHtml() {
  const json = (value) => JSON.stringify(value).replaceAll("<", "\\u003c");
  return [
    '<!doctype html><meta charset="utf-8"><script>', "(async () => {",
    'const nonce = new URL(location.href).searchParams.get("nonce"); const send = fetch.bind(window); const errors = [];',
    'addEventListener("error", event => errors.push(event.message));',
    'addEventListener("unhandledrejection", event => errors.push(String(event.reason)));',
    "try {", 'const extract = new Function("document", "location", "return " + ' + json(extractor) + ");",
    "const results = " + json(samples) + ".map(sample => {",
    'const document = new DOMParser().parseFromString(sample.html, "text/html");',
    'Object.defineProperty(document, "cookie", { get() { throw new Error("Extractor read cookies"); } });',
    'const previousFetch = window.fetch; window.fetch = () => { throw new Error("Extractor made a network request"); };',
    "try { return { name: sample.name, result: extract(document, { href: sample.url ?? " + json(projectUrl) + " }) }; }",
    "finally { window.fetch = previousFetch; }", "});",
    'await send("/__reliability_result/" + nonce, { method: "POST", headers: { "Content-Type": "application/json" },',
    'body: JSON.stringify({ status: "passed", results, browser_errors: errors }) });',
    "} catch (error) {",
    'await send("/__reliability_result/" + nonce, { method: "POST", headers: { "Content-Type": "application/json" },',
    'body: JSON.stringify({ status: "failed", error: String(error) }) });', "}", "})();", "</script>",
  ].join("\n");
}

test("Nexus public project metadata verifies identity and keeps optional icons separate from downloads", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ reactTransform: false, fixtureMiddleware(request, response, next) {
    if (request.url?.split("?")[0] !== "/tests/helpers/runtime-browser.html") return next();
    response.writeHead(200, { "Content-Type": "text/html; charset=utf-8" }).end(fixtureHtml());
  } });
  assert.equal(report.status, "passed", report.error);
  assert.equal(report.results.length, samples.length);
  const results = new Map(report.results.map(({ name, result }) => [name, result]));
  for (const sample of samples) {
    const result = results.get(sample.name);
    assert.equal(result.state, sample.state, sample.name);
    if (sample.state === "ready") {
      assert.equal(result.modId, sample.modId ?? "9559", sample.name);
      assert.equal(result.gameDomain, "7daystodie", sample.name);
      assert.equal(result.title, sample.expectedTitle ?? title, sample.name);
      assert.equal(result.iconUrl, sample.icon, sample.name);
      if (sample.canonical) assert.equal(result.canonicalUrl, sample.canonical, sample.name);
    } else {
      assert.equal(result.modId, null, sample.name);
      assert.equal(result.gameDomain, null, sample.name);
    }
    assert.deepEqual(Object.keys(result).sort(), ["canonicalUrl", "gameDomain", "iconUrl", "modId", "state", "title", "url"]);
    assert.ok(JSON.stringify(result).length < 5000, sample.name);
    assert.doesNotMatch(JSON.stringify(result), /synthetic-test-only|user:fixture|cf-turnstile-response|<html>/, sample.name);
  }
  assert.equal(results.get("query secrets removed").url, projectUrl);
  assert.equal(results.get("query secrets removed").canonicalUrl, projectUrl);
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.browser_exited, true);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
});
