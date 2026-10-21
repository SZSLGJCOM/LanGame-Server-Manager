const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const nativeRoot = path.resolve(__dirname, "../src-tauri/src");
const read = (file) => fs.readFileSync(path.join(nativeRoot, file), "utf8");

test("registered execution and transport policies cover exactly the product adapters", () => {
  const registration = read("command_registry.rs");
  const commands = [...registration.matchAll(/\(\s*([a-z_]+),\s*\[([a-z_:]+)\],\s*(Desktop|Runtime),\s*(Allowed|Denied),\s*(Bounded|Owned)\s*\)/g)]
    .map(([, name, module, execution, lan, completion]) => ({ name, module, execution, lan, completion }));
  assert.ok(commands.some(({ name }) => name === "start_instance_process"), "Registration parser omitted startup");
  assert.equal(new Set(commands.map(({ name }) => name)).size, commands.length, "Duplicate registration");
  const routes = (file) => [...read(file).matchAll(/^\s*"([a-z_]+)"\s*=>/gm)].map((match) => match[1]).sort();
  assert.deepEqual(commands.filter(({ lan }) => lan === "Allowed").map(({ name }) => name).sort(), routes("lan_host.rs"));
  assert.deepEqual(commands.filter(({ execution, lan }) => execution === "Runtime" && lan === "Denied")
    .map(({ name }) => name).sort(), routes("runtime_service/local_commands.rs"));
  for (const name of ["check_app_update", "install_app_update"]) {
    const command = commands.find((command) => command.name === name);
    assert.deepEqual(command, { name, module: "crate::app_updates", execution: "Desktop", lan: "Denied", completion: "Bounded" },
      "Updater progress Channels must remain inside their webview host");
  }
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

test("save extensions are prepared only by server startup", () => {
  assert.doesNotMatch(read("lan_host.rs"), /prepare_instance_world_save/);
  assert.doesNotMatch(read("command_registry.rs"), /prepare_instance_world_save/);
  assert.match(read("commands_runtime_lifecycle.rs"), /commands_save_startup::ensure_for_start\(/);
});

test("file Mod management forwards package identity and retains completion ownership", () => {
  const route = read("lan_host.rs").split('"manage_manual_mod" =>')[1]?.split('"read_manual_mod_inventory" =>')[0];
  assert.ok(route, "File Mod management must be reachable through the service");
  for (const argument of ["instanceId", "packageId", "action"]) {
    assert.ok(route.includes(`arg(&args, "${argument}"`), `Missing ${argument}`);
  }
  assert.match(read("command_registry.rs"), /\(manage_manual_mod,\s*\[crate::commands::commands_file_mods\],\s*Runtime,\s*Allowed,\s*Owned\)/);
});
