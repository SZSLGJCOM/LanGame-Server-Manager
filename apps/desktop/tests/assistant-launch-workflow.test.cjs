const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

function loadWorkflow() {
  const filename = path.join(__dirname, "..", "src", "assistant-workflow.ts");
  const module = { exports: {} };
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), { module, exports: module.exports, require }, { filename });
  return module.exports;
}

function receipt(overrides = {}) {
  return { id: "launch-task", goal: "launch_service", operationLimit: 32, preserveExistingMods: true,
    instanceId: null, moduleId: "dontstarve", status: "proposed", requirements: [], checks: [], ...overrides };
}

function operation(action, task, overrides = {}) {
  return { conversationId: "fixture-conversation", conversationRevision: 1, handled: true, action, task, instanceId: task.instanceId, moduleId: task.moduleId,
    requiresConfirmation: false, message: `Result of ${action}`, ...overrides };
}

function preview(action, task, token = action) {
  return operation(action, task, { requiresConfirmation: true, confirmationToken: token, planSummary: `Confirm ${token}` });
}

function continuation() {
  return { status: "inconclusive", summary: "More confirmed work is needed.", canContinue: true, runId: null, evidence: {} };
}

test("launch confirms install, validation, creation and startup separately and reports completion only after verified startup", async () => {
  const { runAssistantWorkflow, assistantWorkflowResultMessage } = loadWorkflow();
  const actions = ["install_server", "validate_server", "create_server", "customize_config", "repair_ports", "start_server"];
  let task = receipt();
  const confirmed = [];
  const published = [];
  const outcome = await runAssistantWorkflow(preview(actions[0], task), {
    confirmPreview: (step) => { confirmed.push(step.action); return true; },
    executeConfirmed: async ({ confirmationToken }) => {
      const index = actions.indexOf(confirmationToken);
      if (confirmationToken === "create_server") task = receipt({ instanceId: "created-server" });
      if (index === actions.length - 1) return operation(confirmationToken, { ...task, status: "completed" }, {
        verification: { ...continuation(), status: "verified", canContinue: false },
      });
      return operation(confirmationToken, { ...task, status: "inconclusive" }, {
        verification: continuation(), followUp: preview(actions[index + 1], task),
      });
    },
    onResult: (step) => published.push(step.action),
  });
  assert.deepEqual(confirmed, actions);
  assert.deepEqual(published, actions);
  assert.equal(outcome.status, "completed");
  assert.equal(outcome.completedSteps, 6);
  assert.match(assistantWorkflowResultMessage(outcome.lastOperation).content, /^Server launch checks passed\./);
});

test("the frontend failsafe blocks a thirty-third confirmation and cancels pending work", async () => {
  const { runAssistantWorkflow } = loadWorkflow();
  const task = receipt({ instanceId: "existing-server" });
  let executions = 0;
  let cancelled = 0;
  const outcome = await runAssistantWorkflow(preview("start_server", task, "1"), {
    confirmPreview: () => true,
    executeConfirmed: async () => {
      executions += 1;
      return operation("start_server", { ...task, status: "inconclusive" }, {
        verification: continuation(), followUp: preview("start_server", task, String(executions + 1)),
      });
    }, onResult: () => {}, cancelPending: async () => { cancelled += 1; },
  });
  assert.equal(executions, 32);
  assert.equal(cancelled, 1);
  assert.equal(outcome.status, "limit-reached");
});

test("backend operation counters do not replace completion evidence", async () => {
  const { runAssistantWorkflow } = loadWorkflow();
  const outcome = await runAssistantWorkflow(preview("install_server", receipt({ operationLimit: 3 })), {
    confirmPreview: () => true,
    executeConfirmed: async () => operation("install_server", receipt({ operationLimit: 32, status: "inconclusive" })),
    onResult: () => {},
  });
  assert.equal(outcome.completedSteps, 1);
  assert.equal(outcome.status, "inconclusive");
});

test("only the confirmed first creation result may bind a new instance", async () => {
  const { runAssistantWorkflow } = loadWorkflow();
  for (const action of ["install_server", "validate_server", "start_server"]) {
    await assert.rejects(runAssistantWorkflow(preview(action, receipt()), {
      confirmPreview: () => true,
      executeConfirmed: async () => operation(action, receipt({ instanceId: "unexpected" })),
      onResult: () => assert.fail("unrelated result bound the target"),
    }), /binding/i);
  }
  await assert.rejects(runAssistantWorkflow(preview("create_server", receipt()), {
    confirmPreview: () => true,
    executeConfirmed: async () => operation("create_server", receipt({ instanceId: "created" }), { instanceId: "different" }),
    onResult: () => assert.fail("mismatched creation result was published"),
  }), /binding/i);
  let requests = 0;
  await assert.rejects(runAssistantWorkflow(preview("create_server", receipt()), {
    confirmPreview: () => true,
    executeConfirmed: async () => {
      requests += 1;
      return operation("create_server", receipt({ instanceId: "created" }), {
        verification: continuation(), followUp: preview("start_server", receipt({ instanceId: "redirected" })),
      });
    }, onResult: () => {},
  }), /binding/i);
  assert.equal(requests, 1);
});
