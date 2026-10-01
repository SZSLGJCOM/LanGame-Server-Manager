const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

function load(name) {
  const filename = path.join(__dirname, "..", "src", name);
  const module = { exports: {} };
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename),
    { module, exports: module.exports, require }, { filename });
  return module.exports;
}
const { AssistantTurnControl, validateAssistantConversationState } = load("assistant-turn-control.ts");
const store = load("assistant-conversations.ts");
const settings = { provider: "ollama", model: "fixture", baseUrl: "http://127.0.0.1:11434", apiKey: "" };
const user = { id: "request", role: "user", content: "你好", state: "ready" };

test("backend binding is reused only for the same provider, model and endpoint", () => {
  const identity = store.assistantConversationProviderIdentity(settings);
  const first = store.beginAssistantConversationTurn(store.createEmptyAssistantConversationStore(), "scope", user, 1, () => "display-a", identity);
  const bound = store.bindAssistantBackendConversation(first.store, first.conversationId, { conversationId: "backend-a", revision: 0 });
  const next = store.beginAssistantConversationTurn(bound, "scope", user, 2, () => "unused", identity);
  assert.equal(next.backendConversationId, "backend-a");
  assert.equal(next.conversationId, "display-a");
  assert.equal(store.assistantConversationProviderIdentity({ ...settings, baseUrl: settings.baseUrl + "/" }), identity);
  for (const changed of [{ provider: "openai-compatible" }, { model: "other" }, { baseUrl: "http://127.0.0.1:9999" }]) {
    const other = store.beginAssistantConversationTurn(bound, "scope", user, 3, () => "display-new", store.assistantConversationProviderIdentity({ ...settings, ...changed }));
    assert.equal(other.conversationId, "display-new");
    assert.equal(other.backendConversationId, null);
    assert.equal(other.messages.length, 1);
  }
});

test("deleting a display conversation prevents late binding or results from resurrecting it", () => {
  const first = store.beginAssistantConversationTurn(store.createEmptyAssistantConversationStore(), "scope", user, 1, () => "display-a");
  let current = store.deleteAssistantConversation(first.store, "scope", first.conversationId);
  current = store.bindAssistantBackendConversation(current, first.conversationId, { conversationId: "backend-a", revision: 1 });
  current = store.appendAssistantConversationMessage(current, first.conversationId, { ...user, role: "assistant", content: "late reply" });
  assert.equal(current.conversations.length, 0);
  assert.equal(current.activeConversationByScope.scope, undefined);
});

test("stop during session creation is delivered once before a model request can start", async () => {
  const cancelled = [];
  const turn = new AssistantTurnControl(async (id) => { cancelled.push(id); return { conversationId: id, stopping: false }; });
  await turn.stop();
  assert.equal(turn.stopRequested, true);
  assert.equal(cancelled.length, 0);
  await turn.bind("backend-a");
  await turn.stop();
  assert.deepEqual(cancelled, ["backend-a"]);
  assert.equal(turn.stopRequested, true, "accepting cancellation must not unlock the pending frontend request");
});

test("failed cancellation remains retryable and cannot claim successful cleanup", async () => {
  let attempts = 0;
  const turn = new AssistantTurnControl(async (id) => {
    if (++attempts === 1) throw new Error("connection failed");
    return { conversationId: id, stopping: true };
  });
  await turn.bind("backend-a");
  await assert.rejects(turn.stop(), /connection failed/);
  assert.equal(turn.stopRequested, false);
  assert.equal((await turn.stop()).stopping, true);
  assert.equal(attempts, 2);
});

test("results must belong to the backend session and cannot move its revision backwards", async () => {
  const turn = new AssistantTurnControl(async (id) => ({ conversationId: id, stopping: false }));
  await turn.bind("backend-a");
  turn.validate({ conversationId: "backend-a", conversationRevision: 2 });
  for (const output of [
    { conversationId: "backend-b", conversationRevision: 3 },
    { conversationId: "backend-a", conversationRevision: 1 },
    { conversationId: null, conversationRevision: null },
  ]) assert.throws(() => turn.validate(output), /stale or different/);
});

test("unavailable conversations remain visible but cannot be reused even when selected again", () => {
  const identity = store.assistantConversationProviderIdentity(settings);
  const first = store.beginAssistantConversationTurn(store.createEmptyAssistantConversationStore(), "scope", user, 1, () => "display-a", identity);
  let current = store.bindAssistantBackendConversation(first.store, first.conversationId, { conversationId: "backend-a", revision: 0 });
  current = store.markAssistantConversationUnavailable(current, first.conversationId);
  current = store.selectAssistantConversation(current, "scope", first.conversationId);
  const next = store.beginAssistantConversationTurn(current, "scope", { ...user, id: "new-request" }, 2, () => "display-new", identity);
  assert.equal(next.conversationId, "display-new");
  assert.equal(next.backendConversationId, null);
  assert.equal(next.store.conversations.find((entry) => entry.id === first.conversationId).messages[0].content, user.content);
});

test("state inspection rejects mismatched IDs, invalid revisions and malformed paused responses", () => {
  const valid = { conversationId: "backend-a", status: "idle", revision: 1, continuation: null };
  validateAssistantConversationState(valid, "backend-a");
  for (const changes of [{ conversationId: "other" }, { status: "other" }, { revision: null }, { revision: -1 },
    { status: "paused" }, { status: "paused", continuation: { summary: "Saved", canResume: "yes" } }]) {
    assert.throws(() => validateAssistantConversationState({ ...valid, ...changes }, "backend-a"), /invalid conversation/);
  }
});
