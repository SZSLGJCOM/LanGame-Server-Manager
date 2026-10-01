const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { parseSource, sourceText, transpileTypeScript, visitSyntax } = require("../scripts/typescript_source_tools.cjs");

const { loadAppHandler } = require("./helpers/assistant-app-handler.cjs");

function loadWorkflow(file = "assistant-workflow.ts") {
  const filename = path.join(__dirname, "..", "src", file);
  const module = { exports: {} };
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), {
    module, exports: module.exports, require, TextEncoder,
  }, { filename });
  return module.exports;
}

function result(overrides = {}) {
  return {
    conversationId: "fixture-conversation", conversationRevision: 1,
    handled: true, action: "customize_config", requiresConfirmation: false,
    message: "Saved configuration.", instanceId: "fixture-server", moduleId: "dontstarve",
    appliedSettingsKeys: [], rejectedSettingsKeys: [], appliedPortNames: [], rejectedPortNames: [],
    workshopItemIds: [], modReferences: [], resolvedModIds: [], sourcePaths: [],
    runtimeCommands: [], runtimeResponseTexts: [], configDocumentCount: 0,
    ...overrides,
  };
}

function preview(token, overrides = {}) {
  return result({ requiresConfirmation: true, confirmationToken: token, planSummary: `Plan ${token}`, ...overrides });
}

function verification(status, canContinue = true) {
  return { status, summary: `Verification ${status}.`, runId: null, evidence: {}, canContinue };
}

function taskReceipt(overrides = {}) {
  return { id: "task-one", goal: "restore_service", preserveExistingMods: true, operationLimit: 3,
    instanceId: "fixture-server", moduleId: "dontstarve", status: "proposed", requirements: [], checks: [], ...overrides };
}

test("stop and save backup receipts never claim startup verification", () => {
  const { assistantWorkflowResultMessage } = loadWorkflow();
  for (const action of ["stop_server", "create_backup", "restore_backup"]) {
    for (const status of ["verified", "failed", "inconclusive"]) {
      const usedKeys = [];
      const rendered = assistantWorkflowResultMessage(result({ action,
        message: "The confirmed operation returned a receipt.", verification: verification(status, false),
      }), (key, fallback) => { usedKeys.push(key); return fallback; });
      assert.ok(usedKeys.includes(`assistant.operation.lifecycle.${status}`));
      assert.doesNotMatch(rendered.content, /Startup verified|new server processes remained alive/);
    }
  }
  const restart = assistantWorkflowResultMessage(result({ action: "restart_server", verification: verification("verified", false) }));
  assert.match(restart.content, /Startup verified/);
});

test("explicit task authorization is forwarded once and every automatic step is counted and displayed", async () => {
  let binding;
  const completed = result({ task: taskReceipt({ status: "completed" }), completedOperations: [
    { action: "repair_ports", instanceId: "fixture-server", message: "Port repair saved.", verification: null, task: taskReceipt({ status: "completed" }) },
  ] });
  const { runAssistantWorkflow } = loadWorkflow();
  const outcome = await runAssistantWorkflow(preview("scoped", { task: taskReceipt() }), {
    confirmPreview: () => ({ confirmed: true, continueTask: true }),
    executeConfirmed: async (value) => { binding = value; return completed; },
    onResult: () => {},
  });
  assert.equal(binding.continueTask, true);
  assert.equal(outcome.completedSteps, 2);
  const app = loadAppHandler(preview("scoped", { task: taskReceipt() }), [completed], [{ confirmed: true, continueTask: true }]);
  await app.run();
  assert.equal(app.messages[0].content, "Port repair saved.");
  assert.match(app.messages[1].content, /Saved configuration/);
});

test("batch repair receipts retain every backup and rollback state without claiming recovery", async () => {
  const { assistantWorkflowResultMessage, runAssistantWorkflow } = loadWorkflow();
  const operation = result({ action: "patch_instance_files", fileChangesResult: {
    status: "partial", error: "Second file conflicted during rollback.", files: [
      { file: "mods/a.lua", sourceSha256: "a", resultSha256: "b", backupId: "backup-a", state: "rolled_back", readBackVerified: false },
      { file: "mods/b.lua", sourceSha256: "c", resultSha256: "d", backupId: "backup-b", state: "recovery_required", readBackVerified: false, error: "File changed again." },
    ],
  }, verification: verification("inconclusive", true), followUp: preview("must-not-run") });
  const message = assistantWorkflowResultMessage(operation);
  assert.match(message.content, /mods\/a.lua[\s\S]*Rolled back[\s\S]*backup-a/);
  assert.match(message.content, /mods\/b.lua[\s\S]*Recovery required[\s\S]*backup-b/);
  assert.match(message.content, /Second file conflicted/);
  assert.equal(message.state, "error");
  const outcome = await runAssistantWorkflow(operation, {
    confirmPreview: () => assert.fail("Partial write must not continue"), executeConfirmed: () => assert.fail("Partial write must not continue"), onResult: () => {},
  });
  assert.equal(outcome.status, "failed");
});

test("task completion requires a completed receipt and never overrides failed verification", async () => {
  const { runAssistantWorkflow, assistantWorkflowExecutionState } = loadWorkflow();
  for (const [taskStatus, verificationStatus, expected] of [
    ["inconclusive", "verified", "inconclusive"], ["proposed", "verified", "inconclusive"],
    ["completed", "failed", "failed"], ["completed", "verified", "completed"],
  ]) {
    const outcome = await runAssistantWorkflow(result({ task: taskReceipt({ status: taskStatus }),
      verification: verification(verificationStatus, false) }), {
      confirmPreview: () => { throw new Error("unexpected confirmation"); },
      executeConfirmed: () => { throw new Error("unexpected execution"); }, onResult: () => {},
    });
    assert.equal(outcome.status, expected);
    assert.equal(assistantWorkflowExecutionState(outcome), expected === "completed" ? "success" : expected === "failed" ? "error" : expected);
  }
  const cancelled = { status: "cancelled", lastOperation: null };
  assert.equal(assistantWorkflowExecutionState(cancelled), "cancelled");
});

test("task identity and policy cannot change between confirmation steps", async () => {
  const { runAssistantWorkflow } = loadWorkflow();
  for (const changed of [{ id: "other" }, { goal: "apply_change" }, { preserveExistingMods: false },
    { instanceId: "other" }, { moduleId: "other" }, null]) {
    let requests = 0;
    await assert.rejects(runAssistantWorkflow(preview("one", { task: taskReceipt() }), {
      confirmPreview: () => true,
      executeConfirmed: async () => { requests += 1; return result({ task: changed === null ? null : taskReceipt(changed) }); },
      onResult: () => { throw new Error("unbound result was published"); },
    }), /task binding/i);
    assert.equal(requests, 1);
  }
});

test("pending runtime command delivery remains inconclusive and does not claim game effects", async () => {
  const { runAssistantWorkflow, assistantWorkflowExecutionState, assistantWorkflowResultMessage } = loadWorkflow();
  const operation = result({ action: "run_gm_command", message: "Command accepted; awaiting write confirmation.",
    task: taskReceipt({ goal: "apply_change", status: "inconclusive", checks: [
      { name: "runtime_command_delivery", status: "unknown", summary: "Backend pending-delivery summary.", evidence: { write_confirmation_pending: true } },
    ] }),
  });
  const outcome = await runAssistantWorkflow(operation, {
    confirmPreview: () => assert.fail("a submitted command must not be sent again"),
    executeConfirmed: () => assert.fail("a submitted command must not be sent again"),
    onResult: () => {},
  });
  assert.equal(outcome.status, "inconclusive");
  assert.equal(assistantWorkflowExecutionState(outcome), "inconclusive");
  const formatted = assistantWorkflowResultMessage(operation);
  assert.match(formatted.content, /Unknown: Command delivery confirmation; in-game effects are not verified/);
  assert.doesNotMatch(formatted.content, /This operation is complete|Service recovery checks passed|Backend pending-delivery summary/);
});

test("task checks are presented as evidence summaries without serializing private evidence", () => {
  const { assistantWorkflowResultMessage } = loadWorkflow();
  const message = assistantWorkflowResultMessage(result({ task: taskReceipt({ status: "inconclusive", checks: [
    { name: "mods", status: "satisfied", summary: "Existing enabled mods were preserved.", evidence: { token: "synthetic-test-only" } },
  ] }) }));
  assert.match(message.content, /Existing enabled mods were preserved/);
  assert.match(message.content, /inconclusive/i);
  assert.doesNotMatch(message.content, /synthetic-test-only/);
});

test("a plain inspection preserves its explanation without exposing an empty task receipt or claiming verification", async () => {
  const { assistantWorkflowResultMessage, runAssistantWorkflow, assistantWorkflowExecutionState } = loadWorkflow();
  const explanation = "日志中的错误表示 Mod 读取了空值。仅凭当前片段，还不能确定是哪一个 Mod。";
  const operation = result({ handled: false, action: "none", message: explanation,
    task: taskReceipt({ goal: "inspect", status: "inconclusive" }),
  });
  assert.equal(assistantWorkflowResultMessage(operation).content, explanation);
  const outcome = await runAssistantWorkflow(operation, {
    confirmPreview: () => assert.fail("read-only inspection must not request confirmation"),
    executeConfirmed: () => assert.fail("read-only inspection must not execute a change"),
    onResult: () => {},
  });
  assert.equal(outcome.status, "inconclusive");
  assert.equal(assistantWorkflowExecutionState(outcome), "inconclusive");
});

test("inspection rendering retains failures, actual checks, requirements and operation receipts", () => {
  const { assistantWorkflowResultMessage } = loadWorkflow();
  const operation = result({ handled: false, action: "none", message: "Evidence remains limited.",
    task: taskReceipt({ goal: "inspect", status: "inconclusive" }),
  });
  const failedTask = assistantWorkflowResultMessage({ ...operation, task: { ...operation.task, status: "failed" } });
  assert.match(failedTask.content, /Task failed/);
  assert.equal(failedTask.state, "error");
  const failedVerification = assistantWorkflowResultMessage({ ...operation, verification: verification("failed", false) });
  assert.match(failedVerification.content, /Verification failed/);
  assert.equal(failedVerification.state, "error");
  assert.ok(failedVerification.content.includes(operation.message));
  for (const override of [
    { action: "customize_config" },
    { requiresConfirmation: true },
    { task: { ...operation.task, goal: "apply_change" } },
    { task: { ...operation.task, checks: [{ name: "diagnostic_evidence", status: "unknown", summary: "Evidence is incomplete.", evidence: null }] } },
    { task: { ...operation.task, requirements: [{ id: "requirement_1", kind: "unverified", description: "Check player connectivity", sourceText: "Check player connectivity", target: null, expectedDisplay: null }] } },
  ]) {
    const formatted = assistantWorkflowResultMessage({ ...operation, ...override });
    assert.match(formatted.content, /Task inconclusive/);
    assert.ok(formatted.content.includes(operation.message));
  }
});

test("completion wording follows the resolved goal and known checks use localized labels", () => {
  const { assistantWorkflowResultMessage } = loadWorkflow();
  const translations = {
    "assistant.task.status.completed.apply_change": "本次操作已完成。",
    "assistant.task.status.completed.inspect": "本次检查已完成。",
    "assistant.task.status.completed.prepare_service": "服务器配置检查通过，未执行启动操作。",
    "assistant.task.status.completed.restore_service": "恢复服务验收通过。",
    "assistant.task.status.completed.launch_service": "开服验收通过。",
    "assistant.task.check.satisfied": "已满足",
    "assistant.task.checkName.mod_configuration_preserved": "保留原有模组配置",
  };
  for (const goal of ["inspect", "apply_change", "restore_service", "launch_service", "prepare_service"]) {
    const formatted = assistantWorkflowResultMessage(result({ task: taskReceipt({ goal, status: "completed", checks: [
      { name: "mod_configuration_preserved", status: "satisfied", summary: "Backend fixed-language summary", evidence: { secret: "synthetic-test-only" } },
    ] }) }), (key, fallback) => translations[key] ?? fallback);
    assert.ok(formatted.content.startsWith(translations[`assistant.task.status.completed.${goal}`]));
    assert.match(formatted.content, /已满足: 保留原有模组配置/);
    assert.doesNotMatch(formatted.content, /Backend fixed-language summary|synthetic-test-only/);
  }
});

test("preparation verification describes saved configuration without claiming runtime startup", () => {
  const { assistantWorkflowResultMessage } = loadWorkflow();
  const summary = "The saved 12-player configuration was read back. No start operation was performed.";
  const operation = result({ message: "Configuration saved.", task: taskReceipt({ goal: "prepare_service", status: "completed" }),
    verification: { ...verification("verified", false), summary },
  });
  const formatted = assistantWorkflowResultMessage(operation);
  assert.match(formatted.content, /^Server preparation checks passed; no start operation was performed\./);
  assert.ok(formatted.content.includes(summary));
  assert.doesNotMatch(formatted.content, /Startup verified|new server processes remained alive|recovery could not be confirmed/);
  for (const taskStatus of ["completed", "failed"]) {
    const failed = assistantWorkflowResultMessage({ ...operation,
      task: { ...operation.task, status: taskStatus },
      verification: { ...verification("failed", false), summary: "The saved player limit does not match the request." },
    });
    assert.match(failed.content, /^Task failed/);
    assert.match(failed.content, /saved player limit does not match/);
    assert.doesNotMatch(failed.content, /checks passed/);
    assert.equal(failed.state, "error");
  }
  for (const taskStatus of ["completed", "inconclusive"]) {
    const incomplete = assistantWorkflowResultMessage({ ...operation,
      task: { ...operation.task, status: taskStatus },
      verification: { ...verification("inconclusive", false), summary: "The required configuration could not be read back." },
    });
    assert.match(incomplete.content, /^Task inconclusive/);
    assert.match(incomplete.content, /configuration could not be read back/);
    assert.doesNotMatch(incomplete.content, /checks passed|Startup verified|recovery could not be confirmed/);
  }
});

test("workflow confirms each step and records execution evidence before the next confirmation", async () => {
  const { runAssistantWorkflow } = loadWorkflow();
  const events = [];
  const outcome = await runAssistantWorkflow(preview("one"), {
    confirmPreview: (step) => { events.push(`ask:${step.confirmationToken}`); return true; },
    executeConfirmed: async ({ confirmationToken, planSummary }) => {
      assert.equal(planSummary, `Plan ${confirmationToken}`);
      events.push(`execute:${confirmationToken}`);
      return confirmationToken === "one"
        ? result({ verification: verification("inconclusive"), followUp: preview("two", { action: "start_server" }) })
        : result({ action: "start_server", verification: verification("verified", false) });
    },
    onResult: (step) => events.push(`result:${step.verification.status}`),
  });
  assert.deepEqual(events, ["ask:one", "execute:one", "result:inconclusive", "ask:two", "execute:two", "result:verified"]);
  assert.equal(outcome.status, "completed");
  assert.equal(outcome.completedSteps, 2);
});

test("cancelling a follow-up preserves the first executed result and never sends the second token", async () => {
  const { runAssistantWorkflow } = loadWorkflow();
  const executed = [];
  const recorded = [];
  const outcome = await runAssistantWorkflow(preview("one"), {
    confirmPreview: (step) => step.confirmationToken === "one",
    executeConfirmed: async ({ confirmationToken }) => {
      executed.push(confirmationToken);
      return result({ verification: verification("failed"), followUp: preview("two") });
    },
    onResult: (step) => recorded.push(step),
  });
  assert.deepEqual(executed, ["one"]);
  assert.equal(recorded.length, 1);
  assert.equal(outcome.status, "cancelled");
  assert.equal(outcome.completedSteps, 1);
  assert.equal(outcome.lastOperation.verification.status, "failed");
  assert.equal(outcome.pendingOperation.confirmationToken, "two");
});

test("workflow fails closed after its safety ceiling even when the backend keeps offering work", async () => {
  const { runAssistantWorkflow } = loadWorkflow();
  let confirmations = 0;
  const recorded = [];
  const outcome = await runAssistantWorkflow(preview("1"), {
    confirmPreview: () => { confirmations += 1; return true; },
    executeConfirmed: async ({ confirmationToken }) => result({
      verification: verification("inconclusive"), followUp: preview(String(Number(confirmationToken) + 1)),
    }),
    onResult: (step) => recorded.push(step),
  });
  assert.equal(confirmations, 32);
  assert.equal(recorded.length, 32);
  assert.equal(outcome.status, "limit-reached");
  assert.equal(outcome.completedSteps, 32);
});

test("failed verification stays failed and cannot continue against the backend verification gate", async () => {
  const { runAssistantWorkflow, assistantWorkflowResultMessage } = loadWorkflow();
  const operation = result({ verification: verification("failed", false), followUp: preview("blocked") });
  let confirmations = 0;
  const outcome = await runAssistantWorkflow(operation, {
    confirmPreview: () => { confirmations += 1; return true; },
    executeConfirmed: async () => { throw new Error("blocked continuation"); },
    onResult: () => {},
  });
  assert.equal(confirmations, 0);
  assert.equal(outcome.status, "failed");
  assert.equal(assistantWorkflowResultMessage(operation).state, "error");
  assert.equal(assistantWorkflowResultMessage(operation).content, "Verification failed: the server issue remains unresolved.\n\nSaved configuration.\n\nVerification failed.");
});

test("verification headings are translated without replacing backend evidence or diagnosis", () => {
  const { assistantWorkflowResultMessage } = loadWorkflow();
  const headings = {
    verified: "已验证启动：新进程已启动，并在观察期间保持运行；尚未验证玩家能否联机。",
    failed: "验证仍失败：服务器问题尚未解决。",
    inconclusive: "尚无法确认：目前证据不足，无法确认问题已修复。",
  };
  for (const status of Object.keys(headings)) {
    const operation = result({
      message: "Saved configuration.",
      verification: { ...verification(status, false), summary: "Backend evidence.\n模型诊断：仍需检查客户端连接。" },
    });
    const translated = [];
    const formatted = assistantWorkflowResultMessage(operation, (key, fallback) => {
      translated.push({ key, fallback });
      return headings[status];
    });
    assert.equal(translated.length, 1);
    assert.equal(translated[0].key, `assistant.operation.verification.${status}`);
    assert.ok(translated[0].fallback.length > 0);
    assert.equal(formatted.content, `${headings[status]}\n\n${operation.message}\n\n${operation.verification.summary}`);
    assert.equal(formatted.state, status === "failed" ? "error" : "ready");
  }
});

test("verified fallback limits the claim to observed startup rather than player connectivity", () => {
  const { assistantWorkflowResultMessage } = loadWorkflow();
  const formatted = assistantWorkflowResultMessage(result({
    verification: verification("verified", false),
  }));
  assert.match(formatted.content, /^Startup verified: new server processes remained alive during the observation period\./);
  assert.match(formatted.content, /Player connectivity has not been verified\./);
  assert.match(formatted.content, /Verification verified\./);
});

test("workflow rejects missing, repeated and unconsumed confirmation bindings without another execution", async () => {
  const { runAssistantWorkflow } = loadWorkflow();
  for (const scenario of ["missing", "repeated", "unconsumed", "unconfirmed-followup"]) {
    let executions = 0;
    const recorded = [];
    await assert.rejects(runAssistantWorkflow(preview("one", scenario === "missing" ? { planSummary: " " } : {}), {
      confirmPreview: () => true,
      executeConfirmed: async () => {
        executions += 1;
        if (scenario === "unconsumed") return preview("one");
        return result({ verification: verification("inconclusive"), followUp: scenario === "unconfirmed-followup" ? result() : preview("one") });
      },
      onResult: (step) => recorded.push(step),
    }), /confirmation|preview/i);
    assert.equal(executions, scenario === "missing" ? 0 : 1, scenario);
    assert.equal(recorded.length, ["repeated", "unconfirmed-followup"].includes(scenario) ? 1 : 0, scenario);
  }
});

test("workflow preserves diagnosis text without requesting confirmation", async () => {
  const { runAssistantWorkflow, assistantWorkflowResultMessage } = loadWorkflow();
  const diagnosis = result({ handled: false, action: "none", message: "Missing dependency; no changes made." });
  const recorded = [];
  const outcome = await runAssistantWorkflow(diagnosis, {
    confirmPreview: () => { throw new Error("unexpected confirmation"); },
    executeConfirmed: async () => { throw new Error("unexpected execution"); },
    onResult: (step) => recorded.push(step),
  });
  assert.equal(outcome.completedSteps, 0);
  assert.equal(recorded[0].message, diagnosis.message);
  assert.equal(assistantWorkflowResultMessage(diagnosis).content, diagnosis.message);
});

test("a follow-up without explicit verification permission is never confirmed", async () => {
  const { runAssistantWorkflow } = loadWorkflow();
  const recorded = [];
  const outcome = await runAssistantWorkflow(result({ followUp: preview("unverified") }), {
    confirmPreview: () => { throw new Error("missing verification permission"); },
    executeConfirmed: async () => { throw new Error("missing verification permission"); },
    onResult: (step) => recorded.push(step),
  });
  assert.equal(recorded.length, 1);
  assert.equal(outcome.completedSteps, 1);
});

test("cancelling the first preview performs no operation and records no execution", async () => {
  const { runAssistantWorkflow } = loadWorkflow();
  const outcome = await runAssistantWorkflow(preview("one"), {
    confirmPreview: () => false,
    executeConfirmed: async () => { throw new Error("cancelled preview"); },
    onResult: () => { throw new Error("nothing executed"); },
  });
  assert.equal(outcome.status, "cancelled");
  assert.equal(outcome.completedSteps, 0);
  assert.equal(outcome.lastOperation, null);
});

test("asynchronous review sends the original binding only after confirmation resolves", async () => {
  const { runAssistantWorkflow } = loadWorkflow();
  let respond;
  const bindings = [];
  const original = preview("  bound-token  ", { planSummary: "  Original plan\nwith exact spacing  " });
  const running = runAssistantWorkflow(original, {
    confirmPreview: () => new Promise((resolve) => { respond = resolve; }),
    executeConfirmed: async (binding) => { bindings.push(binding); return result(); },
    onResult: () => {},
  });
  await Promise.resolve();
  assert.equal(bindings.length, 0);
  respond(true);
  await running;
  assert.equal(bindings.length, 1);
  assert.equal(bindings[0].confirmationToken, original.confirmationToken);
  assert.equal(bindings[0].planSummary, original.planSummary);
});

test("file receipts show backup and readback without claiming startup or exposing patch text", async () => {
  const { runAssistantWorkflow, assistantWorkflowResultMessage, assistantWorkflowExecutionState } = loadWorkflow();
  const saved = result({ action: "patch_instance_text", message: "Applied instance text change.",
    verification: verification("verified", false),
    fileChangePreview: { before: "PRIVATE_ORIGINAL", after: "PRIVATE_REPLACEMENT" },
    fileChangeResult: { file: "mods/workshop-123/modmain.lua", backupId: "patch-backup-123", readBackVerified: true },
  });
  const rendered = assistantWorkflowResultMessage(saved);
  assert.match(rendered.content, /mods\/workshop-123\/modmain\.lua/);
  assert.match(rendered.content, /patch-backup-123/);
  assert.match(rendered.content, /reading the saved file/);
  assert.doesNotMatch(rendered.content, /Startup verified|processes remained alive|PRIVATE_/);
  for (const [receipt, expected] of [[{ ...saved.fileChangeResult, readBackVerified: false }, "failed"], [null, "inconclusive"]]) {
    const unverified = { ...saved, fileChangeResult: receipt, followUp: preview("retry"),
      task: taskReceipt({ status: "completed" }), verification: verification("verified", true) };
    assert.doesNotMatch(assistantWorkflowResultMessage(unverified).content, /Service recovery checks passed/);
    const outcome = await runAssistantWorkflow(unverified, {
      confirmPreview: () => assert.fail("unverified file writes must stop"),
      executeConfirmed: () => assert.fail("unverified file writes must stop"), onResult: () => {},
    });
    assert.equal(outcome.status, expected);
    assert.equal(assistantWorkflowExecutionState(outcome), expected === "failed" ? "error" : "inconclusive");
  }
});


test("App keeps the executed result when a later preview is cancelled and presents failed verification", async () => {
  const handler = loadAppHandler(preview("one"), [result({
    verification: verification("failed"), followUp: preview("two"),
  })], [true, false]);
  await handler.run();
  assert.deepEqual(handler.confirmations, ["one"]);
  assert.equal(handler.messages.length, 2);
  assert.match(handler.messages[0].content, /Saved configuration/);
  assert.match(handler.messages[0].content, /Verification failed/);
  assert.equal(handler.messages[0].state, "error");
  assert.match(handler.messages[1].content, /earlier|previous|completed/i);
  assert.doesNotMatch(handler.messages[1].content, /^Cancelled without executing:/);
  assert.equal(handler.states.at(-1).status, "error");
});

test("App clearly presents inconclusive recovery in the current locale while preserving the result", async () => {
  const heading = "尚无法确认：目前证据不足，无法确认问题已修复。";
  const handler = loadAppHandler(preview("one"), [result({
    verification: { ...verification("inconclusive", false), summary: "No matching fresh runtime evidence." },
  })], [true], { "assistant.operation.verification.inconclusive": heading });
  await handler.run();
  assert.equal(handler.messages.length, 1);
  assert.equal(handler.messages[0].content, `${heading}\n\nSaved configuration.\n\nNo matching fresh runtime evidence.`);
  assert.equal(handler.states.at(-1).status, "inconclusive");
});

test("App formats request errors once before publishing them to assistant messages and execution state", async () => {
  const backendError = new Error(JSON.stringify({ code: "assistant_request_interpretation_failed",
    message: "The assistant could not interpret this request. No operation was executed." }));
  const localized = "暂时没能理解这条请求，请重试。尚未执行任何操作。";
  const formatted = [];
  const handler = loadAppHandler(() => { throw backendError; }, [], [], {}, {
    formatDesktopError: (translate, error) => { formatted.push({ translate, error }); return localized; },
  });
  await handler.run();
  assert.equal(formatted.length, 1);
  assert.equal(formatted[0].translate, handler.context.t);
  assert.equal(formatted[0].error, backendError);
  assert.equal(handler.messages.length, 1);
  assert.equal(handler.messages[0].content, localized);
  assert.equal(handler.messages[0].state, "error");
  assert.equal(handler.states.at(-1).error, localized);
  assert.equal(handler.states.at(-1).status, "error");
  assert.deepEqual(handler.confirmations, []);
  assert.deepEqual(handler.refreshes, []);
  assert.equal(handler.context.assistantRequestRef.current, false);
});

test("App snapshots selection context, blocks concurrent requests and keeps old-scope results in their conversation", async () => {
  let release;
  let entered;
  const requested = new Promise((resolve) => { entered = resolve; });
  const pending = new Promise((resolve) => { release = resolve; });
  const handler = loadAppHandler(() => { entered(); return pending; }, [], []);
  const first = handler.run();
  await requested;
  assert.equal(handler.context.assistantRequestRef.current, true);
  handler.context.selectedInstanceId = "new-selection";
  handler.context.selectedModuleId = "minecraft";
  handler.context.assistantScopeRef.current = "new-scope";
  await handler.run();
  assert.equal(handler.requests.length, 1);
  assert.equal(Object.hasOwn(handler.requests[0], "task"), false);
  assert.equal(handler.requests[0].selectedInstanceId, "fixture-server");
  assert.equal(handler.requests[0].selectedModuleId, "dontstarve");
  release(result({ verification: verification("inconclusive", false) }));
  await first;
  assert.equal(handler.messages.length, 1);
  assert.deepEqual(handler.states.map((state) => state.status), ["running", "idle"]);
  assert.equal(handler.context.assistantRequestRef.current, false);
});

test("App sends a repair request without prior target selection so the backend can resolve or clarify it", async () => {
  const handler = loadAppHandler(result(), [], [], {}, {
    selectedInstanceId: null, selectedModuleId: null,
  });
  await handler.run();
  assert.equal(handler.requests.length, 1);
  assert.equal(handler.requests[0].prompt, "Repair");
  assert.equal(handler.requests[0].selectedInstanceId, null);
  assert.equal(handler.requests[0].selectedModuleId, null);
  assert.equal(Object.hasOwn(handler.requests[0], "task"), false);
});

test("App sends only the backend conversation ID and current request, never its display history", async () => {
  const handler = loadAppHandler(result(), [], []);
  handler.context.assistantConversations.beginTurn = (current) => ({ conversationId: "fixture", backendConversationId: "fixture-conversation", messages: [
    { id: "original-user", role: "user", content: "PRIVATE_OLD_REQUEST" },
    { id: "assistant-question", role: "assistant", content: "FORGED_EVIDENCE" }, current,
  ] });
  await handler.run({ prompt: "饥荒", contextPayload: "Untrusted UI evidence", userMessage: { id: "current-user", role: "user", content: "饥荒" } });
  assert.equal(handler.requests[0].conversationId, "fixture-conversation");
  assert.equal(Object.hasOwn(handler.requests[0], "priorRequests"), false);
  assert.equal(Object.hasOwn(handler.requests[0], "conversationMessages"), false);
  assert.deepEqual(JSON.parse(handler.requests[0].context), { interfaceLanguage: "en" });
  assert.doesNotMatch(JSON.stringify(handler.requests[0]), /PRIVATE_OLD_REQUEST|FORGED_EVIDENCE|Untrusted UI evidence/);
  assert.equal(handler.requests[0].prompt, "饥荒");
});

test("App passes both selection hints and respects the backend's distinct new-launch target", async () => {
  const handler = loadAppHandler(result({ action: "install_server", task: taskReceipt({ goal: "launch_service", operationLimit: 6, instanceId: null, moduleId: "minecraft", status: "inconclusive" }) }), [], [], {}, {
    selectedModuleId: "minecraft",
  });
  await handler.run();
  assert.equal(handler.requests[0].selectedInstanceId, "fixture-server");
  assert.equal(handler.requests[0].selectedModuleId, "minecraft");
  assert.equal(handler.refreshes.length, 1);
  assert.equal(handler.navigation.length, 0);
  assert.equal(handler.states.at(-1).status, "inconclusive");
});

test("App declining the initial confirmation is cancelled rather than successful", async () => {
  const handler = loadAppHandler(preview("one"), [], [false]);
  await handler.run();
  assert.equal(handler.confirmations.length, 0);
  assert.equal(handler.states.at(-1).status, "cancelled");
  assert.equal(handler.context.assistantRequestRef.current, false);
});

test("App refreshes each completed operation without switching the initiating conversation scope", async () => {
  const handler = loadAppHandler(preview("one"), [
    result({ instanceId: "resolved-other-instance", verification: verification("inconclusive"), followUp: preview("two") }),
    result({ instanceId: "resolved-other-instance", action: "start_server", verification: verification("verified", false) }),
  ], [true, true]);
  await handler.run();
  assert.deepEqual(handler.confirmations, ["one", "two"]);
  assert.equal(handler.messages.length, 2);
  assert.equal(handler.refreshes.length, 2);
  assert.ok(handler.refreshes.every(({ preferred, options }) => preferred === undefined && options.refreshOverlays === true));
  assert.deepEqual(handler.navigation, [], "Automatic navigation hides the active conversation and its execution evidence");
});


test("backend checkpoint pauses without claiming completion or sending another confirmation", async () => {
  const { runAssistantWorkflow, assistantWorkflowExecutionState } = loadWorkflow();
  const output = result({ handled: false, action: "none", continuation: { reason: "run_budget", summary: "Checked files; analysis remains.", canResume: true } });
  const published = [];
  const outcome = await runAssistantWorkflow(output, {
    confirmPreview: () => assert.fail("Checkpoint is not an operation preview"),
    executeConfirmed: () => assert.fail("Checkpoint requires an explicit resume"),
    onResult: (step) => published.push(step),
  });
  assert.equal(outcome.status, "paused");
  assert.equal(assistantWorkflowExecutionState(outcome), "paused");
  assert.equal(outcome.completedSteps, 0);
  assert.equal(published[0], output);
});

test("a stop arriving during confirmation prevents execution even if the dialog resolves yes", async () => {
  const { runAssistantWorkflow } = loadWorkflow();
  let stopped = false;
  let cancellations = 0;
  const outcome = await runAssistantWorkflow(preview("pending"), {
    shouldStop: () => stopped,
    confirmPreview: async () => { stopped = true; return true; },
    executeConfirmed: () => assert.fail("Stopped preview must not execute"),
    onResult: () => assert.fail("No execution receipt exists"),
    cancelPending: async () => { cancellations += 1; },
  });
  assert.equal(outcome.status, "cancelled");
  assert.equal(cancellations, 1);
});

test("App creates a backend conversation before its first long request and binds only that ID", async () => {
  let created;
  const bindings = [];
  const handler = loadAppHandler(result({ handled: false, action: "none" }), [], []);
  handler.context.assistantConversations.beginTurn = () => ({ conversationId: "local-display", backendConversationId: null });
  handler.context.assistantConversations.bindBackend = (...args) => bindings.push(args);
  handler.context.createAssistantConversation = () => new Promise((resolve) => { created = resolve; });
  const pending = handler.run();
  assert.equal(handler.requests.length, 0);
  assert.equal(handler.context.assistantRequestRef.current, true);
  created({ conversationId: "fixture-conversation", revision: 0 });
  await pending;
  assert.equal(bindings[0][0], "local-display");
  assert.equal(handler.requests[0].conversationId, "fixture-conversation");
  assert.equal(handler.context.assistantRequestRef.current, false);
});

test("App stopping an outstanding turn waits for its final response and retains completed receipts", async () => {
  let finish;
  let entered;
  const requested = new Promise((resolve) => { entered = resolve; });
  const cancellations = [];
  const handler = loadAppHandler(() => new Promise((resolve) => { finish = resolve; entered(); }), [], [], {}, {
    cancelAssistantTurn: async (conversationId) => { cancellations.push(conversationId); return { conversationId, stopping: true }; },
  });
  const pending = handler.run();
  await requested;
  await handler.context.assistantTurnRef.current.stop();
  assert.equal(handler.context.assistantRequestRef.current, true);
  assert.equal(handler.states.at(-1).status, "running");
  await handler.run();
  assert.equal(handler.requests.length, 1);
  finish(result({ message: "Configuration was saved before stop arrived." }));
  await pending;
  assert.equal(handler.context.assistantRequestRef.current, false);
  assert.deepEqual(cancellations, ["fixture-conversation"]);
  assert.match(handler.messages[0].content, /saved before stop/);
  assert.equal(handler.states.at(-1).status, "cancelled");
});

test("App resumes only the backend checkpoint and reuses the confirmation workflow", async () => {
  const resumes = [];
  const checkpoints = [];
  const handler = loadAppHandler(() => assert.fail("Resume must not reinterpret a prompt"), [result()], [true], {}, {
    resumeAssistantConversation: async (...args) => { resumes.push(args); return preview("resume-confirmation"); },
  });
  handler.context.assistantConversations.beginTurn = () => assert.fail("Resume must not create another conversation");
  handler.context.assistantConversations.setContinuation = (...args) => checkpoints.push(args);
  await handler.run({ resume: { conversationId: "local-display", backendConversationId: "fixture-conversation" }, prompt: "" });
  assert.equal(resumes.length, 1);
  assert.equal(resumes[0][0], "fixture-conversation");
  assert.deepEqual(Object.keys(resumes[0][1]).sort(), ["apiKey", "baseUrl", "model", "provider"]);
  assert.deepEqual(handler.confirmations, ["resume-confirmation"]);
  assert.equal(handler.requests.length, 0);
  assert.equal(checkpoints.at(-1)[1], null);
});


test("App queues a stop during session creation and never starts the model request", async () => {
  let created;
  const cancellations = [];
  const handler = loadAppHandler(() => assert.fail("Stopped creation cannot start a model request"), [], [], {}, {
    cancelAssistantTurn: async (conversationId) => { cancellations.push(conversationId); return { conversationId, stopping: false }; },
  });
  handler.context.assistantConversations.beginTurn = () => ({ conversationId: "local-display", backendConversationId: null });
  handler.context.createAssistantConversation = () => new Promise((resolve) => { created = resolve; });
  const pending = handler.run();
  await handler.context.assistantTurnRef.current.stop();
  assert.equal(handler.context.assistantRequestRef.current, true);
  created({ conversationId: "fixture-conversation", revision: 0 });
  await pending;
  assert.deepEqual(cancellations, ["fixture-conversation"]);
  assert.equal(handler.requests.length, 0);
  assert.equal(handler.states.at(-1).status, "cancelled");
});
