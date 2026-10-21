const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { parseSource, sourceText, transpileTypeScript, visitSyntax } = require("../scripts/typescript_source_tools.cjs");

const filename = path.resolve(__dirname, "../src/App.tsx");
const source = fs.readFileSync(filename, "utf8");
let declaration;
visitSyntax(parseSource(source, filename), (node) => {
  if (node.name?.text === "maybeRunLifecycleBroadcast" && node.body) declaration = sourceText(source, node);
});
assert.ok(declaration, "test the production lifecycle broadcast entry point");
const createRunner = new Function("dependencies", `
  const { assistantCanRun, readInstanceBroadcastPolicy, listInstanceBroadcastEvents,
    broadcastPolicyAuditSnapshot, generateInstanceBroadcast, automaticBroadcastIntent,
    aiSettings, sendInstanceBroadcast, logFrontendEvent, describeError } = dependencies;
  ${transpileTypeScript(declaration, "lifecycle-broadcast.ts")}
  return maybeRunLifecycleBroadcast;
`);

function harness(events, cooldown = 10) {
  const calls = [];
  const run = createRunner({
    assistantCanRun: true,
    readInstanceBroadcastPolicy: async () => ({ enabled: true, rules: {
      startup: { enabled: true }, shutdown: { enabled: true }, cooldown_minutes: cooldown
    } }),
    listInstanceBroadcastEvents: async () => events,
    broadcastPolicyAuditSnapshot: JSON.stringify,
    automaticBroadcastIntent: () => "fixture intent", aiSettings: {},
    generateInstanceBroadcast: async () => { calls.push("generate"); return { message: "hello" }; },
    sendInstanceBroadcast: async () => { calls.push("send"); return { event: { status: "pending" } }; },
    logFrontendEvent: () => { calls.push("log"); }, describeError: String
  });
  return { run, calls };
}

test("lifecycle pending submissions count toward the existing cooldown", async () => {
  for (const cooldown of [1, 10]) {
    for (const event of [
      { source: "startup", status: "pending", created_at_unix_ms: Date.now() },
      { source: "manual", rule_id: "startup", status: "pending", created_at_unix_ms: Date.now() }
    ]) {
      const state = harness([event], cooldown);
      await state.run("server-a", "startup");
      assert.deepEqual(state.calls, []);
    }
  }
});

test("lifecycle retains sent cooldown and independently handles the other rule", async () => {
  for (const [event, expected] of [
    [{ source: "startup", status: "sent", created_at_unix_ms: Date.now() }, []],
    [{ source: "startup", status: "sent", created_at_unix_ms: 0 }, ["generate", "send"]],
    [{ source: "startup", status: "pending", created_at_unix_ms: 0 }, ["generate", "send"]],
    [{ source: "shutdown", status: "pending", created_at_unix_ms: 0 }, ["generate", "send"]],
    [{ source: "startup", status: "failed", created_at_unix_ms: 0 }, ["generate", "send"]]
  ]) {
    const state = harness([event]);
    await state.run("server-a", "startup");
    assert.deepEqual(state.calls, expected);
  }
  const zeroCooldown = harness([{ source: "startup", status: "pending", created_at_unix_ms: Date.now() }], 0);
  await zeroCooldown.run("server-a", "startup");
  assert.deepEqual(zeroCooldown.calls, ["generate", "send"], "zero cooldown still permits a new lifecycle operation");
});
