const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

const automatic = fs.readFileSync(path.join(__dirname, "../src-tauri/src/nexus_browser_automatic.js"), "utf8");
const extractor = fs.readFileSync(path.join(__dirname, "../src-tauri/src/nexus_page_extract.js"), "utf8");
const fileControls = fs.readFileSync(path.join(__dirname, "../src-tauri/src/nexus_browser_file_controls.js"), "utf8");
const project = "https://www.nexusmods.com/7daystodie/mods/2264";
const manual = (id, extra = "", url = `${project}?tab=files&file_id=${id}`) =>
  `<a href="${url}" data-action="manual-${id}" ${extra}>Manual download</a>`;
const main = (...files) => `<section><h2>Main files</h2>${files.join("")}</section>`;
const slow = (extra = "") => `<button data-action="slow" ${extra}>Slow Download</button>`;
// PLAP's file 51828 uses this official custom element; its public bundle
// attaches an open shadow root and renders the ordinary Slow download button.
const downloadComponent = (fileId = "88", attributes = "", gameDomain = "/7daystodie") =>
  `<mod-file-download file-id="${fileId}" game-domain="${gameDomain}" user-is-logged-in="true" ${attributes}></mod-file-download>`;
const modalRow = (fileId = "88", uid = "1059000088", options = {}) => {
  const file = JSON.stringify({ uid, name: options.name, downloadUrl: options.href ?? `/api/files/${uid}/download`, category: 1 })
    .replaceAll("&", "&amp;").replaceAll("'", "&#39;");
  return `<dl class="accordion"><dt data-id="${options.headerId ?? fileId}"></dt><dd data-id="${fileId}" ${options.rowExtra ?? ""}>
    <div class="tabbed-block"><ul class="accordion-downloads"><li><mod-download-modal placement="files_tab" file='${file}' ${options.extra ?? ""}></mod-download-modal>
    </li></ul></div></dd></dl>`;
};
const modalListing = (...rows) => `<div id="mod_files"><section id="file-container-main-files"><h2>Main files</h2>${rows.join("")}</section></div>`;
const hybridRow = (fileId, uid, name) => modalRow(fileId, uid, { name })
  .replace('</li></ul>', `${manual(fileId)}</li></ul>`);
// HumanitZ 91's actual legacy file list keeps names in its matching DT header.
const legacyRow = (fileId, name, options = {}) => `<dl class="accordion"><dt data-id="${options.headerId ?? fileId}"
  data-name="${name.replaceAll("&", "&amp;").replaceAll('"', "&quot;")}"></dt><dd data-id="${fileId}" ${options.rowExtra ?? ""}>
  <ul class="accordion-downloads"><li>${manual(options.linkId ?? fileId, options.extra ?? "")}</li></ul></dd></dl>`;
const miscListing = (...rows) => modalListing(...rows)
  .replace('id="file-container-main-files"', 'id="file-container-miscellaneous-files"')
  .replace('<h2>Main files</h2>', '<h2>Miscellaneous files</h2>');
// PLAP's Enshrouded Mod 326 has this site-generated notice in place of Manual.
const quarantineRow = (fileId = "1948", options = {}) => `<dl class="accordion"><dt data-id="${options.headerId ?? fileId}"></dt>
  <dd data-id="${fileId}" ${options.rowExtra ?? ""}><div class="tabbed-block"><ul class="accordion-downloads">
  <li class="quarantine-notice" ${options.extra ?? ""}><p>${options.notice ?? "This file has been quarantined by our automated safety checks. This could be due to:"}</p>
  <ul><li>Executables and similar file types</li></ul></li></ul></div></dd></dl>`;
// Public download-gate markup recovered from PLAP's actual 13866 files page.
const guestBanner = '<div id="loggedOutPremiumBanner" class="premium-banner"><span>You have to be logged in to download files</span>'
  + '<a class="btn" href="https://users.nexusmods.com/auth/sign_in" data-action="login">Log in</a>'
  + '<a href="https://users.nexusmods.com/register" data-action="register">Register</a></div>';
const samples = [
  // Nexus 147's published Files page uses Miscellaneous files. PLAP observed
  // no Main heading and five ordinary Manual links, including older files.
  { name: "sole bound Miscellaneous file excludes old versions when Main is absent",
    body: miscListing(legacyRow("1719", "Shroudforge"))
      .replace(/<\/div>$/, '<section id="file-container-old-files"><h2>Old files</h2>'
        + legacyRow("1649", "Shroudforge old") + legacyRow("1615", "Shroudforge older") + '</section></div>'),
    clicks: ["manual-1719"] },
  { name: "bound Miscellaneous file does not require an optional filename hint",
    body: miscListing(legacyRow("1719", "Shroudforge").replace('data-name="Shroudforge"', '')),
    clicks: ["manual-1719"] },
  { name: "bound Miscellaneous category survives a heading count suffix",
    body: miscListing(legacyRow("1719", "Shroudforge")).replace('<h2>Miscellaneous files</h2>', '<h2>Miscellaneous files (1)</h2>'),
    clicks: ["manual-1719"] },
  { name: "modern sole Miscellaneous file still requires native UID authorization",
    body: miscListing(modalRow("1719", "1059001719")), modal: true,
    selection: { fileId: "1719", fileUid: "1059001719" }, clicks: ["modal-1719"] },
  { name: "two Miscellaneous variants remain a choice", body: miscListing(
    legacyRow("1719", "Variant A"), legacyRow("1649", "Variant B")),
    clicks: [], state: "blocked", reason: "file_selection" },
  { name: "Main file takes precedence over Miscellaneous fallback", body: main(manual("88"))
    + miscListing(legacyRow("1719", "Shroudforge")), clicks: ["manual-88"] },
  { name: "pending Main controls are not replaced with Miscellaneous", body: main('<p>Loading files</p>')
    + miscListing(legacyRow("1719", "Shroudforge")), clicks: [], afterWait: "blocked" },
  { name: "pending Main count heading is not replaced with Miscellaneous", body: main('<p>Loading files</p>')
    .replace('Main files', 'Main files (1)') + miscListing(legacyRow("1719", "Shroudforge")),
    clicks: [], afterWait: "blocked" },
  { name: "Main count heading retains selection precedence", body: main(manual("88"))
    .replace('Main files', 'Main files (1)') + miscListing(legacyRow("1719", "Shroudforge")), clicks: ["manual-88"] },
  { name: "explicit Miscellaneous file overrides a pending Main count heading", fileId: "1719", body: main('<p>Loading files</p>')
    .replace('Main files', 'Main files (1)') + miscListing(legacyRow("1719", "Shroudforge")), clicks: ["manual-1719"] },
  { name: "empty Main container does not hide a sole Miscellaneous file", body: miscListing(legacyRow("1719", "Shroudforge"))
    .replace('<div id="mod_files">', '<div id="mod_files"><section id="file-container-main-files"></section>'),
    clicks: ["manual-1719"] },
  { name: "unique bound server Miscellaneous variant is preferred", body: miscListing(
    legacyRow("1719", "Bundle_Server_P"), legacyRow("1649", "Bundle_Client_P")), clicks: ["manual-1719"] },
  { name: "unhydrated Main is not replaced with Miscellaneous", body: modalListing(modalRow())
    .replace(/<\/div>$/, '<section id="file-container-miscellaneous-files"><h2>Miscellaneous files</h2>'
      + legacyRow("1719", "Shroudforge") + '</section></div>'),
    selection: { fileId: "88", fileUid: "1059000088" }, clicks: [], afterWait: "blocked" },
  { name: "quarantined Main is not replaced with Miscellaneous", body: modalListing(quarantineRow())
    .replace(/<\/div>$/, '<section id="file-container-miscellaneous-files"><h2>Miscellaneous files</h2>'
      + legacyRow("1719", "Shroudforge") + '</section></div>'),
    clicks: [], state: "blocked", reason: "file_quarantined" },
  { name: "explicit Miscellaneous file takes precedence over Main", fileId: "1719", body: main(manual("88"))
    + miscListing(legacyRow("1719", "Shroudforge")), clicks: ["manual-1719"] },
  { name: "Miscellaneous fallback requires a verified file row", body: miscListing(
    legacyRow("1719", "Shroudforge", { headerId: "88" })), clicks: [], afterWait: "blocked" },
  { name: "author quoted Miscellaneous list is not a fallback", body: '<div id="description">'
    + miscListing(legacyRow("1719", "Shroudforge")) + '</div>', clicks: [], afterWait: "blocked" },
  { name: "sole old file does not become Miscellaneous", body: miscListing()
    .replace('Miscellaneous files', 'Old files') + legacyRow("1719", "Shroudforge"), clicks: [], afterWait: "blocked" },
  { name: "an old file title is not a Miscellaneous category", body: miscListing(legacyRow("1719", "Shroudforge"))
    .replace('file-container-miscellaneous-files', 'file-container-old-files')
    .replace('<h2>Miscellaneous files</h2>', '<h2>Old files</h2>')
    .replace('</dt>', '<h3>Miscellaneous files</h3></dt>'), clicks: [], afterWait: "blocked" },
  { name: "legacy matching DT name selects the unique server Main and excludes old server files",
    body: modalListing(legacyRow("385", "CraftingRecipes_Server_P"), legacyRow("384", "CraftingRecipes_P"))
      .replace(/<\/div>$/, '<section id="file-container-old-files"><h2>Old files</h2>'
        + legacyRow("352", "CraftingRecipes_Server_P") + '</section></div>'), clicks: ["manual-385"] },
  { name: "legacy server can use the site's ordinary collapsed Manual link", body: modalListing(
    legacyRow("385", "CraftingRecipes_Server_P", { rowExtra: 'style="display:none"' }), legacyRow("384", "CraftingRecipes_P")),
    clicks: ["manual-385"] },
  { name: "explicit legacy client wins over the server hint", fileId: "384", body: modalListing(
    legacyRow("385", "CraftingRecipes_Server_P"), legacyRow("384", "CraftingRecipes_P")), clicks: ["manual-384"] },
  ...[
    ["mismatched DT file ID", { headerId: "384" }],
    ["Manual destination for another file", { linkId: "386" }],
    ["hidden server hint", { extra: "hidden" }],
  ].map(([name, options]) => ({ name: `legacy name cannot authorize ${name}`, body: modalListing(
    legacyRow("385", "CraftingRecipes_Server_P", options), legacyRow("384", "CraftingRecipes_P")),
    clicks: [], state: "blocked", reason: "file_selection" })),
  { name: "two legacy server variants remain ambiguous", body: modalListing(
    legacyRow("385", "CraftingRecipes_Server_HD"), legacyRow("384", "CraftingRecipes_Server_Lite")),
    clicks: [], state: "blocked", reason: "file_selection" },
  { name: "legacy author prose cannot replace a bound filename", body: modalListing(
    legacyRow("385", "CraftingRecipes_P").replace('<ul class="accordion-downloads">', '<p>Server-only</p><ul class="accordion-downloads">'),
    legacyRow("384", "CraftingRecipes_P")), clicks: [], state: "blocked", reason: "file_selection" },
  { name: "legacy filename hint does not admit an unrelated shadow portal", body: modalListing(
    legacyRow("385", "CraftingRecipes_Server_P"), legacyRow("384", "CraftingRecipes_P")),
    portal: '<div role="dialog"><h2>Download mod file</h2><a data-action="unbound" href="/api/files/1059000385/download">Manual download</a></div>',
    clicks: ["manual-385"] },
  { name: "visible legacy Manual keeps the matching modern server identity", body: modalListing(
    hybridRow("88", "1059000088", "CraftingRecipes_Server_P"), hybridRow("99", "1059000099", "CraftingRecipes_P")),
    modal: true, selection: { fileId: "88", fileUid: "1059000088" }, clicks: ["manual-88"] },
  { name: "legacy Manual cannot borrow a name from an invalid modern identity", body: modalListing(
    hybridRow("88", "0", "CraftingRecipes_Server_P"), hybridRow("99", "1059000099", "CraftingRecipes_P")),
    modal: true, clicks: [], state: "blocked", reason: "file_selection" },
  { name: "same file ID with conflicting modern UIDs stays ambiguous", body: modalListing(
    hybridRow("88", "1059000088", "CraftingRecipes_Server_P"), hybridRow("88", "1059000099", "CraftingRecipes_Server_P")),
    modal: true, clicks: [], state: "blocked", reason: "file_selection" },
  // Public Main-file names from https://www.nexusmods.com/humanitz/mods/91?tab=files.
  // File IDs/UIDs remain synthetic; identity and ordinary controls use the real DOM shape.
  { name: "HumanitZ server Main is selected by its bound name rather than a larger file ID",
    gameDomain: "humanitz", modId: "91", url: "https://www.nexusmods.com/humanitz/mods/91",
    canonical: "https://www.nexusmods.com/humanitz/mods/91",
    body: modalListing(modalRow("99", "1059000099", { name: "CraftingRecipes_P" }),
      modalRow("88", "1059000088", { name: "CraftingRecipes_Server_P" })), modal: true,
    selection: { fileId: "88", fileUid: "1059000088" }, clicks: ["modal-88"] },
  { name: "server-only Main still waits for native file authorization", body: modalListing(
    modalRow("88", "1059000088", { name: "Bundle - Server-only" }), modalRow("99", "1059000099", { name: "Bundle" })),
    modal: true, authorize: false, selection: { fileId: "88", fileUid: "1059000088" }, clicks: [] },
  { name: "server preference cannot reuse authorization for another UID", body: modalListing(
    modalRow("88", "1059000088", { name: "Bundle_SERVER_P" }), modalRow("99", "1059000099", { name: "Bundle" })),
    modal: true, authorize: { fileId: "88", fileUid: "1059000099" }, clicks: [] },
  { name: "two server variants still require an explicit file", body: modalListing(
    modalRow("88", "1059000088", { name: "Bundle_Server_HD" }), modalRow("99", "1059000099", { name: "Bundle_Server_Lite" })),
    modal: true, clicks: [], state: "blocked", reason: "file_selection" },
  { name: "the local Windows server selects the sole explicit Windows server beside Linux and client files",
    body: modalListing(modalRow("88", "1059000088", { name: "NPCMechanics_PakOnly_WindowsServer_v1.1.2" }),
      modalRow("99", "1059000099", { name: "NPCMechanics_PakOnly_LinuxServer_v1.1.2" }),
      modalRow("77", "1059000077", { name: "NPCCommunityFix_Client_v1.0.8" })),
    modal: true, selection: { fileId: "88", fileUid: "1059000088" }, clicks: ["modal-88"] },
  { name: "legacy Windows server selection uses its bound name rather than file ordering",
    body: modalListing(legacyRow("88", "Bundle_Linux_Server"), legacyRow("99", "Bundle_Windows_Server")),
    clicks: ["manual-99"] },
  { name: "Windows and unspecified server variants remain an explicit choice", body: modalListing(
    modalRow("88", "1059000088", { name: "Bundle_Windows_Server" }), modalRow("99", "1059000099", { name: "Bundle_Server" })),
    modal: true, clicks: [], state: "blocked", reason: "file_selection" },
  { name: "two Windows server variants cannot be selected by version or ID", body: modalListing(
    modalRow("88", "1059000088", { name: "Bundle_Windows_Server_Lite" }),
    modalRow("99", "1059000099", { name: "Bundle_Windows_Server_HD" }),
    modalRow("77", "1059000077", { name: "Bundle_Linux_Server" })),
    modal: true, clicks: [], state: "blocked", reason: "file_selection" },
  { name: "a sole explicit Linux server does not become an automatic Windows download", body: modalListing(
    modalRow("88", "1059000088", { name: "Bundle_Linux_Server" })),
    modal: true, clicks: [], state: "blocked", reason: "file_selection" },
  { name: "an explicit Linux file keeps precedence over automatic platform selection", fileId: "99", body: modalListing(
    modalRow("88", "1059000088", { name: "Bundle_Windows_Server" }), modalRow("99", "1059000099", { name: "Bundle_Linux_Server" })),
    modal: true, selection: { fileId: "99", fileUid: "1059000099" }, clicks: ["modal-99"] },
  ...["Client And Server", "Server_Client_P", "Both-Client-Server", "Not for server", "Non-server", "No server",
    "Without server", "Server not supported", "Server unsupported", "Server disabled", "ObserverPack", "Serverless",
    "ClientOnly_Server", "Server " + "x".repeat(512), { name: "Server" }].map((name, index) => ({
    name: `mixed, negated or unbounded server hint stays ambiguous ${index}`,
    body: modalListing(modalRow("88", "1059000088", { name }), modalRow("99", "1059000099", { name: "Bundle" })),
    modal: true, clicks: [], state: "blocked", reason: "file_selection",
  })),
  { name: "author prose cannot provide a server file name", body: '<article><h1>Server-only</h1></article>' + modalListing(
    modalRow().replace('<div class="tabbed-block">', '<p>Server-only</p><div class="tabbed-block">'), modalRow("99", "1059000099")),
    modal: true, clicks: [], state: "blocked", reason: "file_selection" },
  { name: "dependency dialog title cannot identify a server file", body: modalListing(modalRow(), modalRow("99", "1059000099"))
    + '<div role="dialog"><h2>Download mod file</h2><p>Server-only</p></div>', modal: true,
    clicks: [], state: "blocked", reason: "website_dialog" },
  { name: "explicit client file takes precedence over a server hint", body: modalListing(
    modalRow("88", "1059000088", { name: "Bundle_Server_P" }), modalRow("99", "1059000099", { name: "Bundle_Client_P" })),
    modal: true, fileId: "99", selection: { fileId: "99", fileUid: "1059000099" }, clicks: ["modal-99"] },
  { name: "server hint cannot hide a quarantined Main variant with unknown name", body: modalListing(
    modalRow("88", "1059000088", { name: "Bundle_Server_P" }), quarantineRow()), modal: true,
    clicks: [], state: "blocked", reason: "file_selection" },
  { name: "PLAP quarantined main file reports the site's actual blocker", body: modalListing(quarantineRow()),
    clicks: [], state: "blocked", reason: "file_quarantined" },
  { name: "explicit quarantined file reports its actual blocker", fileId: "1948", body: modalListing(quarantineRow(), modalRow()), modal: true,
    clicks: [], state: "blocked", reason: "file_quarantined" },
  { name: "another quarantined main file cannot silently change the selection", body: modalListing(modalRow(), quarantineRow()), modal: true,
    clicks: [], state: "blocked", reason: "file_selection" },
  { name: "collapsed quarantined main file still counts as a choice", body: modalListing(modalRow(), quarantineRow("1948", { rowExtra: 'style="display:none"' })), modal: true,
    clicks: [], state: "blocked", reason: "file_selection" },
  { name: "explicit downloadable file ignores another quarantined file", fileId: "88", body: modalListing(quarantineRow(), modalRow()), modal: true,
    selection: { fileId: "88", fileUid: "1059000088" }, clicks: ["modal-88"] },
  { name: "quarantined optional file does not block the main file", body: modalListing(modalRow())
    .replace(/<\/div>$/, '<section id="file-container-optional-files"><h2>Optional files</h2>' + quarantineRow() + '</section></div>'), modal: true,
    selection: { fileId: "88", fileUid: "1059000088" }, clicks: ["modal-88"] },
  ...[
    ["quarantine header must match its file", { headerId: "88" }],
    ["hidden quarantine notice is not an actual blocker", { extra: "hidden" }],
    ["quarantine class without the site's notice is insufficient", { notice: "Author description" }],
  ].map(([name, options]) => ({ name, body: modalListing(quarantineRow("1948", options)), clicks: [], afterWait: "blocked" })),
  { name: "author quoted quarantine is excluded", body: '<div id="description">' + modalListing(quarantineRow()) + '</div>',
    clicks: [], afterWait: "blocked" },
  { name: "another file's quarantine cannot explain a missing specified file", fileId: "99", body: modalListing(quarantineRow()),
    clicks: [], afterWait: "blocked" },
  { name: "quarantine notice on a different project is excluded", url: project.replace("2264", "999"), body: modalListing(quarantineRow()),
    clicks: [], state: "blocked", reason: "project_identity" },
  { name: "PLAP new file row waits for native authorization before Manual", body: modalListing(modalRow()), modal: true,
    selection: { fileId: "88", fileUid: "1059000088" }, clicks: ["modal-88"] },
  { name: "new file row can use a normal site anchor", body: modalListing(modalRow()), modal: true, modalAnchor: true,
    selection: { fileId: "88", fileUid: "1059000088" }, clicks: ["modal-88"] },
  { name: "new file cannot click without authorization", body: modalListing(modalRow()), modal: true, authorize: false, clicks: [] },
  { name: "native authorization for another UID cannot be reused", body: modalListing(modalRow()), modal: true,
    authorize: { fileId: "88", fileUid: "1059000099" }, clicks: [] },
  { name: "two new main files cannot be guessed", body: modalListing(modalRow(), modalRow("99", "1059000099")), modal: true,
    clicks: [], state: "blocked", reason: "file_selection" },
  { name: "collapsed new main file still makes selection ambiguous", body: modalListing(modalRow(), modalRow("99", "1059000099", { rowExtra: 'style="display:none"' })), modal: true,
    clicks: [], state: "blocked", reason: "file_selection" },
  { name: "explicit new file ignores another main file", body: modalListing(modalRow(), modalRow("99", "1059000099")), modal: true, fileId: "99",
    selection: { fileId: "99", fileUid: "1059000099" }, clicks: ["modal-99"] },
  ...[
    ["new row header must agree", { headerId: "99" }],
    ["new component cannot be hidden", { extra: "hidden" }],
    ["new component URL must match its UID", { href: "/api/files/1059000099/download" }],
    ["new component URL cannot leave Nexus", { href: "https://outside.invalid/api/files/1059000088/download" }],
    ["new component URL cannot have private query", { href: "/api/files/1059000088/download?token=fixture-client-secret" }],
  ].map(([name, options]) => ({ name, body: modalListing(modalRow("88", "1059000088", options)), modal: true, clicks: [], afterWait: "blocked" })),
  { name: "new component in author content is excluded", body: '<div id="description">' + modalListing(modalRow()) + '</div>', modal: true,
    clicks: [], afterWait: "blocked" },
  { name: "unhydrated new component does not manufacture a download", body: modalListing(modalRow()),
    clicks: [], afterWait: "blocked" },
  { name: "sole main ignores optional and old files", body: main(manual("88")) + "<h2>Optional files</h2>" + manual("99")
    + "<h2>Old files</h2>" + manual("77"), clicks: ["manual-88"] },
  { name: "two main files require an explicit file link", body: main(manual("88"), manual("99")), clicks: [], state: "blocked", reason: "file_selection" },
  { name: "duplicate links are one file", body: main(manual("88"), manual("88")), clicks: ["manual-88"] },
  { name: "collapsed main row uses its own ordinary link", body: main(manual("88", 'style="display:none"')), clicks: ["manual-88"] },
  { name: "specified optional file wins", fileId: "99", body: main(manual("88")) + "<h2>Optional files</h2>" + manual("99"), clicks: ["manual-99"] },
  { name: "no specified file cannot choose another", fileId: "99", body: main(manual("88")), clicks: [], afterWait: "blocked" },
  { name: "no main file cannot choose an optional one", body: "<h2>Optional files</h2>" + manual("99"), clicks: [], afterWait: "blocked" },
  { name: "author description is not the file listing", body: '<div id="description">' + main(manual("99")) + "</div>" + main(manual("88")), clicks: ["manual-88"] },
  { name: "author-only listing is ignored", body: '<div class="wysiwyg">' + main(manual("99")) + "</div>", clicks: [], afterWait: "blocked" },
  { name: "other project is ignored", body: main(manual("88", "", project.replace("2264", "999") + "?file_id=88")), clicks: [], afterWait: "blocked" },
  { name: "other game is ignored", body: main(manual("88", "", project.replace("7daystodie", "valheim") + "?file_id=88")), clicks: [], afterWait: "blocked" },
  { name: "untrusted destination is ignored", body: main(manual("88", "", "https://untrusted.invalid/file?file_id=88")), clicks: [], afterWait: "blocked" },
  { name: "ambiguous query is ignored", body: main(manual("88", "", `${project}?file_id=88&file_id=99`)), clicks: [], afterWait: "blocked" },
  { name: "modern file URL", body: main(manual("88", "", project.replace("/7daystodie", "/games/7daystodie") + "/files/88")), clicks: ["manual-88"] },
  { name: "IDs stay exact", body: main(manual("18446744073709551615")), clicks: ["manual-18446744073709551615"] },
  { name: "pending title does not block canonical file selection", pageState: "pending", body: main(manual("88")), clicks: ["manual-88"] },
  { name: "resolved project identity required", canonical: null, body: main(manual("88")), clicks: [], afterWait: "blocked" },
  // The real 2264 files page has a project heading and Manual link, but no
  // canonical tag. Exercise both production scripts instead of supplying metadata.
  { name: "real files structure without canonical reaches manual", extractMetadata: true,
    body: '<main><h1>Mod Manager</h1>' + main(manual("19781")) + '</main>', clicks: ["manual-19781"] },
  { name: "no canonical with conflicting social identity cannot click", extractMetadata: true,
    head: '<meta property="og:url" content="' + project.replace("2264", "999") + '">',
    body: '<main><h1>Mod Manager</h1>' + main(manual("19781")) + '</main>', clicks: [], state: "blocked", reason: "project_identity" },
  { name: "wrong canonical cannot click", canonical: project.replace("2264", "999"), body: main(manual("88")), clicks: [], afterWait: "blocked" },
  { name: "invalid metadata cannot click", pageState: "invalid", body: main(manual("88")), clicks: [], state: "blocked", reason: "project_identity" },
  { name: "automatic verification waits without clicking", pageState: "challenge", body: '<h1>Verify you are human</h1><button data-action="captcha">Verify</button>', clicks: [], phase: "verifying" },
  { name: "login is a background blocker", url: "https://users.nexusmods.com/auth/sign_in?redirect=fixture-client-secret", body: slow() + '<input type="password">', clicks: [], state: "blocked", reason: "login" },
  { name: "blank rendering cannot click", rendered: false, body: main(manual("88")), clicks: [], state: "loading" },
  { name: "selected file clicks slow once", url: project + "?file_id=88&tab=files", body: slow(), clicks: ["slow"], phase: "waiting" },
  { name: "PLAP download component reaches its ordinary shadow button", url: project + "?file_id=88", body: downloadComponent(), shadow: slow(), clicks: ["slow"], phase: "waiting" },
  { name: "bare game domain remains valid", url: project + "?file_id=88", body: downloadComponent("88", "", "7daystodie"), shadow: slow(), clicks: ["slow"], phase: "waiting" },
  { name: "display contents host still exposes its visible download button", url: project + "?file_id=88", body: downloadComponent("88", 'style="display:contents"'), shadow: slow(), clicks: ["slow"], phase: "waiting" },
  { name: "different component file cannot be selected", url: project + "?file_id=88", body: downloadComponent("99"), shadow: slow(), clicks: [], afterWait: "blocked" },
  { name: "different component game cannot be selected", url: project + "?file_id=88", body: downloadComponent("88", "", "/valheim"), shadow: slow(), clicks: [], afterWait: "blocked" },
  { name: "network-path game domain cannot be selected", url: project + "?file_id=88", body: downloadComponent("88", "", "//7daystodie"), shadow: slow(), clicks: [], afterWait: "blocked" },
  { name: "game domain with extra path cannot be selected", url: project + "?file_id=88", body: downloadComponent("88", "", "/7daystodie/other"), shadow: slow(), clicks: [], afterWait: "blocked" },
  { name: "author content cannot supply a shadow download control", url: project + "?file_id=88", body: '<div id="description">' + downloadComponent() + '</div>', shadow: slow(), clicks: [], afterWait: "blocked" },
  { name: "hidden component cannot download", url: project + "?file_id=88", body: downloadComponent("88", "hidden"), shadow: slow(), clicks: [], afterWait: "blocked" },
  { name: "hidden component ancestor cannot download", url: project + "?file_id=88", body: '<div style="display:none">' + downloadComponent() + '</div>', shadow: slow(), clicks: [], afterWait: "blocked" },
  { name: "disabled shadow button keeps waiting", url: project + "?file_id=88", body: downloadComponent(), shadow: slow("disabled"), clicks: [], afterWait: "blocked" },
  { name: "unloaded component keeps waiting", url: project + "?file_id=88", body: downloadComponent(), clicks: [], afterWait: "blocked" },
  { name: "component login gate is specific", url: project + "?file_id=88", body: downloadComponent().replace('user-is-logged-in="true"', 'user-is-logged-in="false"'), shadow: '<a href="https://users.nexusmods.com/auth/sign_in" data-action="login">Slow download</a>', clicks: [], state: "blocked", reason: "login" },
  { name: "shadow dialog requires user action", url: project + "?file_id=88", body: downloadComponent(), shadow: '<div role="dialog">' + slow() + '</div>', clicks: [], state: "blocked", reason: "website_dialog" },
  { name: "official shadow portal dialog requires user action", url: project + "?file_id=88", body: downloadComponent(), shadow: slow(), portal: '<div role="dialog"><button data-action="consent">Continue</button></div>', clicks: [], state: "blocked", reason: "website_dialog" },
  { name: "selected file slow anchor uses its own site handler", url: project + "?file_id=88", body: '<a href="#download" data-action="slow">Slow Download</a>', clicks: ["slow"], phase: "waiting" },
  { name: "external slow link is ignored", url: project + "?file_id=88", body: '<a href="https://untrusted.invalid/download" data-action="wrong">Slow Download</a>', clicks: [], afterWait: "blocked" },
  { name: "bare CDN slow link is not tied to selected file", url: project + "?file_id=88", body: '<a href="https://cf-files.nexusmods.com/other-file.zip" data-action="wrong">Slow Download</a>', clicks: [], afterWait: "blocked" },
  { name: "slow link cannot change the selected file", url: project + "?file_id=88", body: '<a href="?file_id=99" data-action="wrong">Slow Download</a>', clicks: [], afterWait: "blocked" },
  { name: "unknown dialog is a background blocker", url: project + "?file_id=88", body: '<div role="dialog">' + slow() + '</div>', clicks: [], state: "blocked", reason: "website_dialog" },
  { name: "mismatched selected file cannot click", url: project + "?file_id=88", fileId: "99", body: slow(), clicks: [], state: "blocked", reason: "file_selection" },
  { name: "slow requires selected file", body: slow(), clicks: [], afterWait: "blocked" },
  { name: "disabled slow waits", url: project + "?file_id=88", body: slow("disabled"), clicks: [], afterWait: "blocked" },
  { name: "hidden slow cannot click", url: project + "?file_id=88", body: slow('style="display:none"'), clicks: [], afterWait: "blocked" },
  { name: "invisible parent cannot click", url: project + "?file_id=88", body: '<div style="opacity:0">' + slow() + "</div>", clicks: [], afterWait: "blocked" },
  { name: "two slow controls cannot be guessed", url: project + "?file_id=88", body: slow() + slow(), clicks: [], state: "blocked", reason: "file_selection" },
  { name: "does not choose premium", url: project + "?file_id=88", body: '<button data-action="premium">Fast Download</button>', clicks: [], afterWait: "blocked" },
  { name: "guest countdown is not a download", url: project + "?file_id=88", body: "<p>You must be logged in.</p><p>Your download will start in 5 seconds...</p>", clicks: [], state: "blocked", reason: "login" },
  { name: "download login requirement reports the actual blocker", url: project + "?file_id=88", body: "<p>You must be logged in to download files.</p>", clicks: [], state: "blocked", reason: "login" },
  { name: "PLAP files-page login wall reports login before selecting a file", extractMetadata: true,
    body: '<main><h1>Public project</h1><div class="tabcontent tabcontent-mod-page" role="status">' + guestBanner + main() + '</div></main>',
    clicks: [], state: "blocked", reason: "login" },
  { name: "download gate takes precedence over an available manual link", body: guestBanner + main(manual("88")),
    clicks: [], state: "blocked", reason: "login" },
  { name: "selected-file page also recognizes have-to login wording", url: project + "?file_id=88", body: guestBanner,
    clicks: [], state: "blocked", reason: "login" },
  { name: "hidden logged-out banner does not block manual download", body: '<div hidden>' + guestBanner + '</div>' + main(manual("88")), clicks: ["manual-88"] },
  { name: "author quoted download gate does not block manual download", body: '<div id="description">' + guestBanner + '</div>' + main(manual("88")), clicks: ["manual-88"] },
  { name: "header sign-in and generic account notice are not a file-list download gate", body: '<header><a href="https://users.nexusmods.com/auth/sign_in">Log in</a></header><p>You must be logged in.</p>' + main(manual("88")), clicks: ["manual-88"] },
  { name: "official inline login link stays a background blocker", url: project + "?file_id=88", body: '<p>You must be logged in to download files. <a href="https://users.nexusmods.com/auth/sign_in" data-action="login">Log in here.</a></p>', clicks: [], state: "blocked", reason: "login" },
  { name: "download table login text does not depend on paragraph markup", extractMetadata: true, url: project + "?file_id=19781",
    body: '<main><h1>Mod Manager</h1><table><tbody><tr><td colspan="2">You must be logged in to download files. <a href="https://users.nexusmods.com/auth/sign_in" data-action="login">Log in here</a>.</td></tr></tbody></table><p>Your download will start in 5 seconds...</p></main>',
    clicks: [], state: "blocked", reason: "login" },
  { name: "plain span login text remains a specific blocker", url: project + "?file_id=88",
    body: '<div><span>\nYou must\n  be logged in to download files.\n</span></div>', clicks: [], state: "blocked", reason: "login" },
  { name: "hidden login text in a visible cell cannot block download", url: project + "?file_id=88",
    body: '<table><tr><td style="padding:20px"><span hidden>You must be logged in to download files.</span></td></tr></table>' + slow(), clicks: ["slow"], phase: "waiting" },
  { name: "author wrapper cannot turn quoted login text into a blocker", url: project + "?file_id=88",
    body: '<div><section id="description"><div>You must be logged in to download files.</div></section></div>' + slow(), clicks: ["slow"], phase: "waiting" },
  { name: "hidden login notice does not block a signed-in download", url: project + "?file_id=88", body: '<p hidden>You must be logged in to download files.</p>' + slow(), clicks: ["slow"], phase: "waiting" },
  { name: "author login prose is not an authentication requirement", url: project + "?file_id=88", body: '<div id="description"><p>You must be logged in.</p></div>' + slow(), clicks: ["slow"], phase: "waiting" },
  { name: "does not dismiss a website dialog", body: main(manual("88")) + '<div role="dialog"><button data-action="consent">Accept cookies</button></div>', clicks: [], state: "blocked", reason: "website_dialog" },
  { name: "disabled manual cannot click", body: main(manual("88", 'aria-disabled="true"')), clicks: [], afterWait: "blocked" },
];

function fixtureHtml() {
  const json = (value) => JSON.stringify(value).replaceAll("<", "\\u003c");
  return `<!doctype html><body><script>
  (async () => {
    const nonce = new URL(location.href).searchParams.get("nonce"), errors = [], results = [];
    addEventListener("error", event => errors.push(event.message));
    try {
      const script = ${json(automatic)}, metadataScript = ${json(extractor)}, controlsScript = ${json(fileControls)}, base = ${json(project)};
      async function create(body, url = base) {
        const frame = document.createElement("iframe");
        frame.style.cssText = "width:1000px;height:700px";
        const loaded = new Promise(resolve => frame.onload = resolve);
        frame.srcdoc = "<!doctype html><body>" + body;
        document.body.append(frame); await loaded;
        const fixtureDocument = frame.contentDocument, state = { url: new URL(url), now: 100, clicks: [], authorized: null };
        Object.defineProperty(fixtureDocument, "cookie", { get() { throw new Error("Automation read cookies"); } });
        frame.contentWindow.fetch = () => { throw new Error("Automation requested an endpoint"); };
        fixtureDocument.addEventListener("click", event => { event.preventDefault(); state.clicks.push(event.composedPath()[0].dataset.action); });
        const run = (sample = {}) => {
          const target = {gameDomain:sample.gameDomain ?? "7daystodie", modId:sample.modId ?? "2264", fileId:sample.fileId ?? null};
          return new frame.contentWindow.Function("location", "performance", "return " + script)(state.url, {now: () => state.now})(target,
          sample.extractMetadata ? new frame.contentWindow.Function("location", "return " + metadataScript)(state.url)
            : {state:sample.pageState ?? "ready", canonicalUrl:sample.canonical === undefined ? base : sample.canonical},
          {hasContent:sample.rendered !== false}, state.authorized,
          new frame.contentWindow.Function("location", "return " + controlsScript)(state.url)(target));
        };
        return {frame, document: fixtureDocument, state, run};
      }
      for (const sample of ${json(samples)}) {
        const f = await create(sample.body, sample.url);
        if (sample.head) f.document.head.innerHTML = sample.head;
        if (sample.shadow) f.document.querySelector("mod-file-download").attachShadow({mode:"open"}).innerHTML = sample.shadow;
        if (sample.modal) for (const host of f.document.querySelectorAll('mod-download-modal')) {
          const file = JSON.parse(host.getAttribute('file')), fileId = host.closest('dd').getAttribute('data-id');
          host.attachShadow({mode:'open'}).innerHTML = sample.modalAnchor
            ? '<a href="' + file.downloadUrl + '" data-action="modal-' + fileId + '">Manual download</a>'
            : '<button data-action="modal-' + fileId + '">Manual download</button>';
        }
        if (sample.portal) {
          const portal = f.document.createElement("div"); portal.id = "next-shadow-root"; f.document.body.append(portal);
          portal.attachShadow({mode:"open"}).innerHTML = sample.portal;
        }
        const first = f.run(sample), beforeAuthorization = [...f.state.clicks];
        if (first.fileSelection && sample.authorize !== false) f.state.authorized = sample.authorize || first.fileSelection;
        const second = f.run(sample);
        f.state.now += 8100; const after = f.run(sample);
        results.push({name:sample.name, first, second, after, clicks:f.state.clicks, beforeAuthorization});
        f.frame.remove();
      }
      // Complete the site's two real click handlers, without replacing the
      // automation function or bypassing its state between DOM changes.
      const chain = await create(${json(main(manual("88")))});
      chain.document.addEventListener("click", event => {
        const control = event.composedPath()[0];
        if (control.dataset.action === "manual-88") {
          chain.state.url = new URL(base + "?file_id=88&tab=files");
          chain.document.body.innerHTML = ${json(downloadComponent())};
          chain.document.querySelector("mod-file-download").attachShadow({mode:"open"}).innerHTML = ${json(slow())};
        } else if (control.dataset.action === "slow") {
          control.disabled = true;
          chain.document.body.insertAdjacentHTML("beforeend", "<p>Your download will start in 5 seconds...</p>");
        }
      });
      const chainStates = [chain.run(), chain.run(), chain.run()];
      chain.state.now += 12000; chainStates.push(chain.run());
      chain.document.querySelector("mod-file-download").shadowRoot.querySelector("button").remove(); chainStates.push(chain.run());
      chain.state.now += 23000; const after35Seconds = chain.run();
      chain.state.now += 55001; const stalled = chain.run();
      const chainResult = {clicks:chain.state.clicks, states:chainStates, after35Seconds, stalled};
      chain.frame.remove();
      // Manual navigation can also take longer than thirty seconds. Keep the
      // original site control clicked once while the native operation owns its budget.
      const manualWait = await create(${json(main(manual("88")))});
      const manualStarted = manualWait.run();
      manualWait.state.now += 35000; const manualAfter35Seconds = manualWait.run();
      manualWait.state.now += 55001; const manualStalled = manualWait.run();
      const manualWaitResult = {started:manualStarted, after35Seconds:manualAfter35Seconds,
        stalled:manualStalled, clicks:manualWait.state.clicks};
      manualWait.frame.remove();
      const modalChain = await create(${json(modalListing(modalRow()))});
      modalChain.document.querySelector('mod-download-modal').attachShadow({mode:'open'}).innerHTML = '<button data-action="modal-88">Manual download</button>';
      modalChain.document.addEventListener('click', event => {
        const control = event.composedPath()[0];
        if(control.dataset.action === 'modal-88') {
          const portal=modalChain.document.createElement('div');portal.id='next-shadow-root';modalChain.document.body.append(portal);
          portal.attachShadow({mode:'open'}).innerHTML = '<div role="dialog"><h2>Download mod file</h2><a data-action="confirm-88" href="/api/files/1059000088/download">Manual download</a><a data-action="dependency" href="/api/files/1059000099/download">Download</a></div>';
        } else if(control.dataset.action === 'confirm-88') {
          modalChain.state.url = new URL(base + '?tab=files&file_id=88');
          modalChain.document.body.innerHTML = ${json(downloadComponent())};
          modalChain.document.querySelector('mod-file-download').attachShadow({mode:'open'}).innerHTML = ${json(slow())};
        } else if(control.dataset.action === 'slow') control.disabled=true;
      });
      const requestSelection=modalChain.run(), unarmedClicks=[...modalChain.state.clicks];
      modalChain.state.authorized=requestSelection.fileSelection;
      const modalStates=[modalChain.run(),modalChain.run(),modalChain.run()];
      const modalChainResult={requestSelection,unarmedClicks,clicks:modalChain.state.clicks,states:modalStates};
      modalChain.frame.remove();
      const resume = await create("<h1>Security verification</h1><button data-action='captcha'>Verify</button>");
      const verifying = resume.run({pageState:"challenge", canonical:null});
      resume.state.now += 25000;
      const stillVerifying = resume.run({pageState:"challenge", canonical:null});
      resume.document.body.innerHTML = ${json(main(manual("88")))};
      const continued = resume.run();
      const resumeResult = {verifying, stillVerifying, continued, clicks:resume.state.clicks}; resume.frame.remove();
      const blocked = await create("<h1>Security verification</h1><button data-action='captcha'>Verify</button>");
      const started = blocked.run({pageState:"challenge", canonical:null});
      blocked.state.now += 30001;
      const ended = blocked.run({pageState:"challenge", canonical:null});
      const challengeResult = {started, ended, clicks:blocked.state.clicks}; blocked.frame.remove();
      await fetch("/__reliability_result/" + nonce, {method:"POST", headers:{"Content-Type":"application/json"},
        body:JSON.stringify({status:"passed",results,chain:chainResult,manualWait:manualWaitResult,modalChain:modalChainResult,resume:resumeResult,challenge:challengeResult,browser_errors:errors})});
    } catch (error) {
      await fetch("/__reliability_result/" + nonce, {method:"POST",headers:{"Content-Type":"application/json"},
        body:JSON.stringify({status:"failed",error:String(error)})});
    }
  })();</script>`;
}

test("Nexus ordinary website clicks choose unambiguous files and report background progress or blockers", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ reactTransform: false, fixtureMiddleware(request, response, next) {
    if (request.url?.split("?")[0] !== "/tests/helpers/runtime-browser.html") return next();
    response.writeHead(200, {"Content-Type":"text/html; charset=utf-8"}).end(fixtureHtml());
  } });
  assert.equal(report.status, "passed", report.error);
  for (const sample of samples) {
    const result = report.results.find((item) => item.name === sample.name);
    assert.equal(result.first.state, sample.state ?? "working", sample.name);
    assert.equal(result.second.state, sample.state ?? "working", sample.name);
    assert.equal(result.after.state, sample.afterWait ?? sample.state ?? "working", sample.name);
    if (sample.reason) assert.equal(result.first.reason, sample.reason, sample.name + ": specific blocker");
    for (const step of [result.first, result.second, result.after]) {
      assert.equal(step.phase, step.state === "working" ? sample.phase ?? "selecting" : undefined, sample.name + ": truthful progress");
      assert.doesNotMatch(JSON.stringify(step), /CraftingRecipes|Bundle_Server|Server-only/, sample.name + ": file names stay inside DOM selection");
    }
    assert.deepEqual(result.clicks, sample.clicks, sample.name + ": clicks once, never chooses another file");
    if (sample.selection) {
      assert.deepEqual(result.first.fileSelection, sample.selection, sample.name + ": public file identity");
      assert.deepEqual(result.beforeAuthorization, [], sample.name + ": native binds the navigation before clicking");
    }
  }
  assert.deepEqual(report.chain.clicks, ["manual-88", "slow"]);
  assert.deepEqual(report.chain.states.map((state) => state.phase), ["selecting", "waiting", "waiting", "waiting", "waiting"]);
  assert.deepEqual(report.chain.after35Seconds, {state:"working", phase:"waiting"});
  assert.equal(report.chain.stalled.state, "blocked");
  assert.equal(report.chain.stalled.reason, "download_not_started");
  assert.deepEqual(report.manualWait.started, {state:"working", phase:"selecting"});
  assert.deepEqual(report.manualWait.after35Seconds, {state:"working", phase:"selecting"});
  assert.deepEqual(report.manualWait.stalled, {state:"blocked", reason:"download_not_started"});
  assert.deepEqual(report.manualWait.clicks, ["manual-88"]);
  assert.deepEqual(report.modalChain.unarmedClicks, []);
  assert.deepEqual(report.modalChain.clicks, ["modal-88", "confirm-88", "slow"]);
  assert.deepEqual(report.modalChain.states.map(state=>state.phase), ["selecting", "selecting", "waiting"]);
  assert.equal(report.resume.verifying.phase, "verifying");
  assert.equal(report.resume.stillVerifying.phase, "verifying");
  assert.equal(report.resume.continued.phase, "selecting");
  assert.deepEqual(report.resume.clicks, ["manual-88"]);
  assert.equal(report.challenge.started.phase, "verifying");
  assert.deepEqual(report.challenge.ended, {state:"blocked", reason:"verification"});
  assert.deepEqual(report.challenge.clicks, []);
  assert.doesNotMatch(JSON.stringify(report.results), /fixture-client-secret|cf-files|https:/);
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.browser_exited, true);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
});
