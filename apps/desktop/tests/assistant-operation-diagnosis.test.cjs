const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { parseSource, sourceText, transpileTypeScript, visitSyntax } = require("../scripts/typescript_source_tools.cjs");

const filename = path.join(__dirname, "..", "src", "App.tsx");
const source = fs.readFileSync(filename, "utf8");
let declaration;
visitSyntax(parseSource(source, filename), (node) => {
  if (node.type === "FunctionDeclaration" && node.identifier?.value === "executeAssistantOperationRequest") {
    declaration = node;
    return false;
  }
});
assert.ok(declaration, "The application must expose its assistant operation handler");
const handlerSource = transpileTypeScript(
  `${sourceText(source, declaration)}\nexport { executeAssistantOperationRequest };`,
  "assistant-operation-handler.ts"
);

function loadHandler(operation) {
  const workflow = { exports: {} };
  for (const file of ["assistant-workflow.ts", "assistant-conversations.ts", "assistant-turn-control.ts", "assistant-progress.ts"]) {
    const workflowFile = path.join(__dirname, "..", "src", file);
    vm.runInNewContext(transpileTypeScript(fs.readFileSync(workflowFile, "utf8"), workflowFile), { module: workflow, exports: workflow.exports, require, TextEncoder, setTimeout, clearTimeout });
  }
  const exports = {};
  const states = [];
  const messages = [];
  const requests = [];
  const unexpected = [];
  const activities = [];
  const rejectUnexpected = (name) => async () => {
    unexpected.push(name);
    throw new Error(`Unexpected ${name} after a backend diagnosis`);
  };
  vm.runInNewContext(handlerSource, {
    exports,
    ...workflow.exports,
    assistantTurnRef: { current: null },
    assistantProgressRef: { current: null },
    getAssistantConversationState: async (conversationId) => ({ conversationId, status: "idle", revision: 1, continuation: null }),
    cancelAssistantTurn: async (conversationId) => ({ conversationId, stopping: false }),
    assistantRequestRef: { current: false }, assistantConversationScopeKey: "scope", assistantScopeRef: { current: "scope" },
    assistantCanRun: true,
    locale: "zh-CN",
    aiSettings: { provider: "ollama", model: "fixture-model", baseUrl: "http://127.0.0.1:11434/v1", apiKey: "" },
    selectedInstanceId: "fixture-instance",
    selectedModuleId: "dontstarve",
    t: (key) => key,
    message: (key, params) => ({ key, params }),
    describeError: (error) => error.message,
    formatDesktopError: (_t, error) => error.message,
    formatAiProviderLabel: () => "Fixture provider",
    createAssistantMessage: (message) => message,
    assistantConversations: {
      setContinuation: () => {},
      setRecoveryPending: () => {},
      beginTurn: (message) => ({ conversationId: "fixture-conversation", backendConversationId: "fixture-conversation", messages: [message] }),
      appendMessage: (conversationId, message) => messages.push({ conversationId, ...message })
    },
    setAssistantExecution: (state) => states.push(state),
    setActivity: (activity) => activities.push(activity),
    executeAssistantOperation: async (input) => { requests.push(input); return {
      conversationId: "fixture-conversation", conversationRevision: 1,
      appliedSettingsKeys: [], appliedPortNames: [], workshopItemIds: [], resolvedModIds: [], sourcePaths: [], runtimeCommands: [], ...operation,
    }; },
    runAssistant: rejectUnexpected("general chat request"),
    executeAssistantRequest: rejectUnexpected("general chat handler"),
    confirmAssistantOperation: rejectUnexpected("operation confirmation"),
    reloadBootstrap: rejectUnexpected("workspace reload"),
    openInstanceView: rejectUnexpected("instance navigation"),
    openLibraryDetail: rejectUnexpected("library navigation"),
    markRuntimeRefreshed: () => unexpected.push("runtime refresh"),
    assistantConfirmation: { confirmPreview: () => { unexpected.push("confirmation dialog"); return false; } }
  }, { filename });
  return { run: exports.executeAssistantOperationRequest, states, messages, requests, unexpected, activities };
}

test("read-only assistant diagnosis preserves backend evidence without a second model request", async () => {
  const diagnosis = "当前启动失败，尚未修改。\n证据：server_log.txt 第 42 行显示依赖 Mod 缺失。\n请先确认该 Mod 的发布版本。";
  const handler = loadHandler({ handled: false, requiresConfirmation: false, action: "none", message: diagnosis });
  await handler.run({
    promptLabel: "检查启动失败",
    prompt: "检查当前服务器为什么启动失败",
    contextPayload: "Selected server context",
    userMessage: { role: "user", content: "检查启动失败", state: "ready" }
  });

  assert.equal(handler.requests.length, 1);
  assert.equal(handler.requests[0].selectedInstanceId, "fixture-instance");
  assert.equal(handler.requests[0].selectedModuleId, "dontstarve");
  assert.deepEqual(handler.unexpected, []);
  assert.deepEqual(handler.states.map((state) => state.status), ["running", "inconclusive"]);
  assert.equal(handler.messages.length, 1);
  assert.equal(handler.messages[0].conversationId, "fixture-conversation");
  assert.equal(handler.messages[0].content, diagnosis);
  assert.equal(handler.messages[0].meta, undefined, "Read-only diagnosis must not expose the internal action enum");
  assert.equal(handler.messages[0].state, "ready");
  assert.equal(handler.activities.at(-1).key, "assistant.run.inconclusive");
});

test("ordinary capability questions display the model reply without task metadata, confirmation or a second request", async () => {
  const reply = "我可以查看服务器状态、分析日志，并按你的要求修改配置。需要执行修改时会先展示具体内容。";
  const handler = loadHandler({ handled: false, action: "none", message: reply,
    task: null, requiresConfirmation: false, verification: null, followUp: null });
  await handler.run({ promptLabel: "你能干啥", prompt: "你能干啥", contextPayload: "Current UI context",
    userMessage: { id: "capabilities-question", role: "user", content: "你能干啥", state: "ready" } });
  assert.equal(handler.requests.length, 1);
  assert.equal(handler.requests[0].prompt, "你能干啥");
  assert.deepEqual(JSON.parse(handler.requests[0].context), { interfaceLanguage: "zh-CN" });
  assert.ok(!JSON.stringify(handler.requests[0]).includes("Current UI context"));
  assert.deepEqual(handler.unexpected, []);
  assert.equal(handler.messages.length, 1);
  assert.equal(handler.messages[0].content, reply);
  assert.equal(handler.messages[0].meta, undefined);
  assert.equal(handler.messages[0].state, "ready");
  assert.equal(handler.states.at(-1).error, null);
  assert.doesNotMatch(handler.messages[0].content, /任务|missing required fields|Task inconclusive/);
});
