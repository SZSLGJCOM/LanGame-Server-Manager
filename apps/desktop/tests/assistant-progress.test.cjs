const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

function load(name, globals = {}) {
  const filename = path.join(__dirname, "..", "src", name);
  const module = { exports: {} };
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename),
    { module, exports: module.exports, require, ...globals }, { filename });
  return module.exports;
}
function state(overrides = {}) {
  return { conversationId: "a", status: "running", revision: 2,
    progress: { revision: 2, cursor: 1, reset: false, text: "检查", events: [] }, ...overrides };
}
const flush = () => new Promise(setImmediate);

test("progress uses bounded snapshots to recover lost deltas without replaying tools", async () => {
  const timers = [];
  const { AssistantProgressPoller } = load("assistant-progress.ts", { setTimeout: (fn) => timers.push(fn), clearTimeout: () => {} });
  const snapshots = [state({ progress: { revision: 2, cursor: 2, reset: true, text: "检查", events: [
    { cursor: 1, kind: "tool_started", text: "", toolName: "read_logs" },
    { cursor: 2, kind: "text_delta", text: "检查", toolName: null },
  ] } }), state({ progress: { revision: 2, cursor: 20, reset: true, text: "检查已完成", events: [
    { cursor: 20, kind: "tool_completed", text: "", toolName: "read_logs" },
  ] } })];
  const published = [], cursors = [];
  const poller = new AssistantProgressPoller("a", 1, async (cursor) => { cursors.push(cursor); return snapshots.shift(); }, (value) => published.push(value), () => true);
  poller.start(); await flush(); timers.shift()(); await flush(); poller.stop();
  assert.equal(published.at(-1).text, "检查已完成");
  assert.equal(published.at(-1).tools.length, 1);
  assert.equal(published.at(-1).tools[0].status, "completed");
  assert.deepEqual(cursors, [undefined, 2]);
});

test("a cursor gap rebuilds tools and phase when a completion is no longer retained", async () => {
  const timers = [];
  const { AssistantProgressPoller } = load("assistant-progress.ts", { setTimeout: (fn) => timers.push(fn), clearTimeout: () => {} });
  const snapshots = [state({ progress: { revision: 2, cursor: 3, reset: true, text: "正在检查", events: [
    { cursor: 1, kind: "phase", text: "investigating", toolName: null },
    { cursor: 2, kind: "tool_started", text: "", toolName: "read_runtime" },
    { cursor: 3, kind: "text_delta", text: "正在检查", toolName: null },
  ] } }), state({ progress: { revision: 2, cursor: 300, reset: true, text: "检查完成", events: [
    // read_runtime completed at cursor 4; that event has left the bounded queue.
    { cursor: 298, kind: "tool_started", text: "", toolName: "read_instance_file" },
    { cursor: 299, kind: "tool_completed", text: "", toolName: "read_instance_file" },
    { cursor: 300, kind: "text_delta", text: "检查完成", toolName: null },
  ] } })];
  const published = [];
  const poller = new AssistantProgressPoller("a", 1, async () => snapshots.shift(), (value) => published.push(value), () => true);
  poller.start(); await flush();
  assert.equal(published.at(-1).tools[0].status, "running");
  assert.equal(published.at(-1).phase, "investigating");
  timers.shift()(); await flush(); poller.stop();
  const recovered = published.at(-1);
  assert.equal(recovered.text, "检查完成");
  assert.equal(recovered.phase, "");
  assert.equal(recovered.tools.length, 1);
  assert.equal(recovered.tools[0].name, "read_instance_file");
  assert.equal(recovered.tools[0].status, "completed");
  assert.equal(recovered.connectionIssue, false);
});

for (const reason of ["stop", "scope"]) {
  test(`late progress is discarded after ${reason}`, async () => {
    const { AssistantProgressPoller } = load("assistant-progress.ts", { setTimeout: () => assert.fail("No polling after invalidation"), clearTimeout: () => {} });
    let resolve, current = true;
    const poller = new AssistantProgressPoller("a", 1, () => new Promise((done) => { resolve = done; }), () => assert.fail("Stale progress was published"), () => current);
    poller.start();
    if (reason === "stop") poller.stop(); else current = false;
    resolve(state()); await flush();
  });
}

test("wrong conversation and malformed events never become displayed text", async () => {
  for (const response of [state({ conversationId: "other" }), state({ progress: { revision: 2, cursor: 1, reset: false, text: "injected", events: [{ cursor: 1, kind: "thinking", text: "private", toolName: null }] } })]) {
    const { AssistantProgressPoller } = load("assistant-progress.ts", { setTimeout: () => 1, clearTimeout: () => {} });
    let last;
    const poller = new AssistantProgressPoller("a", 1, async () => response, (value) => { last = value; }, () => true);
    poller.start(); await flush(); poller.stop();
    assert.equal(last.connectionIssue, true);
    assert.equal(last.text, "");
  }
});

test("restoring a saved conversation never replaces a newer display turn or sends history", () => {
  const model = load("assistant-conversations.ts");
  const saved = { conversationId: "backend", revision: 4, title: "恢复任务", updatedAtUnixMs: 123 };
  const response = { ...state({ conversationId: "backend", status: "paused" }), continuation: { reason: "model_slice", summary: "Saved", canResume: true }, messages: [{ role: "assistant", content: "实际保留的记录" }] };
  let store = model.restoreAssistantConversation(model.createEmptyAssistantConversationStore(), "scope", "provider", saved, response);
  assert.equal(store.conversations[0].messages[0].content, "实际保留的记录");
  store = model.selectAssistantConversation(store, "scope", "restored-backend");
  const turn = model.beginAssistantConversationTurn(store, "scope", { id: "new", role: "user", content: "继续" }, 456, () => "unused", "provider");
  const restoredAgain = model.restoreAssistantConversation(turn.store, "scope", "provider", saved, response);
  assert.equal(restoredAgain.conversations[0].messages.at(-1).content, "继续");
  assert.equal(turn.backendConversationId, "backend");
});
