import type { ModulePlayerActionDetails } from "../types";
import { isArkModule } from "../ark-clusters";

export interface MockDeclaredRuntimeActionDispatch {
  command: string;
  processKey: string;
  transport: string;
}

type MockPalworldCommand =
  | { operation: "players" | "save" }
  | { operation: "announce"; message: string }
  | { operation: "kick" | "ban" | "unban"; userid: string };

function renderPalworldRestCommand(
  action: ModulePlayerActionDetails,
  targetValue: unknown,
  roleValue: unknown
): string {
  if (roleValue !== null && roleValue !== undefined && roleValue !== "") {
    throw new Error("Palworld REST actions do not accept roles or request tokens.");
  }
  if (targetValue !== null && targetValue !== undefined && typeof targetValue !== "string") {
    throw new Error("The Palworld REST action target is invalid.");
  }
  const target = targetValue ?? "";
  const contract = `${action.id}:${action.command_template.trim()}`;
  let command: MockPalworldCommand;
  switch (contract) {
    case "show_players:players": command = { operation: "players" }; break;
    case "save_world:save": command = { operation: "save" }; break;
    case "broadcast:announce {{target}}": command = { operation: "announce", message: target }; break;
    case "kick_player:kick {{target}}": command = { operation: "kick", userid: target }; break;
    case "ban_player:ban {{target}}": command = { operation: "ban", userid: target }; break;
    case "unban_player:unban {{target}}": command = { operation: "unban", userid: target }; break;
    default: throw new Error("The Palworld REST action contract is unsupported.");
  }
  const bindsTarget = "message" in command || "userid" in command;
  if (!bindsTarget && (action.target_required || target !== "")) {
    throw new Error("The Palworld REST action target does not match its contract.");
  }
  if (bindsTarget && (!target.trim()
    || [...target].length > (command.operation === "announce" ? 240 : 128)
    || /[\u0000-\u001f\u007f-\u009f]/u.test(target))) {
    throw new Error("The Palworld REST action target is invalid.");
  }
  // REST values are JSON data: preserve spaces and punctuation without command-line interpolation.
  return JSON.stringify(command);
}

function normalizeRuntimeActionValue(value: unknown, field: "target" | "role"): string {
  const raw = String(value ?? "");
  if ([...raw].some((character) => /[\u0000-\u001f\u007f]/.test(character))
    || raw.includes("{{")
    || raw.includes("}}")
    || [";", "&&", "||", "`", "$("].some((fragment) => raw.includes(fragment))) {
    throw new Error(`Runtime action ${field} contains unsupported command syntax.`);
  }
  return raw.trim();
}

function renderRuntimeActionCommand(
  action: ModulePlayerActionDetails,
  targetValue: unknown,
  roleValue: unknown
): string {
  if (action.transport === "palworld_rest") {
    return renderPalworldRestCommand(action, targetValue, roleValue);
  }
  const template = action.command_template.trim();
  if (!template || [...template].some((character) => /[\u0000-\u001f\u007f]/.test(character))) {
    throw new Error(`Runtime action ${action.id} must declare one command line.`);
  }

  const usesTarget = template.includes("{{target}}");
  const usesRole = template.includes("{{role}}");
  const target = normalizeRuntimeActionValue(targetValue, "target");
  const role = normalizeRuntimeActionValue(roleValue, "role");
  if ((action.target_required || usesTarget) && !target) {
    throw new Error(`Runtime action ${action.id} requires a non-empty target.`);
  }
  if (!usesTarget && target) {
    throw new Error(`Runtime action ${action.id} does not accept a target.`);
  }
  if (usesRole) {
    if (!role || !/^[A-Za-z0-9_.:-]+$/.test(role) || !(action.role_values ?? []).includes(role)) {
      throw new Error(`Runtime action ${action.id} requires a declared role.`);
    }
  } else if (role) {
    throw new Error(`Runtime action ${action.id} does not accept a role.`);
  }

  let encodedTarget = target;
  if (action.target_encoding === "quoted_string") {
    encodedTarget = `"${target.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;
  } else if (action.target_encoding && action.target_encoding !== "raw") {
    throw new Error(`Runtime action ${action.id} declares an unsupported target encoding.`);
  } else if (/\s/.test(target)) {
    throw new Error(`Runtime action ${action.id} requires a single-token target.`);
  }

  const rendered = template
    .split("{{target}}").join(encodedTarget)
    .split("{{role}}").join(role)
    .trim();
  if (!rendered || rendered.includes("{{") || rendered.includes("}}")) {
    throw new Error(`Runtime action ${action.id} contains an unresolved template placeholder.`);
  }
  if (new TextEncoder().encode(rendered).length > 512) {
    throw new Error("Runtime commands are limited to 512 characters per dispatch.");
  }
  return rendered;
}

export function resolveMockDeclaredRuntimeAction(
  actions: readonly ModulePlayerActionDetails[],
  moduleId: string,
  input: Record<string, unknown>
): MockDeclaredRuntimeActionDispatch | null {
  const actionId = String(input.runtimeActionId ?? "").trim();
  const hasRuntimeActionMetadata = actionId
    || input.runtimeActionTarget !== undefined
    || input.runtimeActionRole !== undefined;
  if (!hasRuntimeActionMetadata) {
    return null;
  }
  if (!actionId) {
    throw new Error("runtimeActionId is required when runtime action metadata is provided.");
  }
  const action = actions.find((candidate) => candidate.id === actionId);
  if (!action) {
    throw new Error(`Runtime action ${actionId} is not declared by module ${moduleId}.`);
  }
  if (action.transport === "palworld_rest" && moduleId !== "palworld") {
    throw new Error("Palworld REST actions require the Palworld module.");
  }
  return {
    command: renderRuntimeActionCommand(action, input.runtimeActionTarget, input.runtimeActionRole),
    processKey: String(action.process_key ?? (isArkModule(moduleId) ? input.processKey ?? "main" : "master")).trim()
      || (isArkModule(moduleId) ? "main" : "master"),
    transport: String(action.transport ?? "stdin").trim() || "stdin"
  };
}
