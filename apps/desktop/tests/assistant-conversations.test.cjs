const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

function loadConversationCore(context = {}) {
  const sourcePath = path.join(__dirname, "..", "src", "assistant-conversations.ts");
  const source = fs.readFileSync(sourcePath, "utf8");
  const transpiled = transpileTypeScript(source, sourcePath);
  const module = { exports: {} };
  vm.runInNewContext(transpiled, {
    module,
    exports: module.exports,
    require,
    URL,
    TextEncoder,
    encodeURIComponent,
    ...context
  }, { filename: sourcePath });
  return module.exports;
}

const conversations = loadConversationCore();

function plain(value) {
  return JSON.parse(JSON.stringify(value));
}

function message(id, role, content, label = null) {
  return { id, role, content, label, state: "ready" };
}

test("assistant conversations create a titled session on the first user turn", () => {
  const initial = conversations.createEmptyAssistantConversationStore();
  const turn = conversations.beginAssistantConversationTurn(
    initial,
    "servers|instance-a",
    message("user-1", "user", "帮我检查服务器为什么无法启动", "检查启动问题"),
    1_000,
    () => "conversation-a"
  );

  assert.equal(turn.conversationId, "conversation-a");
  assert.equal(turn.messages.length, 1);
  assert.equal(turn.store.activeConversationByScope["servers|instance-a"], "conversation-a");
  assert.equal(turn.store.conversations[0].title, "检查启动问题");
});

test("assistant conversations preserve history across new, switch, and delete actions", () => {
  const first = conversations.beginAssistantConversationTurn(
    conversations.createEmptyAssistantConversationStore(),
    "servers|instance-a",
    message("user-1", "user", "第一段对话"),
    1_000,
    () => "conversation-a"
  );
  let store = conversations.appendAssistantConversationMessage(
    first.store,
    first.conversationId,
    message("assistant-1", "assistant", "第一段回复"),
    1_100
  );

  store = conversations.startNewAssistantConversation(store, "servers|instance-a");
  const second = conversations.beginAssistantConversationTurn(
    store,
    "servers|instance-a",
    message("user-2", "user", "第二段对话"),
    2_000,
    () => "conversation-b"
  );
  store = second.store;

  assert.deepEqual(
    plain(conversations.listAssistantConversationSummaries(store, "servers|instance-a").map((item) => item.id)),
    ["conversation-b", "conversation-a"]
  );

  store = conversations.selectAssistantConversation(store, "servers|instance-a", "conversation-a");
  assert.equal(store.activeConversationByScope["servers|instance-a"], "conversation-a");

  store = conversations.deleteAssistantConversation(store, "servers|instance-a", "conversation-a");
  assert.equal(store.activeConversationByScope["servers|instance-a"], "conversation-b");
  assert.equal(store.conversations.some((item) => item.id === "conversation-a"), false);
});

test("assistant conversation scopes and normalized data remain isolated", () => {
  const first = conversations.beginAssistantConversationTurn(
    conversations.createEmptyAssistantConversationStore(),
    "library|module-a",
    message("user-1", "user", "模块问题"),
    1_000,
    () => "conversation-a"
  );
  const second = conversations.beginAssistantConversationTurn(
    first.store,
    "servers|instance-a",
    message("user-2", "user", "实例问题"),
    2_000,
    () => "conversation-b"
  );
  const normalized = conversations.normalizeAssistantConversationStore({
    ...second.store,
    conversations: [
      ...second.store.conversations,
      { id: 42, scopeKey: null, messages: "invalid" }
    ]
  });

  assert.equal(conversations.listAssistantConversationSummaries(normalized, "library|module-a").length, 1);
  assert.equal(conversations.listAssistantConversationSummaries(normalized, "servers|instance-a").length, 1);
  assert.equal(normalized.conversations.length, 2);
});

test("assistant conversations clear the insecure persisted store without reading or writing it", () => {
  assert.doesNotThrow(() => conversations.clearInsecureAssistantConversationStorage());

  const stored = new Map([
    ["langame.assistant-conversations.v1:local", "plaintext history"]
  ]);
  const removedKeys = [];
  const isolatedConversations = loadConversationCore({
    localStorage: {
      getItem() {
        throw new Error("conversation cleanup must not read persisted plaintext");
      },
      setItem() {
        throw new Error("conversation cleanup must not persist plaintext");
      },
      removeItem(key) {
        removedKeys.push(key);
        stored.delete(key);
      }
    }
  });

  assert.equal(isolatedConversations.loadAssistantConversationStore, undefined);
  assert.equal(isolatedConversations.persistAssistantConversationStore, undefined);
  isolatedConversations.clearInsecureAssistantConversationStorage();
  isolatedConversations.clearInsecureAssistantConversationStorage();

  assert.equal(stored.has("langame.assistant-conversations.v1:local"), false);
  assert.deepEqual(removedKeys, [
    "langame.assistant-conversations.v1:local",
    "langame.assistant-conversations.v1:local"
  ]);
});
