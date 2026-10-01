const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

function loadWorkflow() {
  const filename = path.join(__dirname, "../src/assistant-workflow.ts");
  const module = { exports: {} };
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), { module, exports: module.exports, require }, { filename });
  return module.exports;
}

function requirement(overrides = {}) {
  return { id: "requirement_1", kind: "setting", description: "Limit the server to four players",
    sourceText: "最多4人", target: "max_players", expectedDisplay: "4", ...overrides };
}

function task(overrides = {}) {
  return { id: "requirements-task", goal: "launch_service", preserveExistingMods: true,
    instanceId: "fixture-server", moduleId: "dontstarve", operationLimit: 6,
    requirements: [requirement()], status: "proposed", checks: [], ...overrides };
}

function operation(receipt, overrides = {}) {
  return { conversationId: "fixture-conversation", conversationRevision: 1, handled: true, action: "customize_config", message: "Configuration result.",
    task: receipt, requiresConfirmation: false, instanceId: receipt.instanceId, moduleId: receipt.moduleId, ...overrides };
}

function preview(receipt) {
  return operation(receipt, { requiresConfirmation: true, confirmationToken: "fixture-confirmation",
    planSummary: "Request requirements:\n1. Limit the server to four players: max_players = 4\nApply this configuration." });
}

function check(status, overrides = {}) {
  return { name: "requirement_1", status, summary: "The saved setting was read back.",
    evidence: { requirementId: "requirement_1", value: "synthetic-private-fixture" }, ...overrides };
}

test("requirement definitions remain bound while checks may change", async () => {
  const { runAssistantWorkflow } = loadWorkflow();
  for (const changed of [[], [requirement({ target: "another_key" })], [requirement({ expectedDisplay: "20" })],
    [requirement({ sourceText: "different request" })], [requirement({ description: "Different goal" })],
    [requirement({ kind: "port" })], [requirement(), requirement({ id: "requirement_2" })]]) {
    await assert.rejects(runAssistantWorkflow(preview(task()), {
      confirmPreview: () => true,
      executeConfirmed: async () => operation(task({ requirements: changed, status: "completed" })),
      onResult: () => assert.fail("a changed requirement was published"),
    }), /requirement.*changed/i);
  }
  const outcome = await runAssistantWorkflow(preview(task()), {
    confirmPreview: () => true,
    executeConfirmed: async () => operation(task({ status: "completed", checks: [check("satisfied")] })),
    onResult: () => {},
  });
  assert.equal(outcome.status, "completed");
});

test("requirement snapshots cannot be changed through a shared response object", async () => {
  const { runAssistantWorkflow } = loadWorkflow();
  const receipt = task();
  await assert.rejects(runAssistantWorkflow(preview(receipt), {
    confirmPreview: () => true,
    executeConfirmed: async () => {
      receipt.requirements[0].expectedDisplay = "20";
      return operation(receipt);
    },
    onResult: () => assert.fail("a mutated requirement was published"),
  }), /requirement.*changed/i);
});

test("missing or duplicated requirement definitions are rejected before confirmation", async () => {
  const { runAssistantWorkflow } = loadWorkflow();
  for (const requirements of [undefined, null, [requirement(), requirement()], [requirement({ kind: "invented" })]]) {
    await assert.rejects(runAssistantWorkflow(preview(task({ requirements })), {
      confirmPreview: () => assert.fail("invalid requirement contract was displayed"),
      executeConfirmed: () => assert.fail("invalid requirement contract was executed"), onResult: () => {},
    }), /requirement/i);
  }
});

test("all bound requirements need actual checks before a task can report completion", async () => {
  const { runAssistantWorkflow, assistantWorkflowResultMessage, assistantWorkflowExecutionState } = loadWorkflow();
  for (const [requirements, checks, expected] of [
    [[requirement()], [], "inconclusive"],
    [[requirement()], [check("unknown")], "inconclusive"],
    [[requirement()], [check("failed")], "failed"],
    [[requirement()], [check("satisfied"), check("satisfied")], "inconclusive"],
    [[requirement({ kind: "unverified", target: null })], [check("satisfied")], "inconclusive"],
    [[requirement()], [check("satisfied")], "completed"],
  ]) {
    const result = operation(task({ status: "completed", requirements, checks }));
    const outcome = await runAssistantWorkflow(result, {
      confirmPreview: () => assert.fail("no preview"), executeConfirmed: () => assert.fail("no mutation"), onResult: () => {},
    });
    assert.equal(outcome.status, expected);
    assert.equal(assistantWorkflowExecutionState(outcome), expected === "completed" ? "success" : expected === "failed" ? "error" : expected);
    const message = assistantWorkflowResultMessage(result).content;
    assert.equal(message.includes("Server launch checks passed."), expected === "completed");
    assert.doesNotMatch(message, /synthetic-private-fixture/);
  }
});

test("requirement results show names, targets and expected display without raw evidence", () => {
  const { assistantWorkflowResultMessage } = loadWorkflow();
  const receipt = task({ status: "inconclusive", requirements: [
    requirement(), requirement({ id: "requirement_2", kind: "forbidden_action", description: "Do not download", target: "install_server", expectedDisplay: null }),
    requirement({ id: "requirement_3", kind: "unverified", description: "Players can connect", target: null, expectedDisplay: null }),
  ], checks: [check("satisfied"), check("satisfied", { name: "requirement_2" }), check("satisfied", { name: "requirement_3" })] });
  const formatted = assistantWorkflowResultMessage(operation(receipt), (key, fallback) => ({
    "assistant.task.check.satisfied": "已满足", "assistant.task.check.unknown": "尚未确认",
    "assistant.task.requirement.target": "目标", "assistant.task.requirement.expected": "期望",
  })[key] ?? fallback).content;
  assert.match(formatted, /已满足: Limit the server to four players/);
  assert.match(formatted, /目标: max_players/);
  assert.match(formatted, /期望: 4/);
  assert.match(formatted, /Do not download/);
  assert.match(formatted, /install_server/);
  assert.match(formatted, /尚未确认: Players can connect/);
  assert.doesNotMatch(formatted, /已满足: Players can connect|synthetic-private-fixture/);
  assert.equal(formatted.match(/Limit the server to four players/g).length, 1);
});

test("unfinished requirements preserve an inconclusive task during configuration", async () => {
  const { runAssistantWorkflow, assistantWorkflowResultMessage, assistantWorkflowExecutionState } = loadWorkflow();
  const intermediate = operation(task({ status: "inconclusive", checks: [check("failed")] }));
  const outcome = await runAssistantWorkflow(intermediate, {
    confirmPreview: () => assert.fail("no preview"), executeConfirmed: () => assert.fail("no mutation"), onResult: () => {},
  });
  assert.equal(outcome.status, "inconclusive");
  assert.equal(assistantWorkflowExecutionState(outcome), "inconclusive");
  const message = assistantWorkflowResultMessage(intermediate);
  assert.equal(message.state, "ready");
  assert.match(message.content, /Task inconclusive/);
  assert.match(message.content, /Not satisfied: Limit the server to four players/);
  assert.doesNotMatch(message.content, /Task failed|Server launch checks passed/);
});

test("declining the first summary rejects the entire requirement proposal", async () => {
  const { runAssistantWorkflow } = loadWorkflow();
  const proposed = preview(task());
  const outcome = await runAssistantWorkflow(proposed, {
    confirmPreview: (shown) => {
      assert.equal(shown.planSummary, proposed.planSummary);
      assert.equal(shown.task.requirements[0].expectedDisplay, "4");
      return false;
    },
    executeConfirmed: () => assert.fail("declined requirement must not execute"), onResult: () => assert.fail("nothing executed"),
  });
  assert.equal(outcome.status, "cancelled");
});
