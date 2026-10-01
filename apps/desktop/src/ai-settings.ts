import { selectLocaleText } from "./i18n";

export type AiProvider = "openai-compatible" | "anthropic-compatible" | "ollama";
export type AiSettingsMissingField = "model" | "baseUrl" | "apiKey";

export interface AiSettings {
  enabled: boolean;
  provider: AiProvider;
  model: string;
  baseUrl: string;
  apiKey: string;
  apiKeyStored: boolean;
}

export type PersistAiSettings = (settings: AiSettings) => Promise<AiSettings | null>;

export function sameAiSettings(left: AiSettings, right: AiSettings): boolean {
  return left.enabled === right.enabled && left.provider === right.provider &&
    left.model === right.model && left.baseUrl === right.baseUrl &&
    left.apiKey === right.apiKey && left.apiKeyStored === right.apiKeyStored;
}

export function mergePersistedAiSettings(
  draft: AiSettings,
  submitted: AiSettings,
  persisted: AiSettings
): AiSettings {
  if (sameAiSettings(draft, submitted)) {
    return persisted;
  }

  // Acknowledging a key write must preserve edits made while the credential store was busy.
  if (draft.provider === submitted.provider && draft.baseUrl.trim() === submitted.baseUrl.trim() &&
    draft.apiKey === submitted.apiKey && draft.apiKeyStored === submitted.apiKeyStored) {
    return { ...draft, apiKey: persisted.apiKey, apiKeyStored: persisted.apiKeyStored };
  }
  return draft;
}

export interface AiProviderPreset {
  id: AiProvider;
  defaultBaseUrl: string;
  defaultModel: string;
  requiresApiKey: boolean;
  deployment: "cloud" | "local";
}

export interface AiSettingsStatus {
  ready: boolean;
  enabled: boolean;
  providerLabel: string;
  deployment: "cloud" | "local";
  requiresApiKey: boolean;
  missing: AiSettingsMissingField[];
}

const STORAGE_KEY = "langame.ai.settings";
const DEFAULT_PROVIDER: AiProvider = "openai-compatible";
const PROVIDER_ORDER: AiProvider[] = ["openai-compatible", "anthropic-compatible", "ollama"];
const AI_PROVIDER_LABELS: Record<AiProvider, { english: string; chinese: string }> = {
  "openai-compatible": { english: "OpenAI Compatible", chinese: "OpenAI 兼容" },
  "anthropic-compatible": { english: "Anthropic Compatible", chinese: "Anthropic 兼容" },
  ollama: { english: "Ollama", chinese: "Ollama" }
};

const AI_PROVIDER_PRESETS: Record<AiProvider, AiProviderPreset> = {
  "openai-compatible": {
    id: "openai-compatible",
    defaultBaseUrl: "https://api.openai.com/v1",
    defaultModel: "",
    requiresApiKey: true,
    deployment: "cloud"
  },
  "anthropic-compatible": {
    id: "anthropic-compatible",
    defaultBaseUrl: "https://api.anthropic.com/v1",
    defaultModel: "",
    requiresApiKey: true,
    deployment: "cloud"
  },
  ollama: {
    id: "ollama",
    defaultBaseUrl: "http://127.0.0.1:11434/v1",
    defaultModel: "",
    requiresApiKey: false,
    deployment: "local"
  }
};

function normalizeProvider(value: unknown): AiProvider | null {
  const candidate = typeof value === "string" ? value.trim().toLowerCase() : "";
  return PROVIDER_ORDER.find((provider) => provider === candidate) ?? null;
}

function normalizeString(value: unknown): string {
  return typeof value === "string" ? value.trim() : "";
}

export function getAiProviderPreset(provider: AiProvider): AiProviderPreset {
  return AI_PROVIDER_PRESETS[provider];
}

export function formatAiProviderLabel(provider: AiProvider, locale?: string): string {
  const labels = AI_PROVIDER_LABELS[provider];
  return selectLocaleText(locale, labels.chinese, labels.english);
}

export function listAiProviderPresets(): AiProviderPreset[] {
  return PROVIDER_ORDER.map((provider) => AI_PROVIDER_PRESETS[provider]);
}

export function createDefaultAiSettings(provider: AiProvider = DEFAULT_PROVIDER): AiSettings {
  const preset = getAiProviderPreset(provider);
  return {
    enabled: true,
    provider,
    model: preset.defaultModel,
    baseUrl: preset.defaultBaseUrl,
    apiKey: "",
    apiKeyStored: false
  };
}

export function applyAiProviderPreset(current: AiSettings, provider: AiProvider): AiSettings {
  const preset = getAiProviderPreset(provider);
  return {
    ...current,
    enabled: true,
    provider,
    model: preset.defaultModel,
    baseUrl: preset.defaultBaseUrl,
    apiKey: "",
    apiKeyStored: false
  };
}

export function applyAiServiceUrl(current: AiSettings, baseUrl: string): AiSettings {
  if (current.baseUrl === baseUrl) {
    return current;
  }
  return { ...current, baseUrl, apiKey: "", apiKeyStored: false };
}

const MAX_PENDING_AI_SETTINGS_WRITES = 8;

export class AiSettingsWriteQueue {
  private tail: Promise<void> = Promise.resolve();
  private pendingCount = 0;
  private generation = 0;

  get revision(): number {
    return this.generation;
  }

  run(write: () => Promise<AiSettings>): Promise<AiSettings | null> {
    if (this.pendingCount >= MAX_PENDING_AI_SETTINGS_WRITES) {
      return Promise.reject(new Error("AI settings are busy. Wait for the current save to finish."));
    }
    this.pendingCount += 1;
    const generation = ++this.generation;
    const operation = this.tail.then(write);
    // Credential writes and clears must finish in order, including after a failed write.
    this.tail = operation.then(() => undefined, () => undefined);
    return operation.then((settings) => generation === this.generation ? settings : null)
      .finally(() => { this.pendingCount -= 1; });
  }
}

export function normalizeAiSettings(value: unknown): AiSettings {
  if (!value || typeof value !== "object") {
    return createDefaultAiSettings();
  }

  const record = value as Partial<Record<keyof AiSettings, unknown>>;
  const provider = normalizeProvider(record.provider);
  if (!provider) {
    return createDefaultAiSettings();
  }
  const preset = getAiProviderPreset(provider);

  return {
    enabled: record.enabled === undefined ? true : Boolean(record.enabled),
    provider,
    model: normalizeString(record.model) || preset.defaultModel,
    baseUrl: normalizeString(record.baseUrl) || preset.defaultBaseUrl,
    apiKey: normalizeString(record.apiKey),
    apiKeyStored: Boolean(record.apiKeyStored)
  };
}

export function loadAiSettings(): AiSettings {
  if (typeof window === "undefined") {
    return createDefaultAiSettings();
  }

  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) {
      return createDefaultAiSettings();
    }
    return normalizeAiSettings(JSON.parse(raw));
  } catch {
    return createDefaultAiSettings();
  }
}

export function persistAiSettings(settings: AiSettings) {
  if (typeof window === "undefined") {
    return;
  }

  const normalized = normalizeAiSettings(settings);
  window.localStorage.setItem(
    STORAGE_KEY,
    JSON.stringify({
      ...normalized,
      apiKey: ""
    })
  );
}

export function getAiSettingsStatus(settings: AiSettings): AiSettingsStatus {
  const preset = getAiProviderPreset(settings.provider);
  const missing: AiSettingsMissingField[] = [];
  const hasApiKey = Boolean(settings.apiKey.trim()) || settings.apiKeyStored;

  if (settings.enabled) {
    if (!settings.model.trim()) {
      missing.push("model");
    }
    if (!settings.baseUrl.trim()) {
      missing.push("baseUrl");
    }
    if (preset.requiresApiKey && !hasApiKey) {
      missing.push("apiKey");
    }
  }

  return {
    ready: settings.enabled && missing.length === 0,
    enabled: settings.enabled,
    providerLabel: formatAiProviderLabel(settings.provider),
    deployment: preset.deployment,
    requiresApiKey: preset.requiresApiKey,
    missing
  };
}
