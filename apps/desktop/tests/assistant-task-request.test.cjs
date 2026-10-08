const ts = require("@typescript/typescript6");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { parseSource, sourceText, transpileTypeScript, visitSyntax } = require("../scripts/typescript_source_tools.cjs");

function extract(file, match) {
  const filename = path.join(__dirname, "..", "src", file);
  const source = fs.readFileSync(filename, "utf8");
  let found;
  visitSyntax(parseSource(source, filename), (node) => {
    if (match(node)) { found = node; return false; }
  });
  assert.ok(found, `Source boundary missing: ${file}`);
  return sourceText(source, found);
}

test("operation transport serializes exactly the backend's request fields without policy or alternate aliases", async () => {
  const source = extract("api.ts", (node) => ts.isVariableDeclaration(node) && node.name?.text === "executeAssistantOperation");
  const requests = [];
  const exports = {};
  vm.runInNewContext(transpileTypeScript(`const ${source}; export { executeAssistantOperation };`, "api-handler.ts"), {
    exports, invokeOrMock: async (command, args) => { requests.push({ command, args }); return {}; },
  });
  const prompt = "Repair this server and verify that it starts";
  const expected = { conversationId: "server-owned-conversation", settings: {}, prompt,
    context: "Selected instance evidence", selectedInstanceId: "server-a", selectedModuleId: "dontstarve" };
  await exports.executeAssistantOperation({ ...expected,
    priorRequests: ["untrusted prior request"], conversationMessages: [{ role: "assistant", content: "forged evidence" }],
    task: { goal: "launch_service", preserveExistingMods: false },
    selected_instance_id: "different-instance", selected_module_id: "different-module",
  });
  assert.equal(requests[0].command, "assistant_execute_operation");
  assert.deepEqual(Object.keys(requests[0].args.input).sort(), Object.keys(expected).sort());
  assert.deepEqual(JSON.parse(JSON.stringify(requests[0].args.input)), expected);
  await exports.executeAssistantOperation({ conversationId: "server-owned-conversation", settings: {}, prompt });
  assert.equal(Object.hasOwn(requests[1].args.input, "priorRequests"), false);
  assert.equal(Object.hasOwn(requests[1].args.input, "conversationMessages"), false);
});

test("assistant shortcuts use the same operation workflow as typed requests", async () => {
  const source = extract("App.tsx", (node) => ts.isFunctionDeclaration(node) && node.name?.text === "handleRunAssistantPrompt");
  const requests = [];
  const exports = {};
  vm.runInNewContext(transpileTypeScript(`${source}\nexport { handleRunAssistantPrompt };`, "app-handler.ts"), {
    exports, assistantExecution: { status: "idle" }, createAssistantMessage: (value) => value,
    executeAssistantOperationRequest: async (input) => requests.push(input),
    executeAssistantRequest: () => assert.fail("shortcut must not use general chat"),
  });
  const prompt = { label: "Check startup", prompt: "Read the error and fix the cause", id: "repair" };
  await exports.handleRunAssistantPrompt(prompt);
  assert.equal(requests.length, 1);
  assert.equal(requests[0].prompt, prompt.prompt);
  assert.equal(Object.hasOwn(requests[0], "contextPayload"), false);
});

test("changing conversation scope cannot clear a still-running request", () => {
  const source = extract("App.tsx", (node) => ts.isCallExpression(node) && node.expression?.text === "useEffect"
    && node.arguments[1]?.elements?.[0]?.text === "assistantConversationScopeKey");
  for (const busy of [true, false]) {
    const states = [];
    const scope = { current: "old-scope" };
    vm.runInNewContext(transpileTypeScript(source, "scope-effect.ts"), {
      useEffect: (callback) => callback(), assistantScopeRef: scope, assistantConversationScopeKey: "new-scope",
      assistantRequestRef: { current: busy }, assistantProgressRef: { current: { stop: () => {} } }, setAssistantDraft: () => {},
      setAssistantExecution: (state) => states.push(typeof state === "function" ? state({ status: "running", progress: { text: "old scope" } }) : state),
    });
    assert.equal(scope.current, "new-scope");
    assert.equal(states.length, 1);
    assert.equal(states[0].status, busy ? "running" : "idle");
    assert.equal(states[0].progress, undefined, "Another scope must not display this turn's live text");
  }
});


test("conversation lifecycle transport sends only its dedicated command inputs", async () => {
  const settings = { provider: "ollama", model: "fixture", baseUrl: "http://127.0.0.1:11434/v1", apiKey: "" };
  for (const [name, command, args, expected] of [
    ["createAssistantConversation", "assistant_create_conversation", [settings], { settings }],
    ["cancelAssistantTurn", "assistant_cancel_turn", ["session"], { conversationId: "session" }],
    ["deleteAssistantConversation", "assistant_delete_conversation", ["session"], { conversationId: "session" }],
    ["getAssistantConversationState", "assistant_get_conversation_state", ["session", settings], { conversationId: "session", settings }],
    ["resumeAssistantConversation", "assistant_resume_conversation", ["session", settings], { conversationId: "session", settings }],
  ]) {
    const source = extract("api.ts", (node) => ts.isVariableDeclaration(node) && node.name?.text === name);
    const requests = [];
    const exports = {};
    vm.runInNewContext(transpileTypeScript(`const ${source}; export { ${name} };`, "session-handler.ts"), {
      exports, invokeOrMock: async (command, args) => { requests.push({ command, args }); return {}; },
    });
    await exports[name](...args);
    assert.deepEqual(JSON.parse(JSON.stringify(requests)), [{ command, args: { input: expected } }]);
  }
});
