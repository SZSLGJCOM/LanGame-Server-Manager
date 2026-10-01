import {
  applyPlayerAccessMutation,
  isHumanitzNetId,
  findPlayerAccessMutationConflict,
  decodePlayerAccessEntries,
  normalizePlayerAccessMutationEntry,
  parsePlayerAccessCodec,
  playerAccessLiveTarget,
  playerAccessMutationActionId,
  parsePlayerAccessSync,
  type PlayerAccessMutationOperation,
  type PlayerAccessSchemaProperty
} from "../domain/player-access";

export interface MockPlayerAccessMutationInput {
  expectedValue?: unknown;
  operation: PlayerAccessMutationOperation;
  value: unknown;
}

export interface MockPlayerAccessMutationContext {
  fieldKey: string;
  properties: Record<string, PlayerAccessSchemaProperty>;
  settings: Record<string, unknown>;
}

export interface MockPlayerAccessMutationOutcome {
  changed: boolean;
  liveTarget: string;
  liveStatus: "sent_unverified" | "not_running" | "restart_required";
  value: unknown;
  verificationStatus: "unavailable";
}

export function applyMockPlayerAccessMutation(
  property: PlayerAccessSchemaProperty,
  currentValue: unknown,
  input: MockPlayerAccessMutationInput,
  running: boolean,
  context?: MockPlayerAccessMutationContext
): MockPlayerAccessMutationOutcome {
  if (context) {
    const conflict = findPlayerAccessMutationConflict(
      context.fieldKey,
      property,
      context.properties,
      context.settings,
      input.operation,
      input.value
    );
    if (conflict) {
      throw new Error(conflict);
    }
  }
  const patched = applyPlayerAccessMutation(
    property,
    currentValue,
    input.operation,
    input.value,
    input.expectedValue
  );
  const sync = parsePlayerAccessSync(property);
  if (!sync) {
    throw new Error("Player-access sync metadata is invalid.");
  }

  const codec = parsePlayerAccessCodec(property);
  const normalized = codec ? normalizePlayerAccessMutationEntry(codec, input.value, property, input.operation) : null;
  const existing = input.operation === "remove" && normalized
    ? decodePlayerAccessEntries(context?.fieldKey ?? "", property, currentValue).find((entry) => entry.key === normalized.identity)
    : undefined;
  const liveTarget = playerAccessLiveTarget(property, existing?.rawValue ?? input.value, input.operation);
  if (liveTarget === null) throw new Error("Player-access value does not provide a valid runtime target.");

  const retainedHumanitzRemoval = codec === "humanitz_net_id" && input.operation === "remove" && !isHumanitzNetId(liveTarget);
  return {
    ...patched,
    liveTarget,
    liveStatus: !running
      ? "not_running"
      : retainedHumanitzRemoval || playerAccessMutationActionId(sync, input.operation) === null ? "restart_required" : "sent_unverified",
    verificationStatus: "unavailable"
  };
}
