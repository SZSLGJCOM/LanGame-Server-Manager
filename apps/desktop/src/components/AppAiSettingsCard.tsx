import { ActivityNotice } from "./ActivityNotice";
import { useEffect, useRef, useState } from "react";
import {
  applyAiProviderPreset,
  applyAiServiceUrl,
  formatAiProviderLabel,
  getAiProviderPreset,
  listAiProviderPresets,
  mergePersistedAiSettings,
  normalizeAiSettings,
  sameAiSettings,
  type AiProvider,
  type AiSettings,
  type PersistAiSettings
} from "../ai-settings";
import { listOllamaModels } from "../api";
import { isChineseLocale, useI18n } from "../i18n";
import { ShellIcon } from "./ShellIcon";
import { AppAiConnectionCheck } from "./AppAiConnectionCheck";
import { AiDataDisclosure } from "./AiDataDisclosure";
import "./app-ai-settings.css";

interface AppAiSettingsCardProps {
  settings: AiSettings;
  onClearSecret: PersistAiSettings;
  onSave: PersistAiSettings;
}

function describeInlineError(error: unknown) {
  if (error instanceof Error && error.message.trim()) {
    return error.message.trim();
  }

  return String(error);
}

export function AppAiSettingsCard({ settings, onClearSecret, onSave }: AppAiSettingsCardProps) {
  const { locale } = useI18n();
  const [draft, setDraft] = useState<AiSettings>(settings);
  const [ollamaModels, setOllamaModels] = useState<string[]>([]);
  const [ollamaState, setOllamaState] = useState<"idle" | "loading" | "ready" | "empty" | "error">("idle");
  const [ollamaError, setOllamaError] = useState<string | null>(null);
  const [ollamaReloadKey, setOllamaReloadKey] = useState(0);
  const [persistenceError, setPersistenceError] = useState<{ message: string; operation: "save" | "clear" } | null>(null);
  const [saveRetryKey, setSaveRetryKey] = useState(0);
  const [clearingSecret, setClearingSecret] = useState(false);
  const [saveState, setSaveState] = useState<"idle" | "pending" | "saving" | "saved">("idle");
  const lastSettingsRef = useRef(settings);
  const mountedRef = useRef(true);
  const writeGenerationRef = useRef(0);
  const editRevisionRef = useRef(0);
  const pendingSaveRef = useRef<(() => void) | null>(null);
  const chinese = isChineseLocale(locale);
  const copy = chinese
    ? {
        clearSecret: "清除密钥",
        clearingSecret: "正在清除",
        retrySave: "重试保存",
        retryClear: "重试清除",
        providerPreset: "接口协议",
        protocolHelp: "协议说明",
        protocolNote: "OpenAI 兼容、Anthropic 兼容和 Ollama 表示接口协议；接收方由服务地址决定。",
        model: "模型",
        baseUrl: "服务地址",
        apiKey: "API Key",
        apiKeyHint: "密钥保存在管理端本机，并发送给配置的服务用于认证。",
        storedKeyHint: "密钥已保存在管理端本机，发送给配置的服务用于认证；输入新密钥可替换。",
        autoSave: "修改后自动保存",
        savePending: "等待保存…",
        saving: "正在保存…",
        saved: "已保存到本机",
        saveFailed: "保存失败",
        clearFailed: "清除失败",
        refreshModels: "刷新模型",
        refreshingModels: "读取中...",
        ollamaLoading: "正在读取配置地址的 Ollama 模型列表...",
        ollamaEmpty: "当前地址下没有发现模型。",
        ollamaError: (message: string) => `Ollama 连接失败：${message}`
      }
    : {
        clearSecret: "Clear key",
        clearingSecret: "Clearing key",
        retrySave: "Retry save",
        retryClear: "Retry clear",
        providerPreset: "API protocol",
        protocolHelp: "About API protocols",
        protocolNote: "OpenAI-compatible, Anthropic-compatible and Ollama describe API protocols. The service URL determines the recipient.",
        model: "Model",
        baseUrl: "Service URL",
        apiKey: "API Key",
        apiKeyHint: "The key is stored on the management host and sent to the configured service for authentication.",
        storedKeyHint: "Key saved on the management host and sent to the configured service for authentication. Enter a new key to replace it.",
        autoSave: "Changes save automatically",
        savePending: "Waiting to save…",
        saving: "Saving…",
        saved: "Saved on this device",
        saveFailed: "Save failed",
        clearFailed: "Could not clear key",
        refreshModels: "Refresh models",
        refreshingModels: "Loading...",
        ollamaLoading: "Reading the configured Ollama model list...",
        ollamaEmpty: "No models were found at this address.",
        ollamaError: (message: string) => `Ollama connection failed: ${message}`
      };

  useEffect(() => {
    const previousSettings = lastSettingsRef.current;
    lastSettingsRef.current = settings;
    setDraft((current) => sameAiSettings(current, previousSettings) ? settings : current);
  }, [settings]);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      pendingSaveRef.current?.();
    };
  }, []);

  useEffect(() => {
    if (draft.provider !== "ollama") {
      setOllamaModels([]);
      setOllamaState("idle");
      setOllamaError(null);
      return;
    }

    let cancelled = false;
    const baseUrl = draft.baseUrl.trim() || getAiProviderPreset("ollama").defaultBaseUrl;

    setOllamaState("loading");
    setOllamaError(null);

    void listOllamaModels(baseUrl)
      .then((models) => {
        if (cancelled) {
          return;
        }

        const normalizedModels = models
          .map((model) => model.trim())
          .filter((model, index, all) => model.length > 0 && all.indexOf(model) === index);

        setOllamaModels(normalizedModels);
        setOllamaState(normalizedModels.length > 0 ? "ready" : "empty");
        setDraft((current) => {
          if (current.provider !== "ollama") {
            return current;
          }

          const currentModel = current.model.trim();
          if (normalizedModels.length === 0) {
            return currentModel ? { ...current, model: "" } : current;
          }
          if (normalizedModels.includes(currentModel)) {
            return current;
          }
          return { ...current, model: normalizedModels[0] };
        });
      })
      .catch((error) => {
        if (cancelled) {
          return;
        }

        setOllamaModels([]);
        setOllamaState("error");
        setOllamaError(describeInlineError(error));
      });

    return () => {
      cancelled = true;
    };
  }, [draft.provider, draft.baseUrl, ollamaReloadKey]);

  const presets = listAiProviderPresets();
  const preset = getAiProviderPreset(draft.provider);
  const ollamaBusy = ollamaState === "loading";
  const showOllamaPicker = draft.provider === "ollama" && ollamaModels.length > 0;
  const ollamaNote =
    draft.provider !== "ollama"
      ? null
      : ollamaState === "loading"
        ? copy.ollamaLoading
        : ollamaState === "empty"
          ? copy.ollamaEmpty
          : ollamaError
            ? copy.ollamaError(ollamaError)
            : null;

  useEffect(() => {
    if (clearingSecret || sameAiSettings(normalizeAiSettings(draft), settings)) {
      if (!clearingSecret) {
        setSaveState((current) => current === "pending" ? "idle" : current);
      }
      return;
    }
    setSaveState("pending");
    const editRevision = editRevisionRef.current;
    const save = () => {
      pendingSaveRef.current = null;
      const generation = ++writeGenerationRef.current;
      if (mountedRef.current) {
        setPersistenceError(null);
        setSaveState("saving");
      }
      void onSave(normalizeAiSettings({ ...draft, enabled: true }))
        .then((persisted) => {
          if (mountedRef.current && persisted) {
            setDraft((current) => mergePersistedAiSettings(current, draft, persisted));
            if (generation === writeGenerationRef.current && editRevision === editRevisionRef.current) {
              setSaveState("saved");
            }
          } else if (mountedRef.current && generation === writeGenerationRef.current && editRevision === editRevisionRef.current) {
            setSaveState("idle");
          }
        })
        .catch((error: unknown) => {
          if (mountedRef.current && generation === writeGenerationRef.current && editRevision === editRevisionRef.current) {
            setPersistenceError({ message: describeInlineError(error), operation: "save" });
            setSaveState("idle");
          }
        });
    };
    pendingSaveRef.current = save;
    const timer = setTimeout(save, 400);
    return () => {
      clearTimeout(timer);
      if (pendingSaveRef.current === save) {
        pendingSaveRef.current = null;
      }
    };
  }, [clearingSecret, draft, onSave, saveRetryKey, settings]);

  async function handleClearSecret() {
    if (clearingSecret) {
      return;
    }
    const submitted = draft;
    const editRevision = editRevisionRef.current;
    const generation = ++writeGenerationRef.current;
    setClearingSecret(true);
    setPersistenceError(null);
    try {
      const persisted = await onClearSecret(submitted);
      if (mountedRef.current && persisted) {
        setDraft((current) => mergePersistedAiSettings(current, submitted, persisted));
        if (generation === writeGenerationRef.current && editRevision === editRevisionRef.current) {
          setSaveState("saved");
        }
      }
    } catch (error) {
      if (mountedRef.current && generation === writeGenerationRef.current) {
        setPersistenceError({ message: describeInlineError(error), operation: "clear" });
      }
    } finally {
      if (mountedRef.current) {
        setClearingSecret(false);
      }
    }
  }

  function updateSetting<Key extends keyof AiSettings>(key: Key, value: AiSettings[Key]) {
    markEdited();
    setDraft((current) => ({ ...current, [key]: value }));
  }

  function markEdited() {
    editRevisionRef.current += 1;
    setSaveState("pending");
    setPersistenceError(null);
  }

  function handleProviderChange(provider: AiProvider) {
    markEdited();
    setDraft((current) => applyAiProviderPreset(current, provider));
  }

  return (
    <section className="app-settings-card app-settings-card--ai">
      <form className="form-stack app-settings-form" onSubmit={(e) => e.preventDefault()}>
        <div className="ai-settings-grid">
          <div className="ai-settings-field">
            <div className="ai-settings-field-heading">
              <label className="detail-label" htmlFor="ai-settings-provider">{copy.providerPreset}</label>
              <details className="ai-settings-protocol-help">
                <summary tabIndex={0} aria-label={copy.protocolHelp} title={copy.protocolHelp}><ShellIcon name="alert-circle" /></summary>
                <p>{copy.protocolNote}</p>
              </details>
            </div>
            <select
              id="ai-settings-provider"
              className="text-input"
              disabled={clearingSecret}
              value={draft.provider}
              onChange={(event) => handleProviderChange(event.target.value as AiProvider)}
            >
              {presets.map((providerPreset) => (
                <option key={providerPreset.id} value={providerPreset.id}>
                  {formatAiProviderLabel(providerPreset.id, locale)}
                </option>
              ))}
            </select>
          </div>

          <div className="ai-settings-field">
            <label className="detail-label" htmlFor="ai-settings-base-url">{copy.baseUrl}</label>
            <input
              id="ai-settings-base-url"
              className="text-input"
              type="url"
              value={draft.baseUrl}
              disabled={clearingSecret}
              autoComplete="off"
              spellCheck={false}
              onChange={(event) => {
                markEdited();
                const baseUrl = event.target.value;
                setDraft((current) => applyAiServiceUrl(current, baseUrl));
              }}
              placeholder={preset.defaultBaseUrl || "https://example.com/v1"}
            />
          </div>

          <div className="ai-settings-field">
            <label className="detail-label" htmlFor="ai-settings-model">{copy.model}</label>
            {draft.provider === "ollama" ? (
              <div className="app-settings-model-control">
                {showOllamaPicker ? (
                  <select
                    id="ai-settings-model"
                    className="text-input"
                    disabled={clearingSecret || ollamaBusy}
                    aria-describedby={ollamaState === "empty" ? "ai-settings-model-note" : undefined}
                    value={draft.model}
                    onChange={(event) => updateSetting("model", event.target.value)}
                  >
                    {ollamaModels.map((model) => (
                      <option key={model} value={model}>
                        {model}
                      </option>
                    ))}
                  </select>
                ) : (
                  <input
                    id="ai-settings-model"
                    className="text-input"
                    disabled={clearingSecret}
                    aria-describedby={ollamaState === "empty" ? "ai-settings-model-note" : undefined}
                    autoComplete="off"
                    spellCheck={false}
                    type="text"
                    value={draft.model}
                    onChange={(event) => updateSetting("model", event.target.value)}
                    placeholder={copy.model}
                  />
                )}
                <button
                  type="button"
                  className="secondary-button app-settings-inline-button"
                  disabled={ollamaBusy || clearingSecret}
                  onClick={() => setOllamaReloadKey((current) => current + 1)}
                >
                  <ShellIcon name="refresh" />
                  <span>{ollamaBusy ? copy.refreshingModels : copy.refreshModels}</span>
                </button>
              </div>
            ) : (
              <input
                id="ai-settings-model"
                className="text-input"
                disabled={clearingSecret}
                autoComplete="off"
                spellCheck={false}
                type="text"
                value={draft.model}
                onChange={(event) => updateSetting("model", event.target.value)}
                placeholder={preset.defaultModel || copy.model}
              />
            )}
            {ollamaNote ? ollamaState === "error" || ollamaState === "loading"
              ? <ActivityNotice tone={ollamaState === "error" ? "error" : "info"}>{ollamaNote}</ActivityNotice> : (
              <p id="ai-settings-model-note" className="ai-settings-note" role="status">{ollamaNote}</p>
            ) : null}
          </div>

          {draft.provider !== "ollama" ? (
            <div className="ai-settings-field ai-settings-secret-field">
              <label className="detail-label" htmlFor="ai-settings-api-key">{copy.apiKey}</label>
              <div className="app-settings-model-control">
                <input
                  id="ai-settings-api-key"
                  className="text-input"
                  type="password"
                  disabled={clearingSecret}
                  aria-describedby="ai-settings-key-note"
                  value={draft.apiKey}
                  onChange={(event) => updateSetting("apiKey", event.target.value)}
                  placeholder={draft.apiKeyStored ? "••••••••••••" : copy.apiKey}
                  autoComplete="new-password"
                />
                {draft.apiKeyStored ? (
                  <button
                    type="button"
                    className="secondary-button app-settings-inline-button"
                    disabled={clearingSecret}
                    onClick={() => void handleClearSecret()}
                  >
                    <ShellIcon name="trash" />
                    <span>{clearingSecret ? copy.clearingSecret : copy.clearSecret}</span>
                  </button>
                ) : null}
              </div>
              <p id="ai-settings-key-note" className="ai-settings-note">
                <ShellIcon name="lock" />
                <span>{draft.apiKeyStored ? copy.storedKeyHint : copy.apiKeyHint}</span>
              </p>
            </div>
          ) : null}
        </div>

        {persistenceError ? (
          <ActivityNotice tone="error" action={<button
              type="button"
              className="secondary-button app-settings-inline-button"
              onClick={() => persistenceError.operation === "clear"
                ? void handleClearSecret()
                : setSaveRetryKey((current) => current + 1)}
            >
              {persistenceError.operation === "clear" ? copy.retryClear : copy.retrySave}
            </button>}>
            {`${persistenceError.operation === "clear" ? copy.clearFailed : copy.saveFailed}: ${persistenceError.message}`}
          </ActivityNotice>
        ) : (
          <ActivityNotice tone={saveState === "saved" && !clearingSecret ? "success" : "info"}>
            {clearingSecret ? copy.clearingSecret : saveState === "saving" ? copy.saving : saveState === "pending" ? copy.savePending : saveState === "saved" ? copy.saved : copy.autoSave}
          </ActivityNotice>
        )}
        <AiDataDisclosure settings={draft} />
        <AppAiConnectionCheck settings={draft} persistedSettings={settings} disabled={clearingSecret || ollamaBusy || saveState === "pending" || saveState === "saving"} />
      </form>
    </section>
  );
}
