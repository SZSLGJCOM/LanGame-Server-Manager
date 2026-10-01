const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");

const root = path.resolve(__dirname, "../../..");
const desktopRoot = path.join(root, "apps/desktop");
const api = fs.readFileSync(path.join(desktopRoot, "src/api-transport.ts"), "utf8");
const lanHost = fs.readFileSync(path.join(desktopRoot, "src-tauri/src/lan_host.rs"), "utf8");
const packageJson = JSON.parse(fs.readFileSync(path.join(desktopRoot, "package.json"), "utf8"));

test("LAN management access remains header-only", () => {
  assert.match(api, /headers\["X-LanGame-Token"\] = token/);
  assert.match(api, /window\.location\.hash/);
  assert.match(api, /window\.history\.replaceState/);
  assert.match(lanHost, /\.get\("x-langame-token"\)/);
  assert.match(lanHost, /access\.management_token\.matches\(candidate\)/);
  assert.doesNotMatch(lanHost, /query_param\(&request\.path, "langameToken"\)/);
  assert.doesNotMatch(lanHost, /query_param\(&request\.path, "token"\)/);
  assert.doesNotMatch(api, /LAN_MEDIA_TOKEN|read_lan_media_access|langameLanMediaToken/);
  assert.doesNotMatch(lanHost, /media_token|map-image|map-tile|commands_world_map/);
});

test("LAN runtime access regression test is part of the release verification", () => {
  assert.equal(
    packageJson.scripts["verify:lan-runtime-token"],
    "node --test tests/lan-runtime-token.test.cjs"
  );
  assert.equal(packageJson.scripts.test, "node --test --test-concurrency=4");
  assert.match(packageJson.scripts.verify, /npm test/);
  assert.match(packageJson.scripts.verify, /npm run build/);
  assert.ok(
    packageJson.scripts.verify.indexOf("npm test") < packageJson.scripts.verify.indexOf("npm run build"),
    "release verification must run the security regression suite before producing the build"
  );
});
