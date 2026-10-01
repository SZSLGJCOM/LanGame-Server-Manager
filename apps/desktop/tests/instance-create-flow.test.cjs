const assert = require("node:assert/strict");
const fs = require("node:fs");
const Module = require("node:module");
const os = require("node:os");
const path = require("node:path");
const test = require("node:test");
const {
  runTypeScriptCli,
  transpileTypeScript
} = require("../scripts/typescript_source_tools.cjs");

const desktopRoot = path.resolve(__dirname, "..");
const repositoryRoot = path.resolve(desktopRoot, "..", "..");

function registerTypeScriptRequireExtension(extension) {
  require.extensions[extension] = function compileTypeScript(module, filename) {
    const source = fs.readFileSync(filename, "utf8");
    module._compile(transpileTypeScript(source, filename), filename);
  };
}

registerTypeScriptRequireExtension(".ts");
registerTypeScriptRequireExtension(".tsx");
// Browser-only styles can be imported by shared mock dependencies; they do not
// participate in these API and state-transition contracts.
require.extensions[".css"] = function ignoreStyles() {};
const originalResolveFilename = Module._resolveFilename;
Module._resolveFilename = function resolveRawImports(request, parent, isMain, options) {
  if (typeof request === "string" && request.endsWith("?raw")) {
    const resolved = originalResolveFilename.call(this, request.slice(0, -4), parent, isMain, options);
    return `${resolved}?raw`;
  }
  return originalResolveFilename.call(this, request, parent, isMain, options);
};
require.extensions[".toml?raw"] = function compileRawToml(module, filename) {
  const source = fs.readFileSync(filename.slice(0, -4), "utf8");
  module._compile(`module.exports = ${JSON.stringify(source)};`, filename);
};

function readSource(...segments) {
  return fs.readFileSync(path.join(repositoryRoot, ...segments), "utf8");
}

test("CreateInstanceInput accepts manager identity without runtime configuration", () => {
  const tempParent = path.join(os.tmpdir(), "langame-create-input-contracts");
  fs.mkdirSync(tempParent, { recursive: true });
  const tempRoot = fs.mkdtempSync(path.join(tempParent, "create-input-"));
  const fixturePath = path.join(tempRoot, "create-input-contract.ts");
  const typesPath = path.join(desktopRoot, "src", "types.ts");
  let relativeTypesPath = path.relative(tempRoot, typesPath).replaceAll("\\", "/").replace(/\.ts$/, "");
  if (!relativeTypesPath.startsWith(".") && !path.isAbsolute(relativeTypesPath)) {
    relativeTypesPath = `./${relativeTypesPath}`;
  }
  fs.writeFileSync(
    fixturePath,
    [
      `import type { CreateInstanceInput } from ${JSON.stringify(relativeTypesPath)};`,
      'type AssertNever<T extends never> = T;',
      'type RetiredRuntimeKeys = AssertNever<Extract<keyof CreateInstanceInput, "bind_ip" | "autostart" | "program_source">>;',
      'const input: CreateInstanceInput = { name: "Managed Palworld", module_id: "palworld" };',
      'void (undefined as RetiredRuntimeKeys);',
      "void input;"
    ].join("\n")
  );

  try {
    const result = runTypeScriptCli({
      cwd: desktopRoot,
      arguments: [
        "--noEmit",
        "--strict",
        "--target",
        "ES2020",
        "--module",
        "ESNext",
        "--moduleResolution",
        "Bundler",
        "--skipLibCheck",
        "--ignoreConfig",
        fixturePath
      ]
    });
    assert.equal(result.status, 0, result.output);
  } finally {
    fs.rmSync(tempRoot, { recursive: true, force: true });
    try {
      fs.rmdirSync(tempParent);
    } catch {
      // Another test may still own a sibling fixture directory.
    }
  }
});

test("mock creation applies backend-equivalent runtime defaults", async () => {
  const { invokeMock } = require(path.join(desktopRoot, "src", "api-mock.ts"));
  const provisioning = await invokeMock("create_instance_record", {
    input: { name: "Mock Backend Defaults", module_id: "palworld" }
  });

  assert.equal(provisioning.summary.bind_ip, "0.0.0.0");
  assert.equal(provisioning.summary.autostart, false);
  const details = await invokeMock("read_instance_details_from_storage", { instanceId: provisioning.summary.id });
  assert.equal(JSON.parse(details.settings_json).bind_ip, "0.0.0.0");
});

test("Library creation owns manager identity and supported program mode", () => {
  const librarySource = readSource("apps", "desktop", "src", "views", "LibraryView.tsx");
  const detailSource = readSource("apps", "desktop", "src", "views", "library", "LibraryDetailPage.tsx");
  const actionsSource = readSource("apps", "desktop", "src", "views", "library", "LibraryServerActions.tsx");

  assert.doesNotMatch(librarySource, /fetchBindAddressCandidates|BIND_ADDRESS_POLL_MS|bindAddressCandidates|preferredBindIp|\[bindIp,|\[autostart,/);
  assert.doesNotMatch(detailSource, /BindAddressCandidate|bindAddressCandidates|bindIp|autostart|onBindIpChange|onAutostartChange/);
  assert.match(detailSource, /<LibraryServerActions/);
  assert.doesNotMatch(actionsSource, /BindAddressCandidate|bindAddressCandidates|bindIp|autostart|onBindIpChange|onAutostartChange/);
  assert.match(actionsSource, /library\.detail\.instanceName/);
});

test("program capability drives shared defaults and explicit independent creation", async () => {
  const { invokeMock } = require(path.join(desktopRoot, "src", "api-mock.ts"));
  const module = await invokeMock("read_module_details", { moduleId: "minecraft" });
  assert.equal(module.runtime.program_sharing, "shared");
  const shared = await invokeMock("create_instance_record", { input: { name: "Shared", module_id: "minecraft" } });
  const independent = await invokeMock("create_instance_record", {
    input: { name: "Independent", module_id: "minecraft" }, programMode: "independent"
  });
  const sharedReport = await invokeMock("read_instance_isolation", { input: { instance_id: shared.summary.id } });
  const independentReport = await invokeMock("read_instance_isolation", { input: { instance_id: independent.summary.id } });
  assert.equal(sharedReport.mode, "shared");
  assert.equal(independentReport.mode, "private");
  assert.notEqual(sharedReport.runtime_path, independentReport.runtime_path);
  assert.equal(sharedReport.conflicts.length, 0);
  const update = await invokeMock("update_instance_program", { instanceId: independent.summary.id, validate: true });
  assert.equal(update.install_root, independentReport.runtime_path);
  const sharedUpdate = await invokeMock("update_instance_program", { instanceId: shared.summary.id, validate: false });
  assert.equal(sharedUpdate.install_root, sharedReport.runtime_path);
  await assert.rejects(invokeMock("create_instance_record", {
    input: { name: "Unsupported sharing", module_id: "palworld" }, programMode: "shared"
  }), /independent/);
});

test("successful creation reloads bootstrap before opening the new Settings workspace", () => {
  const actionsSource = readSource("apps", "desktop", "src", "hooks", "useDesktopActions.ts");
  const body = actionsSource.match(/async function handleCreateServer\(input: CreateInstanceInput\) \{[\s\S]*?\n  async function handleStartServer/);

  assert.ok(body, "handleCreateServer should be present");
  const reloadIndex = body[0].indexOf("await options.reloadBootstrap");
  const openIndex = body[0].indexOf('options.openInstanceView("settings", provisioning.summary.id)');
  assert.ok(reloadIndex >= 0, "creation should reload bootstrap");
  assert.ok(openIndex > reloadIndex, "Settings navigation should happen after bootstrap reload");
  assert.doesNotMatch(body[0], /options\.openView\(|options\.setSelectedInstanceId\(/);
});

test("creation uses fresh instance settings and independent data without a program source choice", async () => {
  const { invokeMock } = require(path.join(desktopRoot, "src", "api-mock.ts"));
  await invokeMock("install_module_game", { moduleId: "minecraft" });
  const instance = await invokeMock("create_instance_record", {
    input: { name: "New clean server", module_id: "minecraft" }, programMode: "independent"
  });
  const isolation = await invokeMock("read_instance_isolation", { input: { instance_id: instance.summary.id } });
  assert.equal(isolation.mode, "private");
  assert.notEqual(isolation.config_path, isolation.runtime_path);
  assert.notEqual(isolation.saves_path, isolation.runtime_path);
  const details = await invokeMock("read_instance_details_from_storage", { instanceId: instance.summary.id });
  assert.equal(details.active_run, null);
  assert.equal(details.summary.status, "Stopped");
  assert.deepEqual(await invokeMock("list_instance_backups", { instanceId: instance.summary.id }), []);
});

test("program counts distinguish independent instances, archives and shared references", async () => {
  const { invokeMock } = require(path.join(desktopRoot, "src", "api-mock.ts"));
  const read = async () => (await invokeMock("read_module_details", { moduleId: "minecraft" })).summary;
  const before = await read();
  const shared = await invokeMock("create_instance_record", {
    input: { name: "Shared count", module_id: "minecraft" }, programMode: "shared"
  });
  assert.equal((await read()).instance_program_count, before.instance_program_count);
  const independent = await invokeMock("create_instance_record", {
    input: { name: "Independent count", module_id: "minecraft" }, programMode: "independent"
  });
  assert.equal((await read()).instance_program_count, before.instance_program_count + 1);
  await invokeMock("archive_instance_record", { instanceId: independent.summary.id });
  let summary = await read();
  assert.equal(summary.instance_program_count, before.instance_program_count);
  assert.equal(summary.archived_program_count, before.archived_program_count + 1);
  assert.equal(summary.install_state, before.install_state);
  const archive = (await invokeMock("list_instance_archives")).archives.find((item) => item.instance_id === independent.summary.id);
  await invokeMock("restore_instance_archive", { input: { archive_id: archive.archive_id } });
  summary = await read();
  assert.equal(summary.instance_program_count, before.instance_program_count + 1);
  assert.equal(summary.archived_program_count, before.archived_program_count);
  await invokeMock("archive_instance_record", { instanceId: shared.summary.id });
  assert.equal((await read()).archived_program_count, before.archived_program_count);
  const archiveCount = (await invokeMock("list_instance_archives")).archives.length;
  const deleted = await invokeMock("delete_instance_record", { instanceId: independent.summary.id });
  assert.equal(deleted.instance_id, independent.summary.id);
  assert.equal(typeof deleted.deleted_instance_root, "string");
  assert.equal(Object.hasOwn(deleted, "archived_instance_root"), false);
  assert.equal((await invokeMock("list_instance_archives")).archives.length, archiveCount,
    "Permanent deletion must not create a recoverable archive");
  assert.equal((await read()).instance_program_count, before.instance_program_count);
});

test("creation availability follows stored programs without changing library status", () => {
  const { hasStoredProgram } = require(path.join(desktopRoot, "src", "views", "library", "library-shared.tsx"));
  const module = { id: "game", install_state: "NotInstalled" };
  assert.equal(hasStoredProgram(module), false);
  assert.equal(hasStoredProgram({ ...module, install_state: "Incomplete" }), false);
  assert.equal(hasStoredProgram({ ...module, install_state: "Installed" }), true);
  assert.equal(hasStoredProgram({ ...module, instance_program_count: 1 }), true);
  assert.equal(hasStoredProgram({ ...module, archived_program_count: 1 }), true);
  assert.equal(module.install_state, "NotInstalled");
});
