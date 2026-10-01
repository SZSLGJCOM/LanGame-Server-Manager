const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");

const desktop = path.resolve(__dirname, "..");
const read = (file) => fs.readFileSync(path.join(desktop, file), "utf8");
const commands = read("src-tauri/src/commands.rs");
const observability = read("src-tauri/src/commands_runtime_observability.rs");
const supervision = read("src-tauri/src/commands_runtime_supervision.rs");
const state = read("src-tauri/src/state.rs");
const effects = read("src/hooks/useDesktopEffects.ts");
const app = read("src/App.tsx");

test("bootstrap leaves expensive telemetry to an explicit request", () => {
  assert.match(commands, /include_system_snapshot:\s*Option<bool>/);
  assert.match(commands, /if include_system_snapshot\.unwrap_or\(false\)/);
  assert.match(read("src/api.ts"), /includeSystemSnapshot/);
  assert.match(effects, /bootstrapApp\(\{ includeSystemSnapshot: true \}\)/);
});

test("host probes use shared caches and blocking workers", () => {
  assert.match(state, /TimedCache<SystemSnapshot>/);
  assert.match(state, /TimedCache<Vec<BindAddressCandidate>>/);
  assert.match(observability, /fresh\(SYSTEM_SNAPSHOT_CACHE_TTL\)/);
  assert.match(supervision, /fresh\(BIND_ADDRESS_CACHE_TTL\)/);
  assert.match(observability, /spawn_timed_cache_refresh/);
  assert.match(supervision, /spawn_timed_cache_refresh/);
  assert.match(observability, /spawn_blocking\(move \|\|/);
  assert.match(observability, /tokio::join!/);
  assert.match(supervision, /pub async fn bind_address_candidates/);
  assert.match(supervision, /spawn_blocking\(WindowsPlatform::bind_address_candidates\)/);
  assert.match(read("src-tauri/src/lan_host.rs"),
    /commands::bind_address_candidates\(app_handle\.state::<DesktopState>\(\)\)\.await/);
});

test("system refresh starts on entry while bind-address probing remains deferred", () => {
  assert.doesNotMatch(effects, /SYSTEM_VIEW_INITIAL_POLL_DELAY_MS/);
  assert.match(effects, /clearInterval\(timer\)/);
  assert.match(app, /setTimeout\(refreshBindAddressCandidates,\s*BIND_ADDRESS_INITIAL_POLL_DELAY_MS\)/);
  assert.match(app, /clearTimeout\(initialTimer\)/);
});
