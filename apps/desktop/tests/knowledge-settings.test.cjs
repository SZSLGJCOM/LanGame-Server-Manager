const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("knowledge IPC preserves settings and exact cancellation identity without LAN fallback", async () => {
  let local = true;
  const calls = [];
  const module = { exports: {} };
  const file = "api-knowledge.ts";
  const source = fs.readFileSync(path.join(__dirname, "../src", file), "utf8");
  vm.runInNewContext(transpileTypeScript(source, file), { module, exports: module.exports, require: (name) => {
    if (name === "@tauri-apps/api/core") return { isTauri: () => local };
    if (name === "./api-transport") return { shouldUseLanApi: () => true, invokeOrMock: async (command, args) => calls.push({ command, args }) };
    throw new Error(`Unexpected dependency: ${name}`);
  } });
  const { knowledgeApi } = module.exports;
  await knowledgeApi.status();
  await knowledgeApi.save({ autoUpdate: false, intervalHours: 72 });
  await knowledgeApi.start(null);
  await knowledgeApi.start("minecraft");
  await knowledgeApi.cancel("precise-job-id");
  assert.deepEqual(JSON.parse(JSON.stringify(calls)), [
    { command: "read_knowledge_status" },
    { command: "update_knowledge_settings", args: { input: { autoUpdate: false, intervalHours: 72 } } },
    { command: "start_knowledge_sync", args: { input: { moduleId: null, force: true } } },
    { command: "start_knowledge_sync", args: { input: { moduleId: "minecraft", force: true } } },
    { command: "cancel_knowledge_sync", args: { input: { jobId: "precise-job-id" } } }
  ]);
  local = false;
  assert.equal(knowledgeApi.available(), false);
  await assert.rejects(knowledgeApi.status, /desktop host/);
  await assert.rejects(knowledgeApi.start, /desktop host/);
  await assert.rejects(() => knowledgeApi.save({ autoUpdate: true, intervalHours: 24 }), /desktop host/);
  await assert.rejects(() => knowledgeApi.cancel("another-id"), /desktop host/);
  assert.equal(calls.length, 5);
});

test("knowledge settings render real controls, cancel the bound task and retain old-index evidence", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ fixturePath: "knowledge-settings-browser.html", screenshotPath: process.env.LANGAME_KNOWLEDGE_SCREENSHOT });
  assert.equal(report.status, "passed", JSON.stringify(report));
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.checks, 12);
  assert.equal(report.browser_exited, true);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
  console.log(`KNOWLEDGE_BROWSER ${JSON.stringify(report)}`);
});
