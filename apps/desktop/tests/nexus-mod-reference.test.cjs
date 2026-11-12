const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
const filename = path.resolve(__dirname, "../src/views/servers/nexus-mod-reference.ts");
const source = { exports: {} };
vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), { module: source, exports: source.exports });
const parse = (formats) => Array.from(source.exports.nexusReferencesFromDrop({ getData: (type) => formats[type] ?? "" }));
const project = "https://www.nexusmods.com/7daystodie/mods/13899?tab=files&file_id=28123";
const thumbnail = "https://staticdelivery.nexusmods.com/mods/1059/images/13899/13899-thumbnail.jpg";

test("Nexus card drops recover the project link when URI/plain expose only its thumbnail", () => {
  assert.deepEqual(parse({ "text/uri-list": thumbnail, "text/plain": thumbnail,
    "text/html": `<a href="${project.replaceAll("&", "&amp;")}"><img src="${thumbnail}"></a>` }), [project]);
});

test("Nexus URI selections exclude images and comment numbers without becoming multiple projects", () => {
  assert.deepEqual(parse({ "text/uri-list": `# 13899\r\n${project}\r\n${thumbnail}` }), [project]);
});

test("Nexus project URI wins over a different HTML representation", () => {
  assert.deepEqual(parse({ "text/uri-list": project, "text/html": '<a href="https://www.nexusmods.com/7daystodie/mods/870">Other</a>' }), [project]);
});

test("Nexus HTML project links take precedence over numeric card captions", () => {
  assert.deepEqual(parse({ "text/plain": "Version 1 2 3", "text/html": `<a href='${project}'>Version 1 2 3</a>` }), [project]);
  assert.deepEqual(parse({ "text/plain": "13899" }), ["13899"]);
});

test("Nexus old and new project paths work across the four supported games", () => {
  for (const domain of ["7daystodie", "abioticfactor", "enshrouded", "humanitz"]) {
    for (const prefix of ["", "games/"]) {
      const reference = `https://www.nexusmods.com/${prefix}${domain}/mods/123/files/456`;
      assert.deepEqual(parse({ "text/uri-list": `${thumbnail}\n${reference}` }), [reference]);
    }
  }
});

test("Nexus signed references retain case and encoded query bytes during drop selection", () => {
  const signed = "nxm://7daystodie/mods/123/files/456?key=aB%2FC&expires=123&user_id=789";
  assert.deepEqual(parse({ "text/uri-list": `${signed}\n${thumbnail}` }), [signed]);
});

test("Nexus true multiple projects remain multiple and image-only drops invent no project", () => {
  const second = "https://www.nexusmods.com/7daystodie/mods/870";
  assert.deepEqual(parse({ "text/uri-list": `${project}\n${second}\n${thumbnail}` }), [project, second]);
  assert.deepEqual(parse({ "text/uri-list": thumbnail }), [thumbnail]);
});
