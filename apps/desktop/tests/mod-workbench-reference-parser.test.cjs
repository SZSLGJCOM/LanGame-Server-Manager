const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const root = path.resolve(__dirname, "..", "..", "..");
const desktopRoot = path.join(root, "apps", "desktop");

require.extensions[".ts"] = function compileTypeScript(module, filename) {
  const source = fs.readFileSync(filename, "utf8");
  const outputText = transpileTypeScript(source, filename);
  module._compile(outputText, filename);
};

const { referenceCandidatesFromText } = require(path.join(
  desktopRoot,
  "src",
  "views",
  "servers",
  "mod-reference-parser.ts"
));

test("mod workbench keeps URLs, numeric ids, and Modrinth shortcuts from pasted text", () => {
  assert.deepEqual(
    referenceCandidatesFromText(`
      <a href="https://modrinth.com/mod/fabric-api">Fabric API</a>
      mr:sodium,
      modrinth:lithium;
      1346144
      not-a-reference
    `),
    [
      "https://modrinth.com/mod/fabric-api",
      "mr:sodium",
      "modrinth:lithium",
      "1346144"
    ]
  );
});

test("mod workbench unwraps common redirect links from copied web markup", () => {
  assert.deepEqual(
    referenceCandidatesFromText(`
      <a href="https://steamcommunity.com/linkfilter/?u=https%3A%2F%2Fwww.curseforge.com%2Fark-survival-ascended%2Fmods%2Fawesome-spyglass">
        CurseForge
      </a>
      <a href="https://www.google.com/url?q=https%3A%2F%2Fmodrinth.com%2Fmod%2Fsodium&amp;sa=D">
        Modrinth
      </a>
      <a href="https://example.com/redirect?target=https%3A%2F%2Fwww.nexusmods.com%2F7daystodie%2Fmods%2F1234">
        Nexus
      </a>
    `),
    [
      "https://www.curseforge.com/ark-survival-ascended/mods/awesome-spyglass",
      "https://modrinth.com/mod/sodium",
      "https://www.nexusmods.com/7daystodie/mods/1234"
    ]
  );
});

test("Thunderstore Mod Manager buttons become pinned package references without launching a protocol", () => {
  for (const [owner, name, version] of [
    ["ValheimModding", "Jotunn", "2.30.2"],
    ["deca", "VampireCommandFramework", "0.11.0"],
    ["BepInEx", "BepInExPack_V_Rising", "1.733.2"]
  ]) {
    const protocol = `ror2mm://v1/install/thunderstore.io/${owner}/${name}/${version}/`;
    const canonical = `https://thunderstore.io/package/${owner}/${name}/${version}/`;
    assert.deepEqual(referenceCandidatesFromText(`${protocol}\n<a href="${protocol}">Install with Mod Manager</a>`), [canonical]);
  }
  for (const invalid of [
    "ror2mm://v1/install/evil.example/Author/Plugin/1.0.0/",
    "ror2mm://v1/install/thunderstore.io/Author/Plugin/1.0.0/?url=https://evil.example",
    "ror2mm://v1/install/thunderstore.io/Author/Plugin/not-a-version/",
    "ror2mm://v1/install/thunderstore.io/Author/Plugin/4294967296.0.0/",
    "ror2mm://user@v1/install/thunderstore.io/Author/Plugin/1.0.0/",
    "ror2mm://v2/install/thunderstore.io/Author/Plugin/1.0.0/"
  ]) assert.deepEqual(referenceCandidatesFromText(invalid), []);
});
