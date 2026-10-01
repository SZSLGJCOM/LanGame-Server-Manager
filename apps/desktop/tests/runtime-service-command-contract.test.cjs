const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const nativeRoot = path.resolve(__dirname, "../src-tauri/src");
const read = (file) => fs.readFileSync(path.join(nativeRoot, file), "utf8");

test("every nonlocal desktop command has a runtime-service dispatcher", () => {
  const registration = read("main.rs").match(/tauri::generate_handler!\[([\s\S]*?)\];/);
  assert.ok(registration, "Desktop registration block missing");
  const names = [...registration[1].matchAll(/^\s*(?:\w+::)+(\w+)\s*,?\s*$/gm)].map((match) => match[1]);
  assert.ok(names.includes("start_instance_process"), "Registration parser omitted the startup contract");
  const localBlock = read("runtime_service.rs").match(/fn is_local_command\([\s\S]*?\n\}/);
  assert.ok(localBlock, "Desktop-local command boundary missing");
  const local = new Set([...localBlock[0].matchAll(/"([a-z_]+)"/g)].map((match) => match[1]));
  const routes = new Set(["runtime_service/server.rs", "runtime_service/local_commands.rs", "lan_host.rs"]
    .flatMap((file) => [...read(file).matchAll(/^\s*"([a-z_]+)"\s*=>/gm)].map((match) => match[1])));
  assert.deepEqual(names.filter((name) => !local.has(name) && !routes.has(name)), []);
  assert.ok(local.has("install_app_update"), "The Tauri update progress Channel must remain inside its webview host");
});

test("service startup routing forwards the confirmed DST world preview", () => {
  const route = read("lan_host.rs").split('"start_instance_process" =>')[1]?.split('"stop_instance_process" =>')[0];
  assert.ok(route, "Startup forwarding route missing");
  // This cross-language contract catches replacing the fourth native argument
  // with None, even when the generic decoder's own tests still pass.
  assert.match(route, /commands::start_instance_process\(\s*app_handle\.clone\(\),\s*app_handle\.state::<DesktopState>\(\),\s*arg\(&args,\s*"instanceId",\s*"instance_id"\)\?,\s*opt_arg\(&args,\s*"expectedWorldStart",\s*"expected_world_start"\)\?/);
});

test("program mode and explicit maintenance cross the background service bridge", () => {
  const routes = read("lan_host.rs");
  const creation = routes.split('"create_instance_record" =>')[1]?.split('"update_instance_record_if_current" =>')[0];
  assert.match(creation, /opt_arg\(&args, "programMode", "program_mode"\)\?/);
  const maintenance = routes.split('"update_instance_program" =>')[1]?.split('"uninstall_module_game" =>')[0];
  assert.match(maintenance, /commands::update_instance_program/);
  assert.match(maintenance, /arg\(&args, "instanceId", "instance_id"\)\?/);
  assert.match(maintenance, /arg\(&args, "validate", "validate"\)\?/);
});

test("cluster backup and recovery commands stay out of the LAN dispatcher", () => {
  const lan = read("lan_host.rs");
  const pipe = read("runtime_service/local_commands.rs");
  for (const name of ["list_ark_cluster_backups", "create_ark_cluster_backup", "restore_ark_cluster_backup",
    "read_pending_ark_cluster_restore", "recover_ark_cluster_restore"]) {
    assert.match(pipe, new RegExp(`"${name}"\\s*=>`));
    assert.doesNotMatch(lan, new RegExp(`"${name}"\\s*=>`));
  }
});
