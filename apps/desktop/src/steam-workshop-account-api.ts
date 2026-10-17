import { canUseLocalDesktopApi, invokeOrMock } from "./api-transport";

export type SteamWorkshopAccountPhase = "ready" | "input_submitted" | "queued" | "preparing_steamcmd" | "connecting" | "authenticating" | "verifying_cache" | "guard_code" | "email_code" | "password" | "steam_guard"
  | "mobile_confirmation" | "authenticated" | "failed" | "cancelled";
export interface SteamWorkshopAccountTarget { kind: "instance" | "module"; id: string }
export interface SteamWorkshopAccountLoginSnapshot {
  session_id: string;
  target: SteamWorkshopAccountTarget;
  revision: number;
  accepted_input_revision: number | null;
  phase: SteamWorkshopAccountPhase;
  input_mode: "command" | "secret" | "disabled";
  events: SteamWorkshopAccountPhase[];
  account_name: string | null;
  steamcmd_root: string | null;
  error: string | null;
}

const phaseInputModes: Record<SteamWorkshopAccountPhase, SteamWorkshopAccountLoginSnapshot["input_mode"]> = {
  ready: "command", input_submitted: "disabled", queued: "disabled", preparing_steamcmd: "disabled", connecting: "disabled",
  authenticating: "disabled", verifying_cache: "disabled", guard_code: "secret", email_code: "secret", password: "secret", steam_guard: "secret",
  mobile_confirmation: "disabled", authenticated: "disabled", failed: "disabled", cancelled: "disabled"
};
const phases = new Set<string>(Object.keys(phaseInputModes));

const inputErrorCodes = ["steam_account_name_invalid", "steam_account_command_invalid", "steam_account_prompt_changed",
  "steam_account_input_invalid", "steam_account_input_pending", "steam_account_input_closed", "steam_account_session_unavailable",
  "steam_account_backend_unavailable", "steam_account_backend_out_of_date", "steam_account_interactive_unsupported",
  "steam_account_input_unconfirmed"] as const;
export type SteamWorkshopAccountInputErrorCode = typeof inputErrorCodes[number];

/** Only an allowlisted code crosses the credential submission error boundary. */
export class SteamWorkshopAccountInputError extends Error {
  constructor(readonly code: SteamWorkshopAccountInputErrorCode) { super(code); }
}

function safeInputErrorCode(error: unknown): SteamWorkshopAccountInputErrorCode {
  if (error instanceof SteamWorkshopAccountInputError) return error.code;
  let value: unknown = error instanceof Error ? error.message : error;
  if (typeof value === "string") {
    try { value = JSON.parse(value); } catch { return "steam_account_input_unconfirmed"; }
  }
  if (value !== null && typeof value === "object" && "code" in value && typeof value.code === "string") {
    return inputErrorCodes.find(code => code === value.code) ?? "steam_account_input_unconfirmed";
  }
  return "steam_account_input_unconfirmed";
}

export function canUseSteamWorkshopAccountLogin(): boolean { return canUseLocalDesktopApi(); }
function validateTarget(target: SteamWorkshopAccountTarget) {
  if (!target || !["instance", "module"].includes(target.kind) || typeof target.id !== "string" || !target.id
    || Object.keys(target).length !== 2) throw new SteamWorkshopAccountInputError("steam_account_backend_out_of_date");
}

async function request(command: string, target: SteamWorkshopAccountTarget, sessionId: string, extra: Record<string, unknown> = {}) {
  if (!canUseSteamWorkshopAccountLogin()) {
    throw new Error(JSON.stringify({ code: "steam_account_interactive_unsupported", message: "Steam account login requires the local desktop." }));
  }
  validateTarget(target);
  const value = await invokeOrMock<SteamWorkshopAccountLoginSnapshot>(command, { target, sessionId, ...extra });
  if (!value || !value.target || value.target.kind !== target.kind || value.target.id !== target.id
    || Object.keys(value.target).length !== 2 || "instance_id" in value || value.session_id !== sessionId
    || !Number.isSafeInteger(value.revision) || value.revision < 1 || !phases.has(value.phase)
    || (value.accepted_input_revision !== null && (!Number.isSafeInteger(value.accepted_input_revision)
      || value.accepted_input_revision < 1 || value.accepted_input_revision >= value.revision))
    || value.input_mode !== phaseInputModes[value.phase]
    || !Array.isArray(value.events) || value.events.length > 32 || value.events.some(phase => !phases.has(phase))
    || (value.account_name !== null && typeof value.account_name !== "string")
    || (value.steamcmd_root !== null && typeof value.steamcmd_root !== "string")
    || (value.error !== null && typeof value.error !== "string")
    || (value.phase === "authenticated" && (!value.account_name || !value.steamcmd_root))) {
    throw new SteamWorkshopAccountInputError("steam_account_backend_out_of_date");
  }
  return value;
}

export const startSteamWorkshopAccountLogin = (target: SteamWorkshopAccountTarget, operationId: string) =>
  request("start_steam_workshop_account_login", target, operationId);
export const readSteamWorkshopAccountLogin = (target: SteamWorkshopAccountTarget, sessionId: string) =>
  request("read_steam_workshop_account_login", target, sessionId);
export async function sendSteamWorkshopAccountInput(target: SteamWorkshopAccountTarget, sessionId: string, expectedRevision: number, input: string) {
  if (input.length === 0) throw new SteamWorkshopAccountInputError("steam_account_input_invalid");
  try {
    return await request("send_steam_workshop_account_input", target, sessionId, { expectedRevision, input });
  } catch (reason) {
    const code = safeInputErrorCode(reason);
    if (code === "steam_account_interactive_unsupported") throw new SteamWorkshopAccountInputError(code);
    // The request may have been accepted before its reply was lost. Read only
    // its receipt; never retry the password or code after an unknown outcome.
    let recovered: SteamWorkshopAccountLoginSnapshot;
    try { recovered = await readSteamWorkshopAccountLogin(target, sessionId); }
    catch { throw new SteamWorkshopAccountInputError(code); }
    if (recovered.accepted_input_revision === expectedRevision) return recovered;
    throw new SteamWorkshopAccountInputError(code);
  }
}
export const cancelSteamWorkshopAccountLogin = (target: SteamWorkshopAccountTarget, sessionId: string) =>
  request("cancel_steam_workshop_account_login", target, sessionId);
export async function clearSteamServerAccountAuthorization(target: SteamWorkshopAccountTarget): Promise<void> {
  if (!canUseSteamWorkshopAccountLogin()) throw new SteamWorkshopAccountInputError("steam_account_interactive_unsupported");
  validateTarget(target);
  await invokeOrMock<void>("clear_steam_server_account_authorization", { target });
}
