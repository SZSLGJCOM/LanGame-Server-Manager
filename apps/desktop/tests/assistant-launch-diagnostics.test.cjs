const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

function compileTypeScript(module, filename) {
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
}
require.extensions[".ts"] = compileTypeScript;
require.extensions[".tsx"] = compileTypeScript;

const { buildAssistantViewModel } = require("../src/assistant-state.ts");
const { buildAssistantCapsuleModel } = require("../src/assistant-summary.ts");

function inputFor(plan, overrides = {}) {
  return {
    aiSettings: { enabled: false, provider: "ollama", model: "", baseUrl: "", apiKey: "", apiKeyStored: false },
    locale: "zh-CN",
    activeJobsCount: 0,
    activeView: "servers",
    bootstrap: { state: { storage: {}, snapshot: {}, modules: [], instances: [{ id: "selected", name: "Server" }] } },
    storageReady: true,
    libraryPage: "catalog",
    overlayNames: [],
    runtimeAutoRefreshPaused: false,
    runtimeRefreshIssue: null,
    selectedInstanceDetails: null,
    selectedInstanceId: "selected",
    selectedInstanceModuleDetails: null,
    selectedLaunchPlan: plan,
    selectedLaunchPlanError: null,
    selectedLogDocument: null,
    selectedModuleDetails: null,
    selectedRuntime: null,
    serverWorkspaceSection: "overview",
    steamCmdStatus: null,
    ...overrides
  };
}

function planFor(issues = [], overrides = {}) {
  return {
    instance_id: "selected",
    executable_path: "D:/games/server/jre64/bin/java.exe",
    executable_exists: true,
    ready_to_launch: !issues.some((issue) => issue.severity === "error"),
    validation_issues: issues,
    working_directory: "D:/games/server",
    install_root: "D:/games/server",
    command_line: "java -jar server.jar",
    ...overrides
  };
}

function issue(code, message, path = null, severity = "error") {
  return { code, message, path, severity };
}

for (const locale of ["zh-CN", "en-US"]) {
  test(`assistant reports missing server jars and blocked ports when Java exists (${locale})`, () => {
    const jar = "D:/games/server/server.jar";
    const input = inputFor(planFor([
      issue("launch_required_file_missing", `Required Java server jar is missing: ${jar}.`, jar),
      { ...issue("port_binding_unavailable", "Port binding 'game' (TCP/127.0.0.1:16261) is not available."),
        context: { port_name: "game", protocol: "TCP", address: "127.0.0.1:16261" } }
    ]), { locale });
    const assistant = buildAssistantViewModel(input);

    assert.equal(assistant.issues.length, 2);
    assert.ok(assistant.issues.every((item) => item.severity === "critical"));
    assert.match(assistant.issues[0].detail, /server\.jar/);
    assert.match(assistant.issues[1].detail, /16261/);
    if (locale === "zh-CN") assert.doesNotMatch(assistant.issues[1].detail, /Port binding|not available/);
    assert.match(assistant.issues[0].title, locale === "zh-CN" ? /启动.*文件.*缺失/ : /Required launch file.*missing/);
    assert.match(assistant.issues[1].title, locale === "zh-CN" ? /端口.*不可用/ : /port.*unavailable/i);
    assert.doesNotMatch(assistant.issues.map((item) => item.title).join(" "), /launch_required_file_missing|port_binding_unavailable/);
    assert.match(assistant.contextPayload, /Ready to Launch: false/);
    assert.match(assistant.contextPayload, /server\.jar/);
    assert.match(assistant.contextPayload, /16261/);
    assert.equal(buildAssistantCapsuleModel(input).tone, "critical");
  });
}

for (const locale of ["zh-CN", "en-US"]) {
  test(`assistant reports incompatible native working directories as a path issue (${locale})`, () => {
    const working_directory = "D:/existing local path/server";
    const input = inputFor(planFor([
      issue("launch_working_directory_incompatible", "UNLOCALIZED_NATIVE_MESSAGE")
    ], { working_directory }), { locale });
    const assistant = buildAssistantViewModel(input);
    assert.equal(assistant.issues.length, 1);
    const diagnostic = assistant.issues[0];
    assert.equal(diagnostic.severity, "critical");
    assert.match(diagnostic.title, locale === "zh-CN" ? /启动工作目录不受支持/ : /Launch working directory is unsupported/);
    assert.match(diagnostic.detail, locale === "zh-CN" ? /较短.*本地路径.*特殊目录名/ : /shorter local path.*special directory names/);
    assert.ok(diagnostic.detail.includes(working_directory));
    assert.equal(diagnostic.action.id, "view-servers");
    assert.doesNotMatch(diagnostic.detail, /UNLOCALIZED_NATIVE_MESSAGE|Java|下载|重装|修复|不存在|创建目录|download|reinstall|repair|does not exist|create.*director/i);
    assert.doesNotMatch(assistant.contextPayload, /UNLOCALIZED_NATIVE_MESSAGE/);
    assert.match(assistant.contextPayload, /Ready to Launch: false/);
    assert.equal(buildAssistantCapsuleModel(input).tone, "critical");
  });
}

test("a missing install root explains its missing children without hiding independent config failures", () => {
  const input = inputFor(planFor([
    issue("working_directory_missing", "Working directory is missing.", "D:\\games\\server"),
    issue("launch_executable_missing", "Launch executable is missing.", "D:/games/server/jre64/bin/java.exe"),
    issue("launch_required_file_missing", "Required jar is missing.", "D:/games/server/server.jar"),
    issue("install_root_missing", "Install root does not exist.", "D:/games/server"),
    issue("config_dir_missing", "Instance config directory is missing.", "D:/instances/selected/config")
  ], { executable_exists: false }));
  const assistant = buildAssistantViewModel(input);

  assert.equal(assistant.issues.length, 2);
  assert.ok(assistant.issues.some((item) => /服务端安装目录缺失/.test(item.title)));
  assert.ok(assistant.issues.some((item) => /实例配置目录缺失/.test(item.title)));
  assert.ok(assistant.issues.every((item) => item.severity === "critical"));
  assert.equal(buildAssistantCapsuleModel(input).tone, "critical");
});

test("launch diagnostics collapse repeated paths but keep different missing files and sibling paths", () => {
  const input = inputFor(planFor([
    issue("install_root_missing", "Missing installation.", "D:/games/server"),
    issue("launch_required_file_missing", "Missing first jar.", "D:/games/server-extra/first.jar"),
    issue("launch_required_file_missing", "Missing first jar again.", "D:\\games\\server-extra\\first.jar"),
    issue("launch_required_file_missing", "Missing second jar.", "D:/games/server-extra/second.jar"),
    issue("launch_required_file_missing", "Missing external jar.", "D:/games/server/../shared/third.jar")
  ]));

  assert.equal(buildAssistantViewModel(input).issues.length, 4);
});

test("warnings remain visible without making a ready launch critical", () => {
  const input = inputFor(planFor([
    issue("server_version_notice", "Check the installed server version before clients join.", null, "warning")
  ]));

  assert.equal(buildAssistantViewModel(input).issues[0]?.severity, "warning");
  assert.equal(buildAssistantCapsuleModel(input).tone, "warning");
});

for (const locale of ["zh-CN", "en-US"]) {
  test(`instance launch preparation is informational when source files are ready (${locale})`, () => {
    const input = inputFor(planFor([
      issue("launch_preparation_required", "", "D:/instances/selected/launch/server.exe", "info")
    ], { executable_exists: false, ready_to_launch: true }), { locale });
    const assistant = buildAssistantViewModel(input);
    assert.equal(assistant.issues.length, 1);
    assert.equal(assistant.issues[0].severity, "info");
    assert.match(assistant.issues[0].detail, locale === "zh-CN" ? /源文件已就绪.*自动/ : /source files are ready.*automatically/);
    assert.doesNotMatch(assistant.issues[0].detail, /修复|repair|不存在|does not exist/);
    assert.equal(buildAssistantCapsuleModel(input).tone, "info");
  });
}

test("unresolved required launch arguments block launch", () => {
  const input = inputFor(planFor([
    { ...issue("unresolved_launch_args", "Unresolved launch argument."), context: { count: "1" } }
  ]));
  assert.equal(buildAssistantViewModel(input).issues[0].severity, "critical");
  assert.equal(buildAssistantCapsuleModel(input).tone, "critical");
});

test("explicit launch refusal has a blocking fallback when no error detail is supplied", () => {
  const input = inputFor(planFor([], { ready_to_launch: false }));
  const assistant = buildAssistantViewModel(input);

  assert.equal(assistant.issues.length, 1);
  assert.equal(assistant.issues[0].severity, "critical");
  assert.equal(assistant.issues[0].action.id, "refresh-launch-preview");
  assert.equal(buildAssistantCapsuleModel(input).tone, "critical");
});

test("a preview without instance identity is excluded from diagnostics and private context", () => {
  for (const instance_id of [undefined, ""]) {
    const input = inputFor(planFor([
      issue("launch_required_file_missing", "Missing private server jar.", "D:/games/unbound/private.jar")
    ], { instance_id }));
    const assistant = buildAssistantViewModel(input);

    assert.deepEqual(assistant.issues, []);
    assert.doesNotMatch(assistant.contextPayload, /Launch Preview|private\.jar|java\.exe/);
    assert.ok(assistant.prompts.every((prompt) => !prompt.payload.includes("private.jar")));
    assert.equal(buildAssistantCapsuleModel(input).tone, "info");
  }
});

test("preview failure replaces previous launch details and keeps capsule severity aligned", () => {
  const input = inputFor(planFor([], { executable_exists: false }), { selectedLaunchPlanError: "Preview request failed" });
  const assistant = buildAssistantViewModel(input);

  assert.equal(assistant.issues.length, 1);
  assert.equal(assistant.issues[0].id, "launch-preview-error");
  assert.equal(buildAssistantCapsuleModel(input).tone, assistant.issues[0].severity);
  assert.doesNotMatch(assistant.contextPayload, /Executable Path|java\.exe/);
});

test("an empty instance list does not add a chat notice", () => {
  const input = inputFor(null, {
    selectedInstanceId: null,
    bootstrap: { state: { storage: {}, snapshot: {}, modules: [], instances: [] } }
  });
  assert.deepEqual(buildAssistantViewModel(input).issues, []);
  assert.equal(buildAssistantCapsuleModel(input).tone, "info");
});

test("a preview for another instance is excluded from issues, prompts, and capsule tone", () => {
  for (const selectedInstanceId of ["different", null]) {
    const input = inputFor(planFor([
      issue("launch_required_file_missing", "Missing private server jar.", "D:/games/previous/private.jar")
    ]), { selectedInstanceId });
    const assistant = buildAssistantViewModel(input);

    assert.deepEqual(assistant.issues, []);
    assert.doesNotMatch(assistant.contextPayload, /Launch Preview|private\.jar|java\.exe/);
    assert.ok(assistant.prompts.every((prompt) => !prompt.payload.includes("private.jar")));
    assert.equal(buildAssistantCapsuleModel(input).tone, "info");
  }
});

test("unknown backend failures retain their actionable message without exposing a code as the title", () => {
  const input = inputFor(planFor([issue("future_launch_check", "A required runtime service is unavailable.")]));
  const assistant = buildAssistantViewModel(input);

  assert.equal(assistant.issues.length, 1);
  assert.match(assistant.issues[0].detail, /required runtime service/);
  assert.doesNotMatch(assistant.issues[0].title, /future_launch_check/);
  assert.equal(buildAssistantCapsuleModel(input).tone, "critical");
});

function selectedDetails(status, id = "selected") {
  return {
    summary: { id, name: "Server", status, module_id: "projectzomboid" },
    config_file_path: "D:/instances/selected/config/server.ini",
    settings_json: "{}",
    ports: []
  };
}

test("a running selected instance does not report its startup port probe as a launch fault", () => {
  const input = inputFor(planFor([
    issue("port_binding_unavailable", "Port binding 'game' (UDP/127.0.0.1:16261) is not available.")
  ]), { selectedInstanceDetails: selectedDetails("Running") });
  const assistant = buildAssistantViewModel(input);

  assert.deepEqual(assistant.issues, []);
  assert.equal(buildAssistantCapsuleModel(input).tone, "info");
  assert.match(assistant.contextPayload, /Port Preflight:.*already active/);
  assert.doesNotMatch(assistant.contextPayload, /Ready to Launch: false|端口无法绑定|Port binding.*not available/);
});

test("a running instance still reports missing launch files", () => {
  const input = inputFor(planFor([
    issue("port_binding_unavailable", "Port is unavailable."),
    issue("launch_required_file_missing", "Required jar is missing.", "D:/games/server/server.jar")
  ]), { selectedInstanceDetails: selectedDetails("Running") });
  const assistant = buildAssistantViewModel(input);

  assert.equal(assistant.issues.length, 1);
  assert.match(assistant.issues[0].detail, /server\.jar/);
  assert.equal(buildAssistantCapsuleModel(input).tone, "critical");
  assert.match(assistant.contextPayload, /Ready to Launch: false/);
});

test("stopped instances keep real port conflicts even when an unkeyed runtime snapshot is running", () => {
  const input = inputFor(planFor([
    issue("port_binding_unavailable", "Port binding 'game' (UDP/127.0.0.1:16261) is not available.")
  ]), {
    selectedInstanceDetails: selectedDetails("Stopped"),
    selectedRuntime: { health: { status: "healthy", summary: "Running" }, recent_runs: [{ run_id: 99, status: "Running" }], log_tail: { lines: [] } }
  });

  assert.equal(buildAssistantViewModel(input).issues[0]?.severity, "critical");
  assert.equal(buildAssistantCapsuleModel(input).tone, "critical");
});

test("running details for another selection cannot hide the current instance's port conflict", () => {
  const input = inputFor(planFor([issue("port_binding_unavailable", "Port is unavailable.")]), {
    selectedInstanceDetails: selectedDetails("Running", "previous")
  });

  assert.equal(buildAssistantViewModel(input).issues[0]?.severity, "critical");
  assert.equal(buildAssistantCapsuleModel(input).tone, "critical");
});

test("blocking launch failures precede warnings in the assistant's first visible issue", () => {
  const input = inputFor(planFor([
    issue("server_version_notice", "Check the installed server version.", null, "warning"),
    issue("launch_required_file_missing", "Required jar is missing.", "D:/games/server/server.jar")
  ]), { runtimeAutoRefreshPaused: true });

  assert.equal(buildAssistantViewModel(input).issues[0]?.severity, "critical");
  assert.equal(buildAssistantCapsuleModel(input).tone, "critical");
});
