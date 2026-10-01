import type {
  AssistantConfirmOperationInput,
  AssistantConversationState,
  AssistantProviderSettingsInput,
  AssistantExecuteOperationInput,
  AssistantExecuteOperationOutput,
  AssistantOperationAction,
  AssistantTaskReceipt,
  InstanceDetails,
  InstanceProvisioning,
} from "../types";
import { mockBootstrap } from "./bootstrap";
import { buildMockAssistantResponse } from "./catalogs";

type MockCommand = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;

const mockAssistantConfirmationStore = new Map<string, {
  conversationId: string;
  revision: number;
  action: AssistantOperationAction;
  task: AssistantTaskReceipt;
  summary: string;
  provider: string;
  model: string;
  baseUrl: string;
  expiresAt: number;
}>();
let mockAssistantConfirmationSequence = 0;
const mockConversations = new Map<string, { identity: string; revision: number; checkpoint?: AssistantExecuteOperationOutput | null }>();
let mockConversationSequence = 0;
function providerIdentity(settings: AssistantProviderSettingsInput): string {
  return JSON.stringify([settings.provider.trim(), settings.model.trim(), settings.baseUrl.trim().replace(/\/+$/, "")]);
}
export function createMockAssistantConversation(settings: AssistantProviderSettingsInput) {
  const conversationId = `mock-conversation-${++mockConversationSequence}`;
  mockConversations.set(conversationId, { identity: providerIdentity(settings), revision: 0 });
  return { conversationId, revision: 0 };
}
export function cancelMockAssistantTurn(conversationId: string) {
  const conversation = mockConversations.get(conversationId);
  if (conversation) conversation.checkpoint = null;
  for (const [token, pending] of mockAssistantConfirmationStore) {
    if (pending.conversationId === conversationId) mockAssistantConfirmationStore.delete(token);
  }
  return { conversationId, stopping: false };
}
export function getMockAssistantConversationState(conversationId: string, settings: AssistantProviderSettingsInput): AssistantConversationState {
  const conversation = mockConversations.get(conversationId);
  if (!conversation || conversation.identity !== providerIdentity(settings)) {
    return { conversationId, status: "unavailable", revision: null, continuation: null, progress: null, messages: [], messagesTruncated: false };
  }
  const continuation = conversation.checkpoint?.continuation ?? null;
  return { conversationId, status: continuation ? "paused" : "idle", revision: conversation.revision, continuation,
    progress: { revision: conversation.revision, cursor: 0, reset: true, text: "", events: [] }, messages: [], messagesTruncated: false };
}
export function listMockAssistantConversations(settings: AssistantProviderSettingsInput) {
  return [...mockConversations].filter(([, value]) => value.identity === providerIdentity(settings)).slice(-16)
    .map(([conversationId, value]) => ({ conversationId, revision: value.revision, title: "Browser preview conversation", updatedAtUnixMs: 0 }));
}
export function resumeMockAssistantConversation(conversationId: string, settings: AssistantProviderSettingsInput): AssistantExecuteOperationOutput {
  const conversation = requireConversation(conversationId, settings);
  const checkpoint = conversation.checkpoint;
  if (!checkpoint?.continuation?.canResume) throw new Error("No resumable assistant checkpoint is available.");
  conversation.checkpoint = null;
  return { ...checkpoint, continuation: null, conversationRevision: ++conversation.revision,
    message: "Mock investigation resumed from its saved evidence. Browser preview does not inspect the host." };
}
export function deleteMockAssistantConversation(conversationId: string) {
  const result = cancelMockAssistantTurn(conversationId);
  mockConversations.delete(conversationId);
  return result;
}
function requireConversation(conversationId: string, settings: AssistantProviderSettingsInput) {
  const conversation = mockConversations.get(conversationId);
  if (!conversation || conversation.identity !== providerIdentity(settings)) {
    throw new Error("Assistant conversation is unavailable or its provider changed.");
  }
  return conversation;
}

function cloneMockAssistantTask(task: AssistantTaskReceipt): AssistantTaskReceipt {
  return { ...task, requirements: task.requirements.map((requirement) => ({ ...requirement })), checks: task.checks.map((check) => ({ ...check })) };
}

function mockRequirementSummary(task: AssistantTaskReceipt): string {
  return task.requirements.length ? `\nRequirements (browser preview fixture):\n${task.requirements.map((requirement, index) =>
    `${index + 1}. ${requirement.description}; ${requirement.target} = ${requirement.expectedDisplay}`).join("\n")}` : "";
}

async function checkMockRequirements(task: AssistantTaskReceipt, invokeMock: MockCommand): Promise<void> {
  const details = task.instanceId && task.requirements.length
    ? await invokeMock<InstanceDetails>("read_instance_details_from_storage", { instanceId: task.instanceId }) : null;
  const settings: unknown = details ? JSON.parse(details.settings_json) : null;
  for (const requirement of task.requirements) {
    const matches = settings !== null && typeof settings === "object" && "bind_ip" in settings && settings.bind_ip === "0.0.0.0";
    task.checks.push({ name: requirement.id, status: details ? matches ? "satisfied" : "failed" : "unknown",
      summary: details ? matches ? "The fixture listen address matches the saved mock settings."
        : "The saved mock listen address no longer matches the confirmed fixture requirement."
        : "The fixture setting can be checked after its instance is created.",
      evidence: { requirementId: requirement.id },
    });
  }
}

export function previewMockAssistantOperation(input: AssistantExecuteOperationInput): AssistantExecuteOperationOutput {
  const conversation = requireConversation(input.conversationId, input.settings);
  if ("priorRequests" in input || "conversationMessages" in input) throw new Error("Client-provided conversation history is not accepted.");
  conversation.revision += 1;
  cancelMockAssistantTurn(input.conversationId);
  // These exact prompts exercise browser fixtures; production intent is resolved by the desktop backend.
  const goal = input.prompt === "Restore this server (browser preview fixture)" ? "restore_service"
    : input.prompt === "Create a server (browser preview fixture)" ? "launch_service" : "apply_change";
  const policy = { goal, preserveExistingMods: true } as const;
  if (policy.goal === "restore_service" && !input.selectedInstanceId) throw new Error("Select a server instance before requesting service recovery.");
  const instance = mockBootstrap.state.instances.find((item) => item.id === input.selectedInstanceId);
  const moduleId = instance?.module_id ?? input.selectedModuleId ?? null;
  const module = mockBootstrap.state.modules.find((item) => item.id === moduleId);
  if (policy.goal === "launch_service" && (input.selectedInstanceId ? !instance : !module)) throw new Error("Select an existing instance or a game for the new server.");
  const prompt = input.prompt.toLowerCase();
  const handled = policy.goal !== "apply_change" || /start|install|download|mod|broadcast|gm|rcon|spawn|配置|开服|启动|下载|广播/.test(prompt);
  const action: AssistantOperationAction = !handled ? "none" : policy.goal === "launch_service" && !input.selectedInstanceId
    ? module?.install_state === "Installed" ? "validate_server" : "install_server" : "start_server";
  const actionLabel = { install_server: "Install server files", validate_server: "Validate server files", create_server: "Create server without starting", start_server: "Start server" };
  const requiresConfirmation = handled;
  const confirmationToken = requiresConfirmation
    ? (++mockAssistantConfirmationSequence).toString(16).padStart(32, "0")
    : null;
  const task: AssistantTaskReceipt = { ...policy, id: `mock-task-${confirmationToken ?? ++mockAssistantConfirmationSequence}`,
    operationLimit: 32,
    instanceId: input.selectedInstanceId ?? null, moduleId,
    status: handled ? "proposed" : "inconclusive", checks: [], requirements: policy.goal === "launch_service" && !input.selectedInstanceId ? [{
      id: "requirement_1", kind: "setting", description: "Keep the fixture listen address (browser preview fixture)",
      sourceText: "Fixed browser preview fixture; no natural-language requirement extraction.",
      target: "bind_ip", expectedDisplay: "0.0.0.0",
    }] : [] };
  const summary = `${actionLabel[action as keyof typeof actionLabel] ?? "Inspect current state"}\nGoal: ${policy.goal === "launch_service" ? "Launch server" : policy.goal === "restore_service" ? "Restore service" : "Execute request"}\nPreserve existing mods: ${policy.preserveExistingMods ? "yes" : "no"}${mockRequirementSummary(task)}`;
  if (confirmationToken) {
    mockAssistantConfirmationStore.set(confirmationToken, {
      conversationId: input.conversationId,
      revision: conversation.revision,
      action,
      task: cloneMockAssistantTask(task),
      summary,
      provider: input.settings.provider,
      model: input.settings.model,
      baseUrl: input.settings.baseUrl.replace(/\/+$/, ""),
      expiresAt: Date.now() + 120_000
    });
  }
  const output: AssistantExecuteOperationOutput = {
    continuation: null,
    conversationId: input.conversationId,
    conversationRevision: conversation.revision,
    task,
    handled,
    action,
    message: handled
      ? requiresConfirmation
        ? `Pending confirmation: ${summary}`
        : "Mock assistant operation completed."
      : buildMockAssistantResponse({
        settings: input.settings,
        promptLabel: input.prompt,
        prompt: input.prompt,
        context: input.context ?? ""
      }),
    requiresConfirmation,
    confirmationToken,
    confirmationExpiresAtUnixMs: requiresConfirmation ? Date.now() + 120_000 : null,
    planSummary: handled ? summary : null,
    instanceId: input.selectedInstanceId ?? null,
    moduleId,
    appliedSettingsKeys: [],
    rejectedSettingsKeys: [],
    appliedPortNames: [],
    rejectedPortNames: [],
    workshopItemIds: [],
    modReferences: [],
    resolvedModIds: [],
    sourcePaths: [],
    runtimeCommands: [],
    runtimeResponseTexts: [],
    configDocumentCount: 0,
    assistantReason: handled ? "mock operation" : null
  };
  if (["Pause an investigation (browser preview fixture)", "Fail an investigation (browser preview fixture)"].includes(input.prompt)) {
    output.continuation = { reason: input.prompt.startsWith("Fail ") ? "investigation_failed" : "model_slice", summary: "Mock investigation paused with saved evidence.", canResume: true };
    conversation.checkpoint = output;
  }
  return output;
}

export async function confirmMockAssistantOperation(
  input: AssistantConfirmOperationInput,
  invokeMock: MockCommand,
): Promise<AssistantExecuteOperationOutput> {
  const conversation = requireConversation(input.conversationId, input.settings);
  const token = input.confirmationToken.trim().toLowerCase();
  const pending = mockAssistantConfirmationStore.get(token);
  mockAssistantConfirmationStore.delete(token);
  if (!pending
    || pending.conversationId !== input.conversationId || pending.revision !== conversation.revision
    || pending.expiresAt <= Date.now()
    || pending.summary !== input.planSummary.trim()
    || pending.provider !== input.settings.provider
    || pending.model !== input.settings.model
    || pending.baseUrl !== input.settings.baseUrl.replace(/\/+$/, "")) {
    throw new Error("Assistant confirmation expired, was already used, or does not match the preview.");
  }
  let task: AssistantTaskReceipt = { ...cloneMockAssistantTask(pending.task), status: "inconclusive", checks: [{ name: "new_run_ready", status: "unknown", summary: "Browser preview does not start a real server or verify recovery.", evidence: null }] };
  if (pending.action === "install_server" || pending.action === "validate_server") {
    await invokeMock(pending.action === "install_server" ? "install_module_game" : "validate_module_game", { moduleId: task.moduleId });
  }
  if (pending.action === "create_server" && task.moduleId) {
    const created = await invokeMock<InstanceProvisioning>("create_instance_record", { input: { module_id: task.moduleId, name: "LAN Preview Server" } });
    task = { ...task, instanceId: created.summary.id };
    task.checks.push({ name: "instance_created", status: "satisfied", summary: "Mock instance created without starting.", evidence: null });
  }
  await checkMockRequirements(task, invokeMock);
  const output: AssistantExecuteOperationOutput = {
    continuation: null,
    conversationId: input.conversationId,
    conversationRevision: ++conversation.revision,
    task,
    handled: true,
    action: pending.action,
    message: pending.action === "create_server" ? "Mock instance created; it has not been started."
      : pending.action === "customize_config" ? "Mock default configuration confirmed. Browser preview retains its fixture values and does not write native configuration files."
        : "Mock assistant operation completed.",
    requiresConfirmation: false,
    confirmationToken: null,
    confirmationExpiresAtUnixMs: null,
    planSummary: input.planSummary,
    instanceId: task.instanceId,
    moduleId: task.moduleId,
    appliedSettingsKeys: [],
    rejectedSettingsKeys: [],
    appliedPortNames: [],
    rejectedPortNames: [],
    workshopItemIds: [],
    modReferences: [],
    resolvedModIds: [],
    sourcePaths: [],
    runtimeCommands: [],
    runtimeResponseTexts: [],
    configDocumentCount: 0,
    assistantReason: "mock confirmed operation"
  };
  const nextAction = pending.task.goal === "launch_service"
    ? pending.action === "install_server" ? "validate_server"
      : pending.action === "validate_server" ? "create_server"
        : pending.action === "create_server" ? "customize_config"
          : pending.action === "customize_config" ? "start_server" : null : null;
  if (nextAction && !task.checks.some((check) => check.status === "failed")) {
    const nextTask = { ...task, status: "proposed" as const, checks: [] };
    const confirmationToken = (++mockAssistantConfirmationSequence).toString(16).padStart(32, "0");
    const nextLabel = nextAction === "validate_server" ? "Validate server files"
      : nextAction === "create_server" ? "Create server without starting"
        : nextAction === "customize_config" ? "Confirm default configuration (browser preview fixture; no native file writes)"
          : "Start the created server";
    const summary = `${nextLabel}\nGoal: Launch server\nPreserve existing mods: ${task.preserveExistingMods ? "yes" : "no"}${mockRequirementSummary(nextTask)}`;
    mockAssistantConfirmationStore.set(confirmationToken, { ...pending, revision: conversation.revision, action: nextAction, task: cloneMockAssistantTask(nextTask), summary, expiresAt: Date.now() + 120_000 });
    output.verification = { status: "inconclusive", summary: "Another operation requires separate confirmation.", canContinue: true, runId: null, evidence: null };
    output.followUp = { ...output, task: nextTask, action: nextAction, message: `Pending confirmation: ${summary}`, requiresConfirmation: true,
      confirmationToken, confirmationExpiresAtUnixMs: Date.now() + 120_000, planSummary: summary, verification: null, followUp: null };
  }
  return output;
}
