const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

require.extensions[".ts"] = function(module, filename) {
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
};
const { executeGmCommandBatch, gmExecutionText } = require("../src/views/servers/gm-tool-execution.ts");
const preview = { commands: ["one", "two", "three"], processKey: "master", dispatchOptions: { transport: "stdin" } };
const resultFor = (command, pending = false) => ({ instance_id: "a", process_key: "master", display_name: "Master", pid: 1,
  command, response_text: `reply to ${command}`, write_confirmation_pending: pending, submitted_at_unix_ms: 1 });

test("GM batch preserves every native response in order without interpreting unknown output as success", async () => {
  const calls = [];
  const result = await executeGmCommandBatch("a", preview, async (...args) => {
    calls.push(args);
    return { ...resultFor(args[1]), response_text: args[1] === "two" ? "Unknown command" : `reply to ${args[1]}` };
  }, new AbortController().signal, () => {}, String);
  assert.equal(result.responses.length, 3);
  assert.equal(result.responses[1].response_text, "Unknown command");
  assert.deepEqual(calls.map((call) => call.slice(0, 3)), [["a", "one", "master"], ["a", "two", "master"], ["a", "three", "master"]]);
  assert.match(gmExecutionText("en-US", { ...result, sending: false }), /^Sent 3\/3 commands\. Check/);
});

test("GM pending stdin write stops the batch without an automatic retry", async () => {
  let calls = 0;
  const result = await executeGmCommandBatch("a", preview, async (_, command) => {
    calls++;
    return resultFor(command, true);
  }, new AbortController().signal, () => {}, String);
  assert.equal(calls, 1);
  assert.equal(result.responses.length, 1);
  assert.equal(result.responses[0].write_confirmation_pending, true);
  assert.match(gmExecutionText("zh-CN", { ...result, sending: false }), /仍等待写入确认/);
});

test("GM transport failure retains prior responses and never dispatches remaining commands", async () => {
  let calls = 0;
  const result = await executeGmCommandBatch("a", preview, async (_, command) => {
    if (++calls === 2) throw new Error("connection lost");
    return resultFor(command);
  }, new AbortController().signal, () => {}, (error) => error.message);
  assert.equal(calls, 2);
  assert.equal(result.responses[0].command, "one");
  assert.equal(result.error, "connection lost");
  assert.match(gmExecutionText("en-US", { ...result, sending: false }), /Sent 1\/3; command 2 was not confirmed/);
});

test("leaving a GM instance stops unsent mutations after the in-flight response returns", async () => {
  const controller = new AbortController();
  let calls = 0;
  const result = await executeGmCommandBatch("a", preview, async (_, command) => {
    calls++;
    controller.abort();
    return resultFor(command);
  }, controller.signal, () => {}, String);
  assert.equal(calls, 1);
  assert.equal(result.responses[0].command, "one");
  assert.equal(result.stopped, true);
});
