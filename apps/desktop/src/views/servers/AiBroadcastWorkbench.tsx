import { ActivityNotice } from "../../components/ActivityNotice";
import { ConfigurationHelp } from "../settings/ConfigurationFieldHelp";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { AiSettings } from "../../ai-settings";
import type { ShellIconName } from "../../components/ShellIcon";
import { describeError } from "../../app-state";
import {
  generateInstanceBroadcast,
  listInstanceBroadcastEvents,
  readInstanceBroadcastPolicy,
  sendInstanceBroadcast,
  updateInstanceBroadcastPolicy
} from "../../api";
import { ShellIcon } from "../../components/ShellIcon";
import { useI18n, type TranslateFn } from "../../i18n";
import type {
  AssistantProviderSettingsInput,
  GenerateInstanceBroadcastOutput,
  InstanceBroadcastEvent,
  InstanceBroadcastPolicy,
  InstanceBroadcastRules,
  InstanceBroadcastSource,
  InstanceDetails,
  InstanceRuntimeOverview,
  ModuleDetails
} from "../../types";
import { formatTime } from "../../view-models";
import {
  instanceHasRunningProcess,
  runtimeProcessKeyIsRunning
} from "../../runtime-action-state";
import {
  BroadcastPolicySaveQueue,
  setupBroadcastPolicySaveQueue
} from "./broadcast-policy-save-queue";

interface AiBroadcastWorkbenchProps {
  active: boolean;
  aiSettings: AiSettings;
  assistantCanRun: boolean;
  details: InstanceDetails;
  moduleDetails: ModuleDetails | null;
  runtime: InstanceRuntimeOverview | null;
}

const DEFAULT_RULES: InstanceBroadcastRules = {
  startup: { enabled: false, prompt: null },
  shutdown: { enabled: false, prompt: null },
  runtime_health: { enabled: false, prompt: null },
  periodic: { enabled: false, interval_minutes: 30, prompt: null },
  tone: "short",
  cooldown_minutes: 10
};

function createFallbackPolicy(instanceId: string): InstanceBroadcastPolicy {
  return {
    instance_id: instanceId,
    enabled: false,
    rules: {
      ...DEFAULT_RULES,
      startup: { enabled: false, prompt: null },
      shutdown: { enabled: false, prompt: null },
      runtime_health: { enabled: false, prompt: null },
      periodic: { enabled: false, interval_minutes: 30, prompt: null }
    },
    updated_at_unix_ms: 0
  };
}

function normalizePrompt(value: string | null | undefined): string | null {
  const trimmed = String(value ?? "").trim();
  return trimmed ? trimmed.slice(0, 320) : null;
}

function normalizeRules(rules: InstanceBroadcastRules | null | undefined): InstanceBroadcastRules {
  const cooldownMinutes = Number(rules?.cooldown_minutes ?? 10);
  return {
    startup: { enabled: Boolean(rules?.startup?.enabled), prompt: normalizePrompt(rules?.startup?.prompt) },
    shutdown: { enabled: Boolean(rules?.shutdown?.enabled), prompt: normalizePrompt(rules?.shutdown?.prompt) },
    runtime_health: { enabled: Boolean(rules?.runtime_health?.enabled), prompt: normalizePrompt(rules?.runtime_health?.prompt) },
    periodic: {
      enabled: Boolean(rules?.periodic?.enabled),
      interval_minutes: Math.max(1, Math.min(1440, Number(rules?.periodic?.interval_minutes ?? 30) || 30)),
      prompt: normalizePrompt(rules?.periodic?.prompt)
    },
    tone: String(rules?.tone ?? "short").trim() || "short",
    cooldown_minutes: Math.max(0, Math.min(1440, Number.isFinite(cooldownMinutes) ? cooldownMinutes : 10))
  };
}

function normalizePolicy(policy: InstanceBroadcastPolicy, instanceId: string): InstanceBroadcastPolicy {
  return {
    instance_id: instanceId,
    enabled: Boolean(policy.enabled),
    rules: normalizeRules(policy.rules),
    updated_at_unix_ms: Number(policy.updated_at_unix_ms ?? 0) || 0
  };
}

function providerSettings(settings: AiSettings): AssistantProviderSettingsInput {
  return {
    provider: settings.provider,
    model: settings.model,
    baseUrl: settings.baseUrl,
    apiKey: settings.apiKey
  };
}

function hasBroadcastAction(moduleDetails: ModuleDetails | null): boolean {
  return Boolean((moduleDetails?.runtime.player_actions ?? []).some((action) => action.kind === "broadcast"));
}

function statusClass(status: string): string {
  const normalized = status.toLowerCase();
  if (normalized === "sent") return "is-success";
  if (normalized === "failed") return "is-danger";
  if (normalized === "blocked") return "is-warning";
  return "";
}

function statusIcon(status: string): ShellIconName {
  const normalized = status.toLowerCase();
  if (normalized === "sent") return "check-circle";
  if (normalized === "failed") return "alert-circle";
  if (normalized === "blocked") return "clock";
  return "bell";
}

const BROADCAST_STATUS_LABELS = {
  generated: ["servers.broadcast.status.generated", "Generated"],
  sent: ["servers.broadcast.status.sent", "Sent"],
  failed: ["servers.broadcast.status.failed", "Failed"],
  blocked: ["servers.broadcast.status.blocked", "Blocked"]
} as const;

const BROADCAST_SOURCE_LABELS = {
  manual: ["servers.broadcast.source.manual", "Manual"],
  startup: ["servers.broadcast.source.startup", "Startup"],
  shutdown: ["servers.broadcast.source.shutdown", "Shutdown"],
  runtime_health: ["servers.broadcast.source.runtimeHealth", "Runtime health"],
  periodic: ["servers.broadcast.source.periodic", "Periodic"]
} as const;

const BROADCAST_INITIATOR_LABELS = {
  manual: ["servers.broadcast.initiator.manual", "Manual"],
  auto: ["servers.broadcast.initiator.auto", "Automatic"],
  lifecycle: ["servers.broadcast.initiator.lifecycle", "Lifecycle"],
  system: ["servers.broadcast.initiator.system", "System"]
} as const;

type BroadcastLabelCatalog = typeof BROADCAST_STATUS_LABELS
  | typeof BROADCAST_SOURCE_LABELS
  | typeof BROADCAST_INITIATOR_LABELS;

function formatBroadcastLabel(value: string, catalog: BroadcastLabelCatalog, t: TranslateFn): string {
  const normalized = value.trim().toLowerCase();
  const label = (catalog as Record<string, readonly [string, string]>)[normalized];
  return label ? t(label[0], undefined, label[1]) : value;
}

export function AiBroadcastWorkbench(props: AiBroadcastWorkbenchProps) {
  const { locale, t } = useI18n();
  const [policy, setPolicy] = useState<InstanceBroadcastPolicy | null>(null);
  const [events, setEvents] = useState<InstanceBroadcastEvent[]>([]);
  const [intent, setIntent] = useState("");
  const [message, setMessage] = useState("");
  const [generatedMeta, setGeneratedMeta] = useState<Pick<GenerateInstanceBroadcastOutput, "provider" | "model"> | null>(null);
  const [loading, setLoading] = useState(false);
  const [savingPolicy, setSavingPolicy] = useState(false);
  const [generating, setGenerating] = useState(false);
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const instanceId = props.details.summary.id;
  const instanceScope = useMemo(() => ({ instanceId }), [instanceId]);
  const currentInstanceScopeRef = useRef(instanceScope);
  const policyRevisionRef = useRef(0);
  const policySaveQueueRef = useRef<BroadcastPolicySaveQueue | null>(null);
  currentInstanceScopeRef.current = instanceScope;
  const moduleReady = props.moduleDetails?.summary.id === props.details.summary.module_id;
  const broadcastSupported = moduleReady && hasBroadcastAction(props.moduleDetails);
  const loadedPolicy = policy?.instance_id === instanceId ? policy : null;
  const activePolicy = loadedPolicy ?? createFallbackPolicy(instanceId);
  const instanceStateReady = loadedPolicy !== null && !loading;
  const commandAction = useMemo(
    () => moduleReady
      ? (props.moduleDetails?.runtime.player_actions ?? []).find((action) => action.kind === "broadcast") ?? null : null,
    [moduleReady, props.moduleDetails]
  );
  const instanceRunning = instanceHasRunningProcess(props.details.summary, props.details.active_run);
  const running = commandAction?.process_key
    ? runtimeProcessKeyIsRunning(props.details.active_run, commandAction.process_key)
    : instanceRunning;

  useEffect(() => {
    setSavingPolicy(false);
    return setupBroadcastPolicySaveQueue(policySaveQueueRef, {
      instanceId,
      execute: (nextPolicy) => updateInstanceBroadcastPolicy({
        instance_id: instanceId,
        enabled: nextPolicy.enabled,
        rules: nextPolicy.rules
      }),
      onSaved: (savedPolicy) => {
        if (currentInstanceScopeRef.current === instanceScope) {
          setPolicy(normalizePolicy(savedPolicy, instanceId));
        }
      },
      onSavingChange: (saving) => {
        if (currentInstanceScopeRef.current === instanceScope) {
          setSavingPolicy(saving);
        }
      },
      onError: (saveError) => {
        if (currentInstanceScopeRef.current === instanceScope) {
          setError(describeError(saveError));
        }
      }
    });
  }, [instanceId, instanceScope]);

  const refreshEvents = useCallback(async () => {
    try {
      const nextEvents = await listInstanceBroadcastEvents(instanceId, 50);
      if (currentInstanceScopeRef.current === instanceScope) {
        setEvents(nextEvents);
      }
    } catch (refreshError) {
      if (currentInstanceScopeRef.current === instanceScope) {
        setError(describeError(refreshError));
      }
    }
  }, [instanceId, instanceScope]);

  useEffect(() => {
    if (!props.active || !broadcastSupported) {
      return;
    }
    let cancelled = false;
    const loadRevision = policyRevisionRef.current;
    setPolicy(null);
    setEvents([]);
    setLoading(true);
    setGenerating(false);
    setSending(false);
    setError(null);
    setIntent("");
    setMessage("");
    setGeneratedMeta(null);
    const saveQueue = policySaveQueueRef.current;
    (saveQueue?.waitUntilIdle() ?? Promise.resolve())
      .then(() => {
        if (cancelled || currentInstanceScopeRef.current !== instanceScope) {
          return null;
        }
        return Promise.all([
          readInstanceBroadcastPolicy(instanceId),
          listInstanceBroadcastEvents(instanceId, 50)
        ]);
      })
      .then((loadResult) => {
        if (!loadResult) return;
        const [nextPolicy, nextEvents] = loadResult;
        if (cancelled || currentInstanceScopeRef.current !== instanceScope) return;
        if (policyRevisionRef.current === loadRevision) {
          setPolicy(normalizePolicy(nextPolicy, instanceId));
        }
        setEvents(nextEvents);
      })
      .catch((loadError) => {
        if (
          !cancelled
          && currentInstanceScopeRef.current === instanceScope
          && policyRevisionRef.current === loadRevision
        ) {
          setError(describeError(loadError));
          const fallback = createFallbackPolicy(instanceId);
          setPolicy(fallback);
        }
      })
      .finally(() => {
        if (!cancelled && currentInstanceScopeRef.current === instanceScope) setLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, [instanceId, instanceScope, props.active, broadcastSupported]);

  function savePolicy(nextPolicy: InstanceBroadcastPolicy) {
    if (!instanceStateReady) {
      return;
    }
    const normalized = normalizePolicy(nextPolicy, instanceId);
    policyRevisionRef.current += 1;
    setPolicy(normalized);
    setError(null);
    policySaveQueueRef.current?.enqueue(normalized);
  }

  function updatePolicyRules(nextRules: InstanceBroadcastRules) {
    savePolicy({
      ...activePolicy,
      rules: normalizeRules(nextRules)
    });
  }

  async function handleGenerate() {
    const trimmedIntent = intent.trim();
    const operationScope = instanceScope;
    if (!trimmedIntent || !props.assistantCanRun || !instanceStateReady) {
      return;
    }

    setGenerating(true);
    setError(null);
    try {
      const result = await generateInstanceBroadcast({
        instanceId,
        settings: providerSettings(props.aiSettings),
        intent: trimmedIntent,
        tone: activePolicy.rules.tone,
        source: "manual",
        initiator: "manual"
      });
      if (currentInstanceScopeRef.current !== operationScope) {
        return;
      }
      setMessage(result.message);
      setGeneratedMeta({ provider: result.provider, model: result.model });
      await refreshEvents();
    } catch (generateError) {
      if (currentInstanceScopeRef.current === operationScope) {
        setError(describeError(generateError));
      }
    } finally {
      if (currentInstanceScopeRef.current === operationScope) {
        setGenerating(false);
      }
    }
  }

  async function handleSend(source: InstanceBroadcastSource = "manual", ruleId?: string | null, sendMessage = message) {
    const trimmedMessage = sendMessage.trim();
    const operationScope = instanceScope;
    if (!trimmedMessage || !broadcastSupported || !running || !instanceStateReady) {
      return;
    }

    setSending(source === "manual");
    setError(null);
    try {
      await sendInstanceBroadcast({
        instanceId,
        message: trimmedMessage,
        source,
        ruleId,
        aiProvider: generatedMeta?.provider ?? null,
        aiModel: generatedMeta?.model ?? null,
        initiator: source === "manual" ? "manual" : "auto"
      });
      if (currentInstanceScopeRef.current !== operationScope) {
        return;
      }
      if (source === "manual") {
        setMessage("");
        setGeneratedMeta(null);
      }
      await refreshEvents();
    } catch (sendError) {
      if (currentInstanceScopeRef.current === operationScope) {
        setError(describeError(sendError));
      }
    } finally {
      if (currentInstanceScopeRef.current === operationScope) {
        setSending(false);
      }
    }
  }

  const generatedAvailable = message.trim().length > 0;
  const canGenerate = instanceStateReady && props.assistantCanRun && intent.trim().length > 0 && !generating;
  const canSend = instanceStateReady && broadcastSupported && running && generatedAvailable && !sending;

  if (!props.active) return null;

  return (
    <section className={`server-workbench-surface server-maintenance-card server-broadcast-panel${broadcastSupported ? "" : " server-broadcast-panel--unavailable"}`}
      aria-label={t("servers.maintenance.broadcast")}>
      <div className="server-broadcast-heading">
        <h3><ShellIcon name="message-square" className="server-workbench-section-icon" />
          {t("servers.maintenance.broadcast")}</h3>
        {broadcastSupported && (error || loading || savingPolicy || generating || sending) ? (
          <span className={error ? "server-broadcast-state is-failed" : "server-broadcast-state"} role="status">
            {t(error ? "servers.maintenance.needsAttention" : "servers.maintenance.busy")}
          </span>
        ) : null}
      </div>
      {!broadcastSupported ? (
        <p className="server-broadcast-unavailable" role="status">
          {t(moduleReady ? "servers.broadcast.unsupported" : "servers.broadcast.capabilityUnavailable")}
        </p>
      ) : <>
        {error ? <ActivityNotice tone="error">{error}</ActivityNotice> : null}
        {!props.assistantCanRun ? <ActivityNotice tone="warning">{t("servers.broadcast.aiNotReady")}</ActivityNotice> : null}
        <div className="server-broadcast-compose">
          <div className="server-broadcast-fields">
            <label className="server-broadcast-field">
              <span>{t("servers.broadcast.intentLabel")}</span>
              <textarea name="broadcast-intent" value={intent} disabled={!instanceStateReady}
                onChange={(event) => setIntent(event.target.value)} rows={3} maxLength={320}
                placeholder={t("servers.broadcast.intentPlaceholder")} />
            </label>
            <label className="server-broadcast-field">
              <span>{t("servers.broadcast.messageLabel")}</span>
              <textarea name="broadcast-message" value={message} disabled={!instanceStateReady}
                onChange={(event) => { setMessage(event.target.value); setGeneratedMeta(null); }}
                rows={3} maxLength={240} placeholder={t("servers.broadcast.messagePlaceholder")} />
            </label>
          </div>
          <div className="server-broadcast-actions">
            <label className="server-broadcast-tone">
              <span>{t("servers.broadcast.toneLabel")}</span>
              <select name="broadcast-tone" value={activePolicy.rules.tone} disabled={!instanceStateReady}
                onChange={(event) => updatePolicyRules({ ...activePolicy.rules, tone: event.target.value })}>
                {(["short", "formal", "friendly"] as const).map((tone) => (
                  <option key={tone} value={tone}>{t(`servers.broadcast.tone.${tone}`)}</option>
                ))}
              </select>
            </label>
            <button type="button" className="secondary-button" disabled={!canGenerate}
              onClick={() => void handleGenerate()}>
              <ShellIcon name={generating ? "loader" : "zap"} className="server-broadcast-icon" />
              {t(generating ? "common.generating" : "servers.broadcast.generate")}
            </button>
            <button type="button" className="primary-button" disabled={!canSend}
              onClick={() => void handleSend()}>
              <ShellIcon name={sending ? "loader" : "send"} className="server-broadcast-icon" />
              {t(sending ? "common.sending" : "servers.broadcast.send")}
            </button>
          </div>
        </div>
        <section className="server-broadcast-rules" aria-label={t("servers.broadcast.policyTitle")}>
          <div className="server-broadcast-policy-row">
            <label className="server-broadcast-rule-toggle">
              <input name="broadcast-enabled" type="checkbox" checked={activePolicy.enabled} disabled={!instanceStateReady}
                onChange={(event) => savePolicy({ ...activePolicy, enabled: event.target.checked })} />
              <span>{t("servers.broadcast.policyTitle")}</span>
            </label>
            <label className="server-broadcast-cooldown">
              <span>{t("servers.broadcast.cooldownMinutes")}</span>
              <input name="broadcast-cooldown" type="number" min={0} max={1440}
                value={activePolicy.rules.cooldown_minutes} disabled={!instanceStateReady || !activePolicy.enabled}
                onChange={(event) => updatePolicyRules({ ...activePolicy.rules,
                  cooldown_minutes: Number(event.target.value) || 0 })} />
            </label>
          </div>
          {(["startup", "shutdown"] as const).map((key) => {
            const rule = activePolicy.rules[key];
            const label = t(`servers.broadcast.rule.${key}`);
            return <div className="server-broadcast-rule" key={key}>
              <label className="server-broadcast-rule-toggle">
                <input name={`broadcast-${key}-enabled`} type="checkbox" checked={rule.enabled}
                  disabled={!instanceStateReady || !activePolicy.enabled}
                  onChange={(event) => updatePolicyRules({ ...activePolicy.rules,
                    [key]: { ...rule, enabled: event.target.checked } })} />
                <span>{label}</span>
              </label>
              <textarea name={`broadcast-${key}-prompt`} rows={1} maxLength={320} value={rule.prompt ?? ""}
                aria-label={`${label} ${t("servers.broadcast.promptTitle")}`}
                disabled={!instanceStateReady || !activePolicy.enabled}
                placeholder={t("servers.broadcast.promptPlaceholder")}
                onChange={(event) => updatePolicyRules({ ...activePolicy.rules,
                  [key]: { ...rule, prompt: normalizePrompt(event.target.value) } })} />
            </div>;
          })}
        </section>
        <section className="server-broadcast-history" aria-label={t("servers.broadcast.historyTitle")}>
          <div className="server-broadcast-history-heading">
            <h4>{t("servers.broadcast.historyTitle")}{events.length > 0 ? <span>{events.length}</span> : null}</h4>
            <ConfigurationHelp description={t("common.refresh")}>{(help) =>
            <button type="button" className="ghost-button" disabled={!instanceStateReady}
              ref={help.anchorRef} {...help.interactionProps} aria-describedby={help.descriptionId}
              onClick={() => void refreshEvents()} aria-label={t("common.refresh")}>
              <ShellIcon name="refresh" className="server-broadcast-icon" />
            </button>}</ConfigurationHelp>
          </div>
          <div className="server-broadcast-events">
            {events.length === 0 ? <p className="server-broadcast-empty">
              {t(loading ? "common.loading" : "servers.broadcast.historyEmpty")}
            </p> : events.map((event) => (
              <article key={event.event_id} className="server-broadcast-event">
                <div className="server-broadcast-event-heading">
                  <span className={`server-broadcast-event-status ${statusClass(event.status)}`}>
                    <ShellIcon name={statusIcon(event.status)} className="server-broadcast-icon" />
                    {formatBroadcastLabel(event.status, BROADCAST_STATUS_LABELS, t)}
                  </span>
                  <span>{formatBroadcastLabel(event.source, BROADCAST_SOURCE_LABELS, t)}</span>
                  <time>{formatTime(locale, event.created_at_unix_ms)}</time>
                </div>
                <p className="server-broadcast-event-message">{event.message}</p>
                {[event.initiator ? formatBroadcastLabel(event.initiator, BROADCAST_INITIATOR_LABELS, t) : null,
                  event.ai_model].filter(Boolean).length > 0 ? <p className="server-broadcast-event-meta">
                  {[event.initiator ? formatBroadcastLabel(event.initiator, BROADCAST_INITIATOR_LABELS, t) : null,
                    event.ai_model].filter(Boolean).join(" · ")}
                </p> : null}
                {event.error_message ? <p className="server-broadcast-error" role="alert">{event.error_message}</p> : null}
              </article>
            ))}
          </div>
        </section>
      </>}
    </section>
  );
}
