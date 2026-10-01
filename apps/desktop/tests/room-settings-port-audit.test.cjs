const assert = require("node:assert/strict");
const { spawnSync } = require("node:child_process");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const test = require("node:test");

const root = path.resolve(__dirname, "..", "..", "..");
const auditScript = path.join(root, "scripts", "audit_room_settings_ports.py");
const targetModuleCount = 32;

function readBundledModuleManifests() {
  const script = [
    "import json, pathlib, tomllib",
    "root = pathlib.Path.cwd() / 'modules'",
    "manifests = {path.parent.name: tomllib.loads(path.read_text(encoding='utf-8-sig')) for path in root.glob('*/module.toml')}",
    "print(json.dumps(manifests))"
  ].join("; ");
  const result = spawnSync("python", ["-c", script], { cwd: root, encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr || result.stdout);
  return JSON.parse(result.stdout);
}

function writeInstance(rootDir, id, data) {
  const configDir = path.join(rootDir, id, "config");
  fs.mkdirSync(configDir, { recursive: true });
  fs.writeFileSync(path.join(configDir, "instance.json"), JSON.stringify(data, null, 2), "utf8");
}

function runAuditCommand(args, moduleOnly = false) {
  let command = [auditScript, ...args];
  if (moduleOnly) {
    const guardedEntry = [
      "import pathlib, runpy, sys",
      "script = pathlib.Path(sys.argv[1])",
      "modules = script.resolve().parent.parent / 'modules'",
      "original_exists = pathlib.Path.exists",
      "def module_only_exists(candidate):",
      "    if not candidate.is_relative_to(modules):",
      "        raise AssertionError('Instance access requires --instances-root')",
      "    return original_exists(candidate)",
      "pathlib.Path.exists = module_only_exists",
      "sys.argv = sys.argv[1:]",
      "runpy.run_path(str(script), run_name='__main__')"
    ].join("\n");
    command = ["-c", guardedEntry, ...command];
  }
  return spawnSync("python", command, { cwd: root, encoding: "utf8" });
}

function runAudit(instanceRoot, extraArgs = []) {
  const outputDir = fs.mkdtempSync(path.join(os.tmpdir(), "room-port-audit-"));
  const jsonPath = path.join(outputDir, "report.json");
  const mdPath = path.join(outputDir, "report.md");
  const result = runAuditCommand(
    [
      ...(instanceRoot ? ["--instances-root", instanceRoot] : []),
      "--json",
      jsonPath,
      "--md",
      mdPath,
      "--strict",
      ...extraArgs
    ],
    !instanceRoot
  );
  const report = fs.existsSync(jsonPath) ? JSON.parse(fs.readFileSync(jsonPath, "utf8")) : null;
  return { result, report, mdPath };
}

test("room port audit defaults to bundled modules without instance access", () => {
  const result = runAuditCommand([], true);
  assert.equal(result.status, 0, result.stderr || result.stdout);
  const summary = JSON.parse(result.stdout);
  assert.equal(summary.module_count, targetModuleCount);
  assert.equal(summary.instance_count, 0);
  assert.equal(summary.repair_count, 0);

  const { result: reportResult, report } = runAudit();
  assert.equal(reportResult.status, 0, reportResult.stderr || reportResult.stdout);
  assert.equal(report.instances_root, null);
  assert.deepEqual(report.instances, []);
  assert.deepEqual(report.repairs, []);
});

test("room port repair requires an explicit instance root before accessing files", () => {
  const result = runAuditCommand(["--repair-missing"], true);
  assert.equal(result.status, 2, result.stderr || result.stdout);
  assert.match(result.stderr, /--repair-missing requires --instances-root/);
  assert.equal(result.stdout, "");
});

test("room port repair updates only the explicitly selected instance fixture", () => {
  const instanceRoot = fs.mkdtempSync(path.join(os.tmpdir(), "room-port-repair-instances-"));
  const fixture = {
    instance_id: "seven-days-repair",
    module_id: "sevendaystodie",
    ports: [
      { name: "game_udp", protocol: "udp", port: 27900 },
      { name: "game_tcp", protocol: "tcp", port: 27900 },
      { name: "web_dashboard", protocol: "tcp", port: 9080 }
    ],
    settings: { server_name: "Retained fixture" }
  };
  writeInstance(instanceRoot, fixture.instance_id, fixture);

  const { result, report } = runAudit(instanceRoot, ["--repair-missing"]);
  assert.equal(result.status, 0, result.stderr || result.stdout);
  assert.equal(report.summary.instance_count, 1);
  assert.equal(report.summary.repair_count, 1);
  assert.equal(report.summary.failure_count, 0);
  const saved = JSON.parse(fs.readFileSync(path.join(instanceRoot, fixture.instance_id, "config", "instance.json"), "utf8"));
  assert.deepEqual(saved, {
    ...fixture,
    ports: [...fixture.ports, { name: "telnet", protocol: "tcp", port: 8081 }]
  });
  const { result: readResult, report: readReport } = runAudit(instanceRoot);
  assert.equal(readResult.status, 0, readResult.stderr || readResult.stdout);
  assert.equal(readReport.summary.repair_count, 0);
  assert.equal(readReport.summary.instance_port_count, 4);
});

test("room port audit passes for a complete instance fixture", () => {
  const instanceRoot = fs.mkdtempSync(path.join(os.tmpdir(), "room-port-audit-instances-"));
  writeInstance(instanceRoot, "seven-days-valid", {
    instance_id: "seven-days-valid",
    instance_name: "7 Days Valid",
    module_id: "sevendaystodie",
    autostart: false,
    ports: [
      { name: "game_udp", protocol: "udp", port: 26900 },
      { name: "game_tcp", protocol: "tcp", port: 26900 },
      { name: "web_dashboard", protocol: "tcp", port: 8080 },
      { name: "telnet", protocol: "tcp", port: 8081 }
    ],
    settings: {}
  });

  const { result, report, mdPath } = runAudit(instanceRoot);

  assert.equal(result.status, 0, result.stderr || result.stdout);
  assert.equal(report.summary.module_count, targetModuleCount);
  assert.equal(report.summary.instance_count, 1);
  assert.equal(report.summary.failure_count, 0);
  assert.equal(fs.existsSync(mdPath), true);
});

test("room port audit fails when an instance drops a declared module port", () => {
  const instanceRoot = fs.mkdtempSync(path.join(os.tmpdir(), "room-port-audit-instances-"));
  writeInstance(instanceRoot, "seven-days-missing-telnet", {
    instance_id: "seven-days-missing-telnet",
    instance_name: "7 Days Missing Telnet",
    module_id: "sevendaystodie",
    autostart: false,
    ports: [
      { name: "game_udp", protocol: "udp", port: 26900 },
      { name: "game_tcp", protocol: "tcp", port: 26900 },
      { name: "web_dashboard", protocol: "tcp", port: 8080 }
    ],
    settings: {}
  });

  const { result, report } = runAudit(instanceRoot);

  assert.notEqual(result.status, 0);
  assert.equal(report.summary.module_count, targetModuleCount);
  assert.equal(report.summary.instance_count, 1);
  assert.match(JSON.stringify(report.failures), /missing_instance_port/);
  assert.match(JSON.stringify(report.failures), /telnet/);
});

test("all bundled module ports have exactly one player or service role", () => {
  const manifests = readBundledModuleManifests();
  assert.equal(Object.keys(manifests).length, targetModuleCount);

  for (const [moduleId, manifest] of Object.entries(manifests)) {
    const declaredPorts = (manifest.default_ports ?? []).map((port) => port.name);
    const roles = manifest.runtime?.port_roles ?? [];
    const roleCounts = new Map(declaredPorts.map((name) => [name, 0]));
    const declaredRoles = new Set();

    for (const roleSpec of roles) {
      assert.match(roleSpec.role, /^(player|service)$/, `${moduleId} has an invalid port role`);
      assert.equal(declaredRoles.has(roleSpec.role), false, `${moduleId} repeats role ${roleSpec.role}`);
      declaredRoles.add(roleSpec.role);
      for (const portName of roleSpec.port_names ?? []) {
        assert.equal(roleCounts.has(portName), true, `${moduleId} role references unknown port ${portName}`);
        roleCounts.set(portName, roleCounts.get(portName) + 1);
      }
    }

    for (const [portName, count] of roleCounts) {
      assert.equal(count, 1, `${moduleId}.${portName} must belong to exactly one role`);
    }
  }
});

test("administrator-facing ports are classified as service ports", () => {
  const manifests = readBundledModuleManifests();
  const expectedServicePorts = {
    arksurvivalascended: ["rcon"],
    arksurvivalevolved: ["rcon"],
    conanexiles: ["rcon"],
    humanitz: ["rcon"],
    minecraft: ["rcon"],
    palworld: ["rcon", "rest_api"],
    projectzomboid: ["rcon"],
    rust: ["rcon"],
    sevendaystodie: ["web_dashboard", "telnet"],
    soulmask: ["echo", "rcon"],
    squad: ["rcon"],
    vrising: ["rcon"]
  };

  for (const [moduleId, expectedPorts] of Object.entries(expectedServicePorts)) {
    const servicePorts = (manifests[moduleId].runtime?.port_roles ?? [])
      .filter((roleSpec) => roleSpec.role === "service")
      .flatMap((roleSpec) => roleSpec.port_names)
      .sort();
    assert.deepEqual(servicePorts, [...expectedPorts].sort(), `${moduleId} service port classification changed`);
  }
});
