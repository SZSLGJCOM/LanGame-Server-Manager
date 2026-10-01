const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const test = require("node:test");

test("production Vite config serves the DST world inventory without exposing other repository files", async () => {
  const { createServer, normalizePath } = await import("vite");
  const root = path.resolve(__dirname, "..");
  const temporaryRoot = path.resolve(os.tmpdir());
  const scratch = fs.mkdtempSync(path.join(temporaryRoot, "lgsm-dst-vite-"));
  let server;
  try {
    server = await createServer({
      root,
      configFile: path.join(root, "vite.config.ts"),
      cacheDir: path.join(scratch, "cache"),
      logLevel: "silent",
      optimizeDeps: { noDiscovery: true, include: [] },
      server: { host: "127.0.0.1", port: 0, strictPort: true }
    });
    await server.listen();
    assert.equal(server.config.server.fs.strict, true);
    const base = server.resolvedUrls.local[0];
    const response = await fetch(new URL("src/views/settings/modules/dontstarve-world-groups.ts", base));
    assert.equal(response.status, 200, "the production import must resolve");
    const moduleText = await response.text();
    const inventoryImport = moduleText.match(/from\s+["']([^"']*world-options\.json[^"']*)["']/);
    assert.ok(inventoryImport, "Vite must emit the native inventory dependency");
    const inventoryResponse = await fetch(new URL(inventoryImport[1], base));
    assert.equal(inventoryResponse.status, 200, "the emitted dependency must pass Vite's file policy");
    const inventoryModule = await inventoryResponse.text();
    assert.match(inventoryModule, /export default/);
    assert.match(inventoryModule, /masterControlled/);
    const unrelatedFile = normalizePath(path.resolve(root, "../../modules/dontstarve/module.toml"));
    const blocked = await fetch(new URL(`/@fs/${unrelatedFile}`, base));
    assert.equal(blocked.status, 403, "the rest of the repository stays outside the serving allow list");
  } finally {
    if (server) await server.close();
    assert.equal(path.dirname(path.resolve(scratch)), temporaryRoot);
    assert.ok(path.basename(scratch).startsWith("lgsm-dst-vite-"));
    fs.rmSync(scratch, { recursive: true, force: true });
  }
});

test("a running Vite recovers when the external DST inventory is created and then updated", async () => {
  const { createServer } = await import("vite");
  const desktopRoot = path.resolve(__dirname, "..");
  const temporaryRoot = path.resolve(os.tmpdir());
  const scratch = fs.mkdtempSync(path.join(temporaryRoot, "lgsm-dst-vite-"));
  const root = path.join(scratch, "apps/desktop");
  const entry = "src/views/settings/modules/dontstarve-world-groups.ts";
  const inventory = path.join(scratch, "modules/dontstarve/world-options.json");
  fs.mkdirSync(path.dirname(path.join(root, entry)), { recursive: true });
  fs.mkdirSync(path.dirname(inventory), { recursive: true });
  fs.copyFileSync(path.join(desktopRoot, entry), path.join(root, entry));
  let server;
  try {
    server = await createServer({
      root,
      configFile: path.join(desktopRoot, "vite.config.ts"),
      cacheDir: path.join(scratch, "cache"),
      logLevel: "silent",
      optimizeDeps: { noDiscovery: true, include: [] },
      server: { host: "127.0.0.1", port: 0, strictPort: true }
    });
    await server.listen();
    const base = server.resolvedUrls.local[0];
    assert.equal((await fetch(new URL(entry, base))).status, 500, "first resolve fails because the generated inventory is absent");
    fs.copyFileSync(path.resolve(desktopRoot, "../../modules/dontstarve/world-options.json"), inventory);
    const deadline = Date.now() + 2500;
    let response;
    do {
      response = await fetch(new URL(entry, base));
      if (response.status === 200) break;
      await new Promise((resolve) => setTimeout(resolve, 40));
    } while (Date.now() < deadline);
    assert.equal(response.status, 200, "creation outside the frontend root must recover the failed import without restart");
    const moduleText = await response.text();
    const importPath = moduleText.match(/from\s+["']([^"']*world-options\.json[^"']*)["']/)?.[1];
    assert.ok(importPath);
    const inventoryUrl = new URL(importPath, base);
    assert.equal((await fetch(inventoryUrl)).status, 200);
    const next = JSON.parse(fs.readFileSync(inventory, "utf8"));
    next.gameVersion = "999999";
    fs.writeFileSync(inventory, JSON.stringify(next));
    const updateDeadline = Date.now() + 2500;
    let updated;
    do {
      updated = await (await fetch(inventoryUrl)).text();
      if (updated.includes("999999")) break;
      await new Promise((resolve) => setTimeout(resolve, 40));
    } while (Date.now() < updateDeadline);
    assert.ok(updated.includes("999999"), "subsequent inventory edits invalidate the transformed dependency");
  } finally {
    if (server) await server.close();
    assert.equal(path.dirname(path.resolve(scratch)), temporaryRoot);
    assert.ok(path.basename(scratch).startsWith("lgsm-dst-vite-"));
    fs.rmSync(scratch, { recursive: true, force: true });
  }
});
