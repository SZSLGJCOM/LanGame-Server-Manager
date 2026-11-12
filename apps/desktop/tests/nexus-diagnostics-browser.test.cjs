const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

const script = fs.readFileSync(path.join(__dirname, "../src-tauri/src/nexus_browser_diagnostics.js"), "utf8");
const fileControls = fs.readFileSync(path.join(__dirname, "../src-tauri/src/nexus_browser_file_controls.js"), "utf8");
const project = "https://www.nexusmods.com/7daystodie/mods/2264";
const manual = (id, href = project + "?tab=files&file_id=" + id) => `<a href="${href}">Manual download</a>`;
const component = (fileId = "88", attributes = "", gameDomain = "/7daystodie") =>
  `<mod-file-download file-id="${fileId}" game-domain="${gameDomain}" ${attributes}></mod-file-download>`;
const modalListing = '<div id="mod_files"><section id="file-container-main-files"><h2>Main files</h2><dl class="accordion">'
  + '<dt data-id="88"></dt><dd data-id="88"><div class="tabbed-block"><ul class="accordion-downloads"><li>'
  + '<mod-download-modal placement="files_tab" file=\'{"uid":"1059000088","downloadUrl":"/api/files/1059000088/download"}\'></mod-download-modal>'
  + '</li></ul></div></dd></dl></section></div>';
const samples = [
  { name: "legacy filename stays inside DOM and does not authorize an unrelated portal",
    body: '<div id="mod_files"><section id="file-container-main-files"><h2>Main files</h2><dl class="accordion">'
      + '<dt data-id="385" data-name="fixture-client-secret_Server_P"></dt><dd data-id="385"><ul class="accordion-downloads"><li>'
      + manual("385") + '</li></ul></dd></dl></section></div>',
    portal: '<div role="dialog"><a href="/api/files/1059000385/download">Manual download</a></div>',
    expected: { manualControlCount: 1, matchingManualCount: 1, visibleDialogCount: 0 } },
  { name: "PLAP new files component", body: modalListing, modalShadow: '<button>Manual download</button>',
    expected: { mainHeadingCount: 1, manualControlCount: 1, matchingManualCount: 1 } },
  { name: "new component ordinary site link", body: modalListing, modalShadow: '<a href="/api/files/1059000088/download">Manual download</a>',
    expected: { manualControlCount: 1, matchingManualCount: 1 } },
  { name: "new component link cannot borrow another UID", body: modalListing, modalShadow: '<a href="/api/files/1059000099/download">Manual download</a>',
    expected: { manualControlCount: 1, matchingManualCount: 0 } },
  { name: "new component row must agree", body: modalListing.replace('dt data-id="88"','dt data-id="99"'), modalShadow: '<button>Manual download</button>',
    expected: { manualControlCount: 0, matchingManualCount: 0 } },
  { name: "new component outside files cannot supply controls", body: '<div id="description">'+modalListing+'</div>', modalShadow: '<button>Manual download</button>',
    expected: { manualControlCount: 0, matchingManualCount: 0 } },
  { name: "file listing", body: '<main><h2>Main files</h2>' + manual("88")
      + manual("99", "https://untrusted.invalid/?file_id=99") + '</main>',
    expected: { mainHeadingCount: 1, manualControlCount: 2, matchingManualCount: 1 } },
  { name: "selected file", url: project + "?file_id=88&key=fixture-client-secret", body:
      '<button disabled>Slow Download</button><a href="#download">Slow Download</a>'
      + '<a href="?file_id=99">Slow Download</a><a href="https://cf-files.nexusmods.com/file.zip?key=fixture-client-secret">Download</a>',
    expected: { fileId: "88", slowControlCount: 3, matchingSlowCount: 2 } },
  { name: "login cell", body: '<table><tr><td>You must be logged in to download files.</td></tr></table>',
    expected: { loginNoticeCount: 1 } },
  { name: "PLAP files-page download gate", body: '<div id="loggedOutPremiumBanner" class="premium-banner"><span>You have to be logged in to download files</span><a href="https://users.nexusmods.com/auth/sign_in">Log in</a></div><h2>Main files</h2>',
    expected: { loginNoticeCount: 1, mainHeadingCount: 1, manualControlCount: 0 } },
  { name: "PLAP selected-file shadow download button", url: project + "?file_id=88", body: component(),
    shadow: '<button>Slow download</button><input type="password" value="fixture-client-secret">',
    expected: { fileId: "88", slowControlCount: 1, matchingSlowCount: 1 } },
  { name: "bare game domain remains valid", url: project + "?file_id=88", body: component("88", "", "7daystodie"),
    shadow: '<button>Slow download</button>', expected: { slowControlCount: 1, matchingSlowCount: 1 } },
  ...["/valheim", "//7daystodie", "/7daystodie/other"].map(gameDomain => ({
    name: `unmatched game domain ${gameDomain}`, url: project + "?file_id=88", body: component("88", "", gameDomain),
    shadow: '<button>Slow download</button>', expected: { slowControlCount: 0, matchingSlowCount: 0 } })),
  { name: "shadow host with display contents", url: project + "?file_id=88", body: component("88", 'style="display:contents"'),
    shadow: '<button>Slow download</button>', expected: { slowControlCount: 1, matchingSlowCount: 1 } },
  { name: "wrong file shadow stays outside diagnostics", url: project + "?file_id=88", body: component("99"),
    shadow: '<button>Slow download</button>', expected: { slowControlCount: 0, matchingSlowCount: 0 } },
  { name: "author shadow stays outside diagnostics", url: project + "?file_id=88", body: '<div id="description">' + component() + '</div>',
    shadow: '<button>Slow download</button>', expected: { slowControlCount: 0, matchingSlowCount: 0 } },
  { name: "hidden shadow host stays outside diagnostics", url: project + "?file_id=88", body: component("88", "hidden"),
    shadow: '<button>Slow download</button>', expected: { slowControlCount: 0, matchingSlowCount: 0 } },
  { name: "component reports its explicit login gate", url: project + "?file_id=88", body: component("88", 'user-is-logged-in="false"'),
    shadow: '<a href="https://users.nexusmods.com/auth/sign_in">Slow download</a>',
    expected: { loginNoticeCount: 1, slowControlCount: 1, matchingSlowCount: 0 } },
  { name: "official portal dialog is reported", url: project + "?file_id=88", body: component(),
    shadow: '<button>Slow download</button>', portal: '<div role="dialog"><button>Continue</button></div>',
    expected: { slowControlCount: 1, visibleDialogCount: 1 } },
  { name: "hidden and author content", body: '<div><section id="description"><p>You must be logged in.</p>'
      + manual("44") + '</section></div><p hidden>You must be logged in.</p><button hidden>Slow Download</button>'
      + '<textarea>You must be logged in. fixture-client-secret</textarea><input type="password" value="fixture-client-secret">'
      + '<input type="submit" value="Download fixture-client-secret">',
    expected: { loginNoticeCount: 0, manualControlCount: 0, slowControlCount: 1, matchingSlowCount: 0 } },
  { name: "private account page is not inspected", url: "https://users.nexusmods.com/auth/sign_in?token=fixture-client-secret",
    body: '<h1>fixture-client-secret</h1><p>You must be logged in.</p><button>Download fixture-client-secret</button>',
    expected: { loginNoticeCount: 0, manualControlCount: 0, slowControlCount: 0, controls: [] } },
  { name: "diagnostics stay bounded", body: Array.from({ length: 12 }, (_, index) => manual(String(index + 1))).join(""),
    expected: { manualControlCount: 12, matchingManualCount: 12 } },
  { name: "only categories leave the page", body: '<div role="dialog"><button>Download fixture-client-secret</button></div>'
      + manual("88", project + "?file_id=88&token=fixture-client-secret")
      + '<a href="https://user:fixture@www.nexusmods.com/7daystodie/mods/2264?file_id=99">Manual download</a>',
    expected: { visibleDialogCount: 1, manualControlCount: 2, matchingManualCount: 1 } },
];

function fixtureHtml() {
  const json = (value) => JSON.stringify(value).replaceAll("<", "\\u003c");
  return `<!doctype html><body><script>
  (async () => {
    const nonce = new URL(location.href).searchParams.get("nonce"), results = [], errors = [];
    addEventListener("error", event => errors.push(event.message));
    try {
      for (const sample of ${json(samples)}) {
        const frame = document.createElement("iframe"); frame.style.cssText = "width:1000px;height:700px";
        const loaded = new Promise(resolve => frame.onload = resolve);
        frame.srcdoc = "<!doctype html><body>" + sample.body;
        document.body.append(frame); await loaded;
        const doc = frame.contentDocument;
        if (sample.shadow) doc.querySelector("mod-file-download").attachShadow({mode:"open"}).innerHTML = sample.shadow;
        if (sample.modalShadow) doc.querySelector('mod-download-modal').attachShadow({mode:'open'}).innerHTML=sample.modalShadow;
        if (sample.portal) {
          const portal = doc.createElement("div"); portal.id = "next-shadow-root"; doc.body.append(portal);
          portal.attachShadow({mode:"open"}).innerHTML = sample.portal;
        }
        Object.defineProperty(doc, "cookie", { get() { throw new Error("Diagnostics read cookies"); } });
        const roots = [doc, ...[...doc.querySelectorAll("mod-file-download,#next-shadow-root")].flatMap(element => element.shadowRoot ? [element.shadowRoot] : [])];
        for (const input of roots.flatMap(root => [...root.querySelectorAll("input,textarea,select")])) {
          Object.defineProperty(input, "value", { get() { throw new Error("Diagnostics read form values"); } });
        }
        frame.contentWindow.fetch = () => { throw new Error("Diagnostics made a network request"); };
        let clicks = 0; doc.addEventListener("click", event => { event.preventDefault(); clicks++; });
        const location=new URL(sample.url ?? ${json(project)}), target={gameDomain:'7daystodie',modId:'2264'};
        const bound=new frame.contentWindow.Function('location','return '+${json(fileControls)})(location)(target);
        const result = new frame.contentWindow.Function("location", "return " + ${json(script)})(location)(target,bound);
        results.push({ name: sample.name, result, clicks }); frame.remove();
      }
      await fetch("/__reliability_result/" + nonce, {method:"POST",headers:{"Content-Type":"application/json"},
        body:JSON.stringify({status:"passed",results,browser_errors:errors})});
    } catch (error) {
      await fetch("/__reliability_result/" + nonce, {method:"POST",headers:{"Content-Type":"application/json"},
        body:JSON.stringify({status:"failed",error:String(error)})});
    }
  })();</script>`;
}

test("Nexus failure diagnostics explain controls without exporting page or account data", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ reactTransform: false, fixtureMiddleware(request, response, next) {
    if (request.url?.split("?")[0] !== "/tests/helpers/runtime-browser.html") return next();
    response.writeHead(200, { "Content-Type": "text/html; charset=utf-8" }).end(fixtureHtml());
  } });
  assert.equal(report.status, "passed", report.error);
  assert.equal(report.results.length, samples.length);
  for (const sample of samples) {
    const { result, clicks } = report.results.find((item) => item.name === sample.name);
    for (const [key, value] of Object.entries(sample.expected)) assert.deepEqual(result[key], value, sample.name + ": " + key);
    assert.equal(clicks, 0, sample.name);
    assert.ok(result.controls.length <= 8, sample.name);
    assert.doesNotMatch(JSON.stringify(result), /fixture-client-secret|https?:|<html|untrusted|file\.zip|user:/, sample.name);
    for (const control of result.controls) {
      assert.deepEqual(Object.keys(control).sort(), ["disabled", "displayed", "fileId", "kind", "matchesProject", "tag"]);
    }
  }
  assert.equal(report.results.find((item) => item.name === "diagnostics stay bounded").result.controls.length, 8);
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.browser_exited, true);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
});
