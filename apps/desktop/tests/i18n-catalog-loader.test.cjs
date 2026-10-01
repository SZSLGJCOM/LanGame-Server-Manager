const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

require.extensions[".ts"] = function compileTypeScript(module, filename) {
  const source = fs.readFileSync(filename, "utf8");
  module._compile(transpileTypeScript(source, filename), filename);
};

const { createCatalogLoader } = require(path.resolve(__dirname, "..", "src", "i18n-catalog-loader.ts"));

test("catalog loader deduplicates an in-flight locale import", async () => {
  const cache = {};
  let imports = 0;
  let resolveImport;
  const loader = createCatalogLoader(cache, () => {
    imports += 1;
    return new Promise((resolve) => {
      resolveImport = resolve;
    });
  });

  const first = loader.load("zh-CN");
  const second = loader.load("zh-CN");
  assert.equal(first, second);
  assert.equal(imports, 0);

  await Promise.resolve();
  assert.equal(imports, 1);
  resolveImport({ ready: true });
  assert.deepEqual(await first, { ready: true });
  assert.deepEqual(await second, { ready: true });
});

test("catalog loader retries after a rejected import", async () => {
  const cache = {};
  let imports = 0;
  const loader = createCatalogLoader(cache, async () => {
    imports += 1;
    if (imports === 1) {
      throw new Error("temporary chunk failure");
    }
    return { ready: true };
  });

  await assert.rejects(loader.load("zh-CN"), /temporary chunk failure/);
  assert.deepEqual(await loader.load("zh-CN"), { ready: true });
  assert.equal(imports, 2);
});

test("invalidating a loaded locale forces a fresh import", async () => {
  const cache = {};
  let imports = 0;
  const loader = createCatalogLoader(cache, async () => ({ version: ++imports }));

  assert.deepEqual(await loader.load("en-US"), { version: 1 });
  assert.deepEqual(await loader.load("en-US"), { version: 1 });
  loader.invalidate("en-US");
  assert.deepEqual(await loader.load("en-US"), { version: 2 });
});
