const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const test = require("node:test");
const { loadCatalog, loadEffectiveCatalogs } = require("../scripts/i18n-catalog-loader.cjs");

test("catalog loading follows imported computed keys and the actual spread order", (t) => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "lgsm-i18n-catalog-"));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.writeFileSync(path.join(root, "labels.ts"), `
    export const labels: Record<string, string> = Object.fromEntries(
      ["world", "network"].map((name) => [
        \`settings.\${name}.title\`, \`Computed \${name}\`
      ])
    );
  `);
  const filename = path.join(root, "catalog.ts");
  fs.writeFileSync(filename, `
    import { labels } from "./labels";
    export const messages = {
      "settings.world.title": "Earlier value",
      ...labels,
      "settings.network.title": "Final value"
    };
  `);
  const originalHook = require.extensions[".ts"];
  const catalog = loadCatalog(filename, "messages");
  assert.deepEqual([...catalog], [
    ["settings.world.title", "Computed world"],
    ["settings.network.title", "Final value"]
  ]);
  assert.equal(require.extensions[".ts"], originalHook);
  catalog.clear();
  assert.equal(loadCatalog(filename).size, 2, "readers cannot mutate the cached module export");

  const failed = path.join(root, "failed.ts");
  fs.writeFileSync(failed, 'throw new Error("catalog initialization failed"); export const messages = {};');
  assert.throws(() => loadCatalog(failed), /catalog initialization failed/);
  assert.equal(require.extensions[".ts"], originalHook);
});

test("effective catalogs include runtime-built ARK labels and retain language-specific overrides", () => {
  const { english, chinese, englishGames, chineseGames } = loadEffectiveCatalogs();
  const key = "settings.schema.arksurvivalevolved.force_allow_cave_flyers.title";
  assert.equal(chineseGames.get(key), "允许在洞穴骑乘飞行生物");
  assert.equal(chinese.get(key), chineseGames.get(key));
  assert.ok(englishGames.has(key));
  assert.ok(english.has(key));
  assert.deepEqual([...chineseGames.keys()].sort(), [...englishGames.keys()].sort());
  assert.equal(chinese.get("assistant.chat.assistantName"), "LAN");
});
