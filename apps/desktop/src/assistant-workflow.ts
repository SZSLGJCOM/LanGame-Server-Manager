import type { AssistantChatMessageState, AssistantExecuteOperationOutput, AssistantExecutionState, AssistantFileChangesResult, AssistantTaskReceipt, AssistantTaskRequirementView } from "./types";

export const ASSISTANT_REQUEST_SAFETY_LIMIT = 32;
const TASK_CHECK_LABELS: Record<string, string> = {
  operation_result: "Operation and readback checks",
  runtime_command_delivery: "Command delivery confirmation; in-game effects are not verified",
  server_files_ready: "Server files installed and validated",
  prepared_files_ready: "Server installation and executable are available",
  prepared_instance_bound: "Prepared instance is bound to the request",
  initial_configuration_saved: "Requested initial configuration saved",
  instance_created: "Server instance created without starting it",
  mod_configuration_preserved: "Existing mod configuration preserved",
  new_run_ready: "New run startup readiness; player connectivity is not verified",
  mod_requirements_known: "Complete intended mod set identified",
  required_mods_running: "Required mod enablement and loading; full mod functionality is not verified",
  task_snapshot_unchanged: "Configuration and run identity unchanged during verification",
  file_changes_preserved: "Confirmed file changes preserved through verification",
  bound_target: "Target instance bound to the task baseline",
};

function requirementBinding(requirements: AssistantTaskRequirementView[]): string {
  const ids = new Set<string>();
  if (!Array.isArray(requirements) || requirements.some((requirement) => {
    if (!requirement || typeof requirement.id !== "string" || !requirement.id.trim() || ids.has(requirement.id)
      || !["setting", "port", "forbidden_action", "unverified"].includes(requirement.kind)
      || typeof requirement.description !== "string" || typeof requirement.sourceText !== "string"
      || (requirement.target !== null && typeof requirement.target !== "string")
      || (requirement.expectedDisplay !== null && typeof requirement.expectedDisplay !== "string")) return true;
    ids.add(requirement.id);
    return false;
  })) throw new Error("Assistant task requirements are missing or invalid.");
  return JSON.stringify(requirements.map(({ id, kind, description, sourceText, target, expectedDisplay }) =>
    [id, kind, description, sourceText, target, expectedDisplay]));
}

function requirementStatus(task: AssistantTaskReceipt, requirement: AssistantTaskRequirementView): "satisfied" | "failed" | "unknown" {
  const checks = task.checks.filter((check) => check.name === requirement.id);
  return requirement.kind !== "unverified" && checks.length === 1 ? checks[0].status : "unknown";
}

function taskResultStatus(task: AssistantTaskReceipt): AssistantTaskReceipt["status"] {
  if (task.status !== "completed") return task.status;
  const statuses = task.requirements.map((requirement) => requirementStatus(task, requirement));
  if (statuses.includes("failed")) return "failed";
  return statuses.some((status) => status !== "satisfied") ? "inconclusive" : task.status;
}

interface AssistantConfirmationBinding {
  conversationId: string;
  confirmationToken: string;
  planSummary: string;
  continueTask?: boolean;
}

export type AssistantConfirmationDecision = boolean | { confirmed: boolean; continueTask: boolean };

interface AssistantWorkflowCallbacks {
  confirmPreview: (preview: AssistantExecuteOperationOutput) => AssistantConfirmationDecision | Promise<AssistantConfirmationDecision>;
  executeConfirmed: (binding: AssistantConfirmationBinding) => Promise<AssistantExecuteOperationOutput>;
  onResult: (operation: AssistantExecuteOperationOutput) => void | Promise<void>;
  cancelPending?: () => Promise<void>;
  shouldStop?: () => boolean;
}

export interface AssistantWorkflowOutcome {
  status: "completed" | "failed" | "inconclusive" | "cancelled" | "paused" | "limit-reached";
  completedSteps: number;
  lastOperation: AssistantExecuteOperationOutput | null;
  pendingOperation: AssistantExecuteOperationOutput | null;
}

export function assistantWorkflowExecutionState(outcome: AssistantWorkflowOutcome): AssistantExecutionState["status"] {
  if (outcome.lastOperation?.fileChangesResult && outcome.lastOperation.fileChangesResult.status !== "applied") return "error";
  if (outcome.lastOperation?.fileChangeResult?.readBackVerified === false) return "error";
  if (outcome.status === "failed" || (outcome.lastOperation?.task && taskResultStatus(outcome.lastOperation.task) === "failed") || outcome.lastOperation?.verification?.status === "failed") return "error";
  return outcome.status === "completed" ? "success" : outcome.status;
}

export function assistantFileChangesReceipt(
  result: AssistantFileChangesResult,
  translate?: (key: string, fallback: string) => string,
): string {
  const text = (key: string, fallback: string) => translate?.(key, fallback) ?? fallback;
  const statuses = { applied: "Applied", not_applied: "Not applied", rolled_back: "Rolled back", partial: "Partial changes require attention", recovery_required: "Recovery required" };
  const sections = [text(`assistant.operation.files.${result.status}`, statuses[result.status]),
    text("assistant.operation.file.runtimeUnverified", "Text changes do not verify server recovery or mod compatibility.")];
  for (const file of result.files) {
    const details = [`${text("assistant.operation.file.path", "File")}: ${file.file}`,
      text(`assistant.operation.files.${file.state}`, statuses[file.state])];
    if (file.backupId) details.push(`${text("assistant.operation.file.backup", "Backup")}: ${file.backupId}`);
    if (file.readBackVerified && file.state === "applied") details.push(text("assistant.operation.file.verified", "Saved text verified by readback."));
    if (file.error) details.push(file.error);
    sections.push(details.join("\n"));
  }
  if (result.error) sections.push(result.error);
  return sections.join("\n\n");
}

export function assistantWorkflowResultMessage(
  operation: AssistantExecuteOperationOutput,
  translate?: (key: string, fallback: string) => string,
): {
  content: string;
  state: AssistantChatMessageState;
} {
  const summary = operation.verification?.summary.trim();
  const sections = [operation.message];
  if (summary && summary !== operation.message.trim()) sections.push(summary);
  const saveOrStop = ["stop_server", "create_backup", "restore_backup"].includes(operation.action);
  if (operation.verification && saveOrStop) {
    const headings = {
      verified: "The requested operation and its saved result were verified. No server startup was performed.",
      failed: "The requested operation failed. Review the result before making another change.",
      inconclusive: "The operation result could not be fully verified. Check the current state before retrying.",
    };
    const status = operation.verification.status;
    sections.unshift(translate?.(`assistant.operation.lifecycle.${status}`, headings[status]) ?? headings[status]);
  }
  if (operation.verification && !saveOrStop && !["patch_instance_text", "patch_instance_files"].includes(operation.action) && operation.task?.goal !== "prepare_service") {
    const headings = {
      verified: {
        key: "assistant.operation.verification.verified",
        fallback: "Startup verified: new server processes remained alive during the observation period. Player connectivity has not been verified.",
      },
      failed: {
        key: "assistant.operation.verification.failed",
        fallback: "Verification failed: the server issue remains unresolved.",
      },
      inconclusive: {
        key: "assistant.operation.verification.inconclusive",
        fallback: "Not yet verified: recovery could not be confirmed.",
      },
    };
    const heading = headings[operation.verification.status];
    sections.unshift(translate?.(heading.key, heading.fallback) ?? heading.fallback);
  }
  if (operation.action === "patch_instance_text") {
    const result = operation.fileChangeResult;
    const text = (key: string, fallback: string) => translate?.(key, fallback) ?? fallback;
    sections.unshift(text("assistant.operation.file.runtimeUnverified", "A text change does not verify server recovery or mod compatibility."));
    if (result) {
      sections.push(`${text("assistant.operation.file.path", "File")}: ${result.file}`);
      sections.push(`${text("assistant.operation.file.backup", "Backup")}: ${result.backupId}`);
      sections.push(result.readBackVerified
        ? text("assistant.operation.file.verified", "Text written and verified by reading the saved file.")
        : text("assistant.operation.file.failed", "Saved text could not be verified. Review the failure and backup before retrying."));
    } else {
      sections.push(text("assistant.operation.file.missing", "No verified file write receipt was returned."));
    }
  }
  if (operation.action === "patch_instance_files") {
    sections.unshift(operation.fileChangesResult ? assistantFileChangesReceipt(operation.fileChangesResult, translate)
      : translate?.("assistant.operation.file.missing", "No verified file write receipt was returned.") ?? "No verified file write receipt was returned.");
  }
  const plainInspection = operation.task?.goal === "inspect" && operation.action === "none"
    && !operation.requiresConfirmation && operation.task.status !== "failed"
    && operation.task.checks.length === 0 && operation.task.requirements.length === 0;
  if (operation.task && !plainInspection) {
    const { task } = operation;
    let taskStatus = operation.action === "patch_instance_text" && operation.fileChangeResult?.readBackVerified !== true
      ? operation.fileChangeResult ? "failed" : "inconclusive" : taskResultStatus(task);
    if (operation.action === "patch_instance_files" && operation.fileChangesResult?.status !== "applied") {
      taskStatus = operation.fileChangesResult ? "failed" : "inconclusive";
    }
    if (task.goal === "prepare_service") {
      if (operation.verification?.status === "failed") taskStatus = "failed";
      else if (taskStatus === "completed" && operation.verification?.status === "inconclusive") taskStatus = "inconclusive";
    }
    const statuses = {
      proposed: "Task proposed: confirmation is required.",
      completed: task.goal === "launch_service" ? "Server launch checks passed."
        : task.goal === "restore_service" ? "Service recovery checks passed."
          : task.goal === "prepare_service" ? "Server preparation checks passed; no start operation was performed."
            : task.goal === "inspect" ? "Inspection is complete." : "This operation is complete.",
      failed: "Task failed: the requested goal was not met.",
      inconclusive: "Task inconclusive: the requested goal has not been verified.",
    };
    const statusKey = taskStatus === "completed" ? `completed.${task.goal}` : taskStatus;
    sections.unshift(translate?.(`assistant.task.status.${statusKey}`, statuses[taskStatus]) ?? statuses[taskStatus]);
    const labels = { satisfied: "Satisfied", failed: "Not satisfied", unknown: "Unknown" };
    for (const requirement of task.requirements) {
      const status = requirementStatus(task, requirement);
      const label = translate?.(`assistant.task.check.${status}`, labels[status]) ?? labels[status];
      const details = [requirement.description];
      if (requirement.target !== null) details.push(`${translate?.("assistant.task.requirement.target", "Target") ?? "Target"}: ${requirement.target}`);
      if (requirement.expectedDisplay !== null) details.push(`${translate?.("assistant.task.requirement.expected", "Expected") ?? "Expected"}: ${requirement.expectedDisplay}`);
      if (status === "unknown" && requirement.kind === "unverified") {
        details.push(translate?.("assistant.task.requirement.unverified", "This requirement cannot be verified automatically.") ?? "This requirement cannot be verified automatically.");
      } else if (status !== "satisfied") {
        const matching = task.checks.filter((check) => check.name === requirement.id);
        const summary = matching.length === 1 ? matching[0].summary.trim() : "";
        if (summary && summary !== requirement.description) details.push(summary);
      }
      sections.push(`${label}: ${details.join("; ")}`);
    }
    for (const check of task.checks) {
      if (task.requirements.some((requirement) => requirement.id === check.name)) continue;
      const label = translate?.(`assistant.task.check.${check.status}`, labels[check.status]) ?? labels[check.status];
      const knownLabel = Object.prototype.hasOwnProperty.call(TASK_CHECK_LABELS, check.name) ? TASK_CHECK_LABELS[check.name] : undefined;
      const summary = knownLabel ? translate?.(`assistant.task.checkName.${check.name}`, knownLabel) ?? knownLabel : check.summary;
      sections.push(`${label}: ${summary}`);
    }
  }
  return {
    content: sections.join("\n\n"),
    state: (operation.task && taskResultStatus(operation.task) === "failed") || operation.verification?.status === "failed"
      || operation.fileChangeResult?.readBackVerified === false
      || (operation.fileChangesResult && operation.fileChangesResult.status !== "applied") ? "error" : "ready",
  };
}

export async function runAssistantWorkflow(
  initial: AssistantExecuteOperationOutput,
  callbacks: AssistantWorkflowCallbacks,
): Promise<AssistantWorkflowOutcome> {
  let operation = initial;
  let completedSteps = 0;
  let confirmations = 0;
  let lastOperation: AssistantExecuteOperationOutput | null = null;
  const tokens = new Set<string>();
  const conversationId = initial.conversationId;
  if (!conversationId?.trim()) throw new Error("Assistant conversation binding is missing.");
  let taskBinding = initial.task ? { ...initial.task } : null;
  let requirementsBinding = initial.task ? requirementBinding(initial.task.requirements) : null;

  function validateTaskBinding(next: AssistantExecuteOperationOutput, isCreationResult = false) {
    if (next.conversationId !== conversationId) throw new Error("Assistant conversation changed during the confirmation workflow.");
    const nextRequirements = next.task ? requirementBinding(next.task.requirements) : null;
    if (!taskBinding) {
      taskBinding = next.task ? { ...next.task } : null;
      requirementsBinding = nextRequirements;
      return;
    }
    const task = next.task;
    if (!task || task.id !== taskBinding.id || task.goal !== taskBinding.goal
      || task.preserveExistingMods !== taskBinding.preserveExistingMods
      || (taskBinding.instanceId === null && task.instanceId !== null
        && (!isCreationResult || next.instanceId !== task.instanceId))
      || (taskBinding.instanceId !== null && task.instanceId !== taskBinding.instanceId)
      || (taskBinding.moduleId !== null && task.moduleId !== taskBinding.moduleId)) {
      throw new Error("Assistant task binding changed during the confirmation workflow.");
    }
    if (nextRequirements !== requirementsBinding) {
      throw new Error("Assistant task requirements changed during the confirmation workflow.");
    }
    // Only the confirmed creation result may resolve the previously unbound instance.
    taskBinding = { ...task };
  }

  while (true) {
    validateTaskBinding(operation);
    if (operation.requiresConfirmation) {
      if (confirmations >= ASSISTANT_REQUEST_SAFETY_LIMIT) {
        await callbacks.cancelPending?.();
        return { status: "limit-reached", completedSteps, lastOperation, pendingOperation: operation };
      }
      const confirmationToken = operation.confirmationToken;
      const planSummary = operation.planSummary;
      if (!confirmationToken?.trim() || !planSummary?.trim() || tokens.has(confirmationToken.trim())) {
        throw new Error("Assistant preview did not include a new valid confirmation binding.");
      }
      const decision = !callbacks.shouldStop?.() && await callbacks.confirmPreview(operation);
      const confirmed = typeof decision === "boolean" ? decision : decision.confirmed;
      if (!confirmed || callbacks.shouldStop?.()) {
        await callbacks.cancelPending?.();
        return { status: "cancelled", completedSteps, lastOperation, pendingOperation: operation };
      }
      confirmations += 1;
      tokens.add(confirmationToken.trim());
      const confirmedAction = operation.action;
      operation = await callbacks.executeConfirmed({ conversationId, confirmationToken, planSummary,
        ...(typeof decision !== "boolean" && decision.continueTask ? { continueTask: true } : {}) });
      validateTaskBinding(operation, confirmedAction === "create_server" && operation.action === "create_server" && !operation.requiresConfirmation);
      if (operation.requiresConfirmation) {
        throw new Error("Assistant confirmation was not consumed.");
      }
    }

    // Publish evidence before asking about another step, including failed checks.
    const completed = operation.completedOperations ?? [];
    if (!Array.isArray(completed) || completed.length > ASSISTANT_REQUEST_SAFETY_LIMIT) throw new Error("Invalid assistant completed-operation history.");
    for (const step of completed) {
      if (!step || typeof step.message !== "string" || !step.instanceId || step.instanceId !== operation.instanceId) {
        throw new Error("Assistant completed-operation target changed.");
      }
      validateTaskBinding({ ...operation, ...step });
    }
    completedSteps += completed.length;
    lastOperation = operation;
    if (operation.handled && operation.action !== "none") completedSteps += 1;
    await callbacks.onResult(operation);
    if (callbacks.shouldStop?.()) {
      await callbacks.cancelPending?.();
      return { status: "cancelled", completedSteps, lastOperation, pendingOperation: operation.followUp ?? null };
    }
    if (operation.continuation) {
      return { status: "paused", completedSteps, lastOperation, pendingOperation: null };
    }
    if (operation.action === "patch_instance_text" && operation.fileChangeResult?.readBackVerified !== true) {
      return { status: operation.fileChangeResult ? "failed" : "inconclusive", completedSteps, lastOperation, pendingOperation: null };
    }
    if (operation.action === "patch_instance_files" && (operation.fileChangesResult?.status !== "applied"
      || operation.fileChangesResult.files.some((file) => file.state !== "applied" || !file.readBackVerified))) {
      return { status: operation.fileChangesResult ? "failed" : "inconclusive", completedSteps, lastOperation, pendingOperation: null };
    }
    const status = operation.verification?.status;
    const taskStatus = operation.task ? taskResultStatus(operation.task) : null;
    const followUp = operation.followUp;
    if (!followUp || operation.verification?.canContinue !== true) {
      return {
        status: status === "failed" || taskStatus === "failed" ? "failed"
          : operation.task ? taskStatus === "completed" ? "completed" : "inconclusive"
            : status === "inconclusive" || !operation.handled ? "inconclusive" : "completed",
        completedSteps,
        lastOperation,
        pendingOperation: null,
      };
    }
    if (!followUp.requiresConfirmation) {
      throw new Error("Assistant follow-up must be a new confirmation preview.");
    }
    operation = followUp;
  }
}
