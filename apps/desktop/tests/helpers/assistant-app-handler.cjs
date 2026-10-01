const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const { parseSource, sourceText, transpileTypeScript, visitSyntax } = require("../../scripts/typescript_source_tools.cjs");

function loadWorkflow(file = "assistant-workflow.ts") {
  const filename = path.join(__dirname, "..", "..", "src", file);
  const module = { exports: {} };
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), {
    module, exports: module.exports, require, TextEncoder, setTimeout, clearTimeout,
  }, { filename });
  return module.exports;
}

function loadAppHandler(initial, responses, decisions, translations = {}, options = {}) {
  const filename = path.join(__dirname, "..", "..", "src", "App.tsx");
  const source = fs.readFileSync(filename, "utf8");
  const declarations = [];
  visitSyntax(parseSource(source, filename), (node) => {
    if (node.type === "FunctionDeclaration" && ["executeAssistantOperationRequest", "handleAssistantResume", "handleAssistantStop", "handleDeleteAssistantConversation"].includes(node.identifier?.value)) {
      declarations.push(node);
      return false;
    }
  });
  assert.equal(declarations.length, 4);
  const exports = {};
  const messages = [];
  const states = [];
  const confirmations = [];
  const refreshes = [];
  const navigation = [];
  const requests = [];
  const context = {
    assistantConversationStore: loadWorkflow("assistant-conversations.ts"),
    exports, ...loadWorkflow(), ...loadWorkflow("assistant-conversations.ts"), ...loadWorkflow("assistant-turn-control.ts"), ...loadWorkflow("assistant-progress.ts"), assistantCanRun: true, locale: "en",
    assistantTurnRef: { current: null },
    assistantProgressRef: { current: null },
    createAssistantConversation: async () => ({ conversationId: "fixture-conversation", revision: 0 }),
    getAssistantConversationState: async (conversationId) => ({ conversationId, status: "idle", revision: 1, continuation: null }),
    cancelAssistantTurn: async (conversationId) => ({ conversationId, stopping: false }),
    assistantRequestRef: { current: false }, assistantConversationScopeKey: "original-scope", assistantScopeRef: { current: "original-scope" },
    aiSettings: { provider: "ollama", model: "fixture", baseUrl: "http://127.0.0.1:11434/v1", apiKey: "" },
    selectedInstanceId: "fixture-server", selectedModuleId: "dontstarve",
    t: (key, params = {}, fallback = key) => (translations[key] ?? fallback).replace(/\{(\w+)\}/g, (_, name) => params[name] ?? ""),
    message: (key, params) => ({ key, params }), describeError: (error) => error.message,
    formatDesktopError: (_t, error) => error.message,
    formatAiProviderLabel: () => "Ollama",
    createAssistantMessage: (value) => value,
    assistantConversations: {
      beginTurn: (value) => ({ conversationId: "fixture", backendConversationId: "fixture-conversation", messages: [value] }),
      bindBackend: () => {},
      setContinuation: () => {},
      setRecoveryPending: () => {},
      markUnavailable: () => {},
      appendMessage: (_, value) => messages.push(value),
    },
    setAssistantExecution: (value) => states.push(typeof value === "function" ? value(states.at(-1)) : value), setActivity: () => {},
    executeAssistantOperation: async (input) => { requests.push(input); return typeof initial === "function" ? initial() : initial; },
    confirmAssistantOperation: async (binding) => { confirmations.push(binding.confirmationToken); return responses.shift(); },
    reloadBootstrap: async (preferred, options) => { refreshes.push({ preferred, options }); },
    markRuntimeRefreshed: () => {},
    openInstanceView: (...args) => navigation.push({ type: "instance", args }),
    openLibraryDetail: (...args) => navigation.push({ type: "module", args }),
    assistantConfirmation: { respond: () => {}, confirmPreview: async () => decisions.shift() },
    ...options,
  };
  vm.runInNewContext(transpileTypeScript(`${declarations.map((node) => sourceText(source, node)).join("\n")}\nexport { executeAssistantOperationRequest, handleAssistantResume, handleAssistantStop, handleDeleteAssistantConversation };`, filename), context, { filename });
  return { resume: exports.handleAssistantResume, stop: exports.handleAssistantStop, remove: exports.handleDeleteAssistantConversation, run: (overrides = {}) => exports.executeAssistantOperationRequest({ promptLabel: "Repair", prompt: "Repair", contextPayload: "", userMessage: { id: "current-user", role: "user", content: "Repair" }, ...overrides }), messages, states, confirmations, refreshes, navigation, requests, context };
}

module.exports = { loadAppHandler };
