export interface ConfigurationSaveFailure {
  state: "failed" | "conflict";
  message: string;
}

const CONFLICT_ERROR_PATTERNS = [
  /\bprecondition(?:[-_\s]*failed)?\b/i,
  /\bcompare[-_\s]*and[-_\s]*set\b/i,
  /\bcas(?:[-_\s]*conflict)?\b/i,
  /settings changed while this edit was pending/i,
  /\bsettings[-_\s]*conflict\b/i
];

function readErrorText(error: unknown): { message: string; classifier: string } {
  if (error instanceof Error) {
    const code = (error as Error & { code?: unknown }).code;
    const codeText = typeof code === "string" ? code : "";
    return {
      message: error.message || error.name,
      classifier: `${error.name} ${codeText} ${error.message}`
    };
  }
  if (typeof error === "string") {
    return { message: error, classifier: error };
  }
  if (error && typeof error === "object") {
    const record = error as Record<string, unknown>;
    const message = typeof record.message === "string" ? record.message : "";
    const code = typeof record.code === "string" ? record.code : "";
    if (message || code) {
      return {
        message: message || code,
        classifier: `${code} ${message}`
      };
    }
  }
  const message = String(error ?? "").trim();
  return {
    message: message || "Unable to save configuration.",
    classifier: message
  };
}

export function normalizeConfigurationSaveError(error: unknown): ConfigurationSaveFailure {
  const normalized = readErrorText(error);
  return {
    state: CONFLICT_ERROR_PATTERNS.some((pattern) => pattern.test(normalized.classifier))
      ? "conflict"
      : "failed",
    message: normalized.message
  };
}
