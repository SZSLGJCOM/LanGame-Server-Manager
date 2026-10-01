import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { describeError } from "../../app-state";
import { readInstanceLogDocument } from "../../api";
import { createRuntimeLogRefresh } from "../../runtime-log-refresh";
import { formatInstancePanelError, type InstancePanelLoadState } from "../../instance-panel-loader";
import { selectLocaleText, useI18n, type TranslateFn } from "../../i18n";
import {
  appendRuntimeLogStreamEvent,
  collectRuntimeLogPaths,
  normalizeRuntimeLogPath,
  retainRuntimeLogHistory,
  retainNewestRuntimeGameDocument,
  RUNTIME_LOG_STREAM_EVENT,
  RUNTIME_SERVICE_EVENTS_RESET,
  type RuntimeLogStreamEvent,
  type RuntimeLogStartupBoundary
} from "../../runtime-log-stream";
import { ShellIcon } from "../../components/ShellIcon";
import { localizeRuntimeHealthSummary } from "../../runtime-health-message";
import { isArkModule } from "../../ark-clusters";
import { resolveRuntimeConsoleTransport } from "../../runtime-console-transport";
import type { InstanceArchiveDetails } from "../../storage-management-types";
import type {
  InstanceDetails,
  InstanceProcessRecord,
  InstanceRuntimeOverview,
  InstanceRuntimeCommandResult,
  LogTailSnapshot,
  ModuleDetails,
  RuntimeCommandDispatchOptions,
  RuntimeLogSource,
  RuntimeWindowSnapshot
} from "../../types";
import {
  instanceHasRunningProcess,
  runtimeProcessIsRunning
} from "../../runtime-action-state";

interface RuntimeSurfaceWorkbenchProps {
  details: InstanceDetails;
  moduleDetails?: ModuleDetails | null;
  runtime: InstanceRuntimeOverview | null;
  runtimeWindows: RuntimeWindowSnapshot | null;
  panelLoadState?: InstancePanelLoadState | null;
  onRetryReads?: () => void;
  startupPending: boolean;
  startupBoundary?: RuntimeLogStartupBoundary;
  launchHostSurface: string;
  archive?: Pick<InstanceArchiveDetails, "runs" | "log">;
  onSendRuntimeCommand?: (
    instanceId: string,
    command: string,
    processKey?: string | null,
    options?: RuntimeCommandDispatchOptions
  ) => Promise<InstanceRuntimeCommandResult | null>;
  onSuppressRuntimeWindows?: (instanceId: string) => Promise<void>;
}

interface RuntimeCommandTargetOption {
  value: string;
  processKey: string | null;
  label: string;
  disabled: boolean;
}

interface RuntimeConsoleTab {
  value: string;
  processKey: string | null;
  label: string;
  meta: string;
  runId: number | null;
  logPath: string;
  isPrimary: boolean;
}

const PRIMARY_RUNTIME_TARGET = "__primary__";

type RuntimeConsoleCopyState = "idle" | "copied" | "failed";

function pickRuntimeProcessRecords(
  details: InstanceDetails,
  runtime: InstanceRuntimeOverview | null
): InstanceProcessRecord[] {
  const activeProcesses = details.active_run?.processes ?? [];
  if (activeProcesses.length > 0) {
    return activeProcesses;
  }

  for (const run of runtime?.recent_runs ?? []) {
    if ((run.processes?.length ?? 0) > 0) {
      return run.processes ?? [];
    }
  }

  return [];
}

function buildRuntimeCommandTargets(
  processes: InstanceProcessRecord[],
  instanceRunning: boolean,
  t: TranslateFn,
  ark: boolean
): RuntimeCommandTargetOption[] {
  const primaryRoleLabel = t("servers.runtimeSurface.processRolePrimary", undefined, "Primary");
  const targets: RuntimeCommandTargetOption[] = ark && processes.some((process) => normalizeProcessKey(process)) ? [] : [
    {
      value: PRIMARY_RUNTIME_TARGET,
      processKey: null,
      label: t("servers.runtimeSurface.primaryProcess", undefined, "LanGameCMD"),
      disabled: !instanceRunning
    }
  ];
  const seen = new Set<string>();

  for (const process of processes) {
    const processKey = String(process.process_key ?? "").trim();
    if (!processKey || seen.has(processKey)) {
      continue;
    }

    seen.add(processKey);

    targets.push({
      value: processKey,
      processKey,
      label: ark ? formatRuntimeProcessLabel(process, t, true)
        : process.is_primary ? `${processKey} / ${primaryRoleLabel}` : processKey,
      disabled: !runtimeProcessIsRunning(process)
    });
  }

  return targets;
}

function normalizeProcessKey(process: InstanceProcessRecord): string {
  return String(process.process_key ?? "").trim();
}

function formatRuntimeProcessLabel(process: InstanceProcessRecord, t: TranslateFn, ark: boolean): string {
  const processKey = normalizeProcessKey(process);
  const displayName = String(process.display_name ?? "").trim();
  const key = processKey.toLowerCase();

  if (ark && displayName) return displayName;

  if (key === "master") {
    return t("servers.runtimeSurface.processLabel.master", undefined, "Master / Surface");
  }

  if (key === "caves") {
    return t("servers.runtimeSurface.processLabel.caves", undefined, "Caves / Underground");
  }

  if (displayName && displayName.toLowerCase() !== key) {
    return `${processKey || displayName} / ${displayName}`;
  }

  return processKey || displayName || t("servers.runtimeSurface.primaryProcess", undefined, "LanGameCMD");
}

function buildRuntimeConsoleTabs(
  processes: InstanceProcessRecord[],
  fallbackRunId: number | null,
  fallbackLogPath: string,
  t: TranslateFn,
  ark: boolean
): RuntimeConsoleTab[] {
  const seen = new Set<string>();
  const tabs: RuntimeConsoleTab[] = [];

  for (const process of processes) {
    const processKey = normalizeProcessKey(process);
    const runId = typeof process.run_id === "number" ? process.run_id : null;
    const value = processKey || (runId ? `run-${runId}` : PRIMARY_RUNTIME_TARGET);
    if (seen.has(value)) {
      continue;
    }

    seen.add(value);
    const pidLabel = process.pid ? `PID ${process.pid}` : t("servers.runtimeSurface.pidPending", undefined, "PID pending");
    const status = String(process.status ?? "").trim();
    const logPath = String(process.log_path ?? "").trim();

    tabs.push({
      value,
      processKey: processKey || null,
      label: formatRuntimeProcessLabel(process, t, ark),
      meta: [pidLabel, status].filter(Boolean).join(" / "),
      runId,
      logPath,
      isPrimary: Boolean(process.is_primary)
    });
  }

  if (tabs.length === 0) {
    tabs.push({
      value: PRIMARY_RUNTIME_TARGET,
      processKey: null,
      label: t("servers.runtimeSurface.primaryProcess", undefined, "LanGameCMD"),
      meta: t("servers.runtimeSurface.defaultRuntimeLog", undefined, "Default runtime log"),
      runId: fallbackRunId,
      logPath: fallbackLogPath,
      isPrimary: true
    });
  }

  if (!tabs.some((tab) => tab.isPrimary)) {
    tabs[0] = { ...tabs[0], isPrimary: true };
  }

  return tabs;
}

export function RuntimeSurfaceWorkbench(props: RuntimeSurfaceWorkbenchProps) {
  const { t, locale } = useI18n();
  const ark = isArkModule(props.details.summary.module_id);
  const readOnly = Boolean(props.archive);
  const startupPending = !readOnly && props.startupPending;
  const archivedLogSnapshot = useMemo<LogTailSnapshot | null>(() => props.archive ? {
    source_path: props.archive.log.relative_path,
    lines: props.archive.log.text ? props.archive.log.text.split(/\r?\n/) : [],
    truncated: props.archive.log.truncated,
    read_error: props.archive.log.issues.length ? props.archive.log.issues.join("\n") : null
  } : null, [props.archive]);
  const [runtimeCommandDraft, setRuntimeCommandDraft] = useState("");
  const [runtimeTargetValue, setRuntimeTargetValue] = useState(PRIMARY_RUNTIME_TARGET);
  const [runtimeConsoleTabValue, setRuntimeConsoleTabValue] = useState(PRIMARY_RUNTIME_TARGET);
  const [processLogSnapshot, setProcessLogSnapshot] = useState<LogTailSnapshot | null>(null);
  const [processLogReadScope, setProcessLogReadScope] = useState("");
  const [arkLogSelection, setArkLogSelection] = useState<{ scope: string; source: RuntimeLogSource } | null>(null);
  const [liveLogSnapshot, setLiveLogSnapshot] = useState<LogTailSnapshot | null>(null);
  const [processLogLoading, setProcessLogLoading] = useState(false);
  const [processLogError, setProcessLogError] = useState<{ scope: string; message: string } | null>(null);
  const [processLogRetryGeneration, setProcessLogRetryGeneration] = useState(0);
  const [streamError, setStreamError] = useState<string | null>(null);
  const [commandFeedback, setCommandFeedback] = useState<string | null>(null);
  const [commandPending, setCommandPending] = useState(false);
  const logRefreshRef = useRef<ReturnType<typeof createRuntimeLogRefresh<LogTailSnapshot>> | null>(null);
  const commandScope = `${props.details.summary.id}:${props.details.active_run?.run_id ?? ""}:${runtimeTargetValue}`;
  const commandScopeRef = useRef(commandScope);
  commandScopeRef.current = commandScope;
  const [suppressingWindows, setSuppressingWindows] = useState(false);
  const [consoleCopyState, setConsoleCopyState] = useState<RuntimeConsoleCopyState>("idle");
  const observedLogPathsRef = useRef({ instanceId: props.details.summary.id, paths: new Set<string>() });
  const startupBoundaryRef = useRef<{
    source: RuntimeLogStartupBoundary | undefined;
    value: RuntimeLogStartupBoundary;
  } | null>(null);
  const runtimeConsoleRef = useRef<HTMLPreElement>(null);
  const consoleFollowRef = useRef<{
    scope: string | null; following: boolean; feedback: string | null; scrollTop: number;
  }>({
    scope: null, following: true, feedback: null, scrollTop: 0
  });
  const revealedReadErrorsRef = useRef(new Set<string>());

  useEffect(() => {
    if (observedLogPathsRef.current.instanceId !== props.details.summary.id) {
      observedLogPathsRef.current = { instanceId: props.details.summary.id, paths: new Set() };
      startupBoundaryRef.current = null;
    }
    setRuntimeCommandDraft("");
    setRuntimeTargetValue(PRIMARY_RUNTIME_TARGET);
    setRuntimeConsoleTabValue(PRIMARY_RUNTIME_TARGET);
    setProcessLogSnapshot(null);
    setLiveLogSnapshot(null);
    setProcessLogLoading(false);
    setProcessLogError(null);
    setSuppressingWindows(false);
    setConsoleCopyState("idle");
    setCommandFeedback(null);
    setCommandPending(false);
  }, [props.details.summary.id, readOnly]);

  useEffect(() => {
    setCommandFeedback(null);
    setCommandPending(false);
  }, [commandScope]);

  const runtimeProcesses = readOnly || startupPending ? [] : pickRuntimeProcessRecords(props.details, props.runtime);
  const instanceRunning = !readOnly && instanceHasRunningProcess(props.details.summary, props.details.active_run);
  const fallbackRunId = readOnly ? null : props.details.active_run?.run_id ?? props.runtime?.recent_runs[0]?.run_id ?? null;
  const fallbackLogPath = String(readOnly ? archivedLogSnapshot?.source_path ?? "" : (
    props.runtime?.log_tail.source_path
      ?? props.details.active_run?.log_path
      ?? props.runtime?.recent_runs[0]?.log_path
      ?? ""
  )).trim();
  const runtimeConsoleTabs = useMemo(
    () => buildRuntimeConsoleTabs(runtimeProcesses, fallbackRunId, fallbackLogPath, t, ark),
    [ark, fallbackLogPath, fallbackRunId, runtimeProcesses, t]
  );
  const runtimeConsoleTabKeys = runtimeConsoleTabs.map((tab) => tab.value).join("|");
  const selectedConsoleTab =
    runtimeConsoleTabs.find((tab) => tab.value === runtimeConsoleTabValue)
    ?? runtimeConsoleTabs.find((tab) => tab.isPrimary)
    ?? runtimeConsoleTabs[0]
    ?? null;
  const runtimeTargets = useMemo(
    () => buildRuntimeCommandTargets(runtimeProcesses, instanceRunning, t, ark),
    [ark, instanceRunning, runtimeProcesses, t]
  );
  const selectedTarget =
    runtimeTargets.find((target) => target.value === runtimeTargetValue) ?? runtimeTargets[0] ?? null;

  useEffect(() => {
    if (!selectedTarget?.disabled) {
      return;
    }
    const fallbackTarget = runtimeTargets.find((target) => !target.disabled);
    if (fallbackTarget) {
      setRuntimeTargetValue(fallbackTarget.value);
    }
  }, [runtimeTargets, selectedTarget]);

  useEffect(() => {
    if (!selectedConsoleTab) {
      return;
    }

    if (!runtimeConsoleTabs.some((tab) => tab.value === runtimeConsoleTabValue)) {
      setRuntimeConsoleTabValue(selectedConsoleTab.value);
      setRuntimeTargetValue(selectedConsoleTab.processKey ?? PRIMARY_RUNTIME_TARGET);
    }
  }, [runtimeConsoleTabKeys, runtimeConsoleTabValue, runtimeConsoleTabs, selectedConsoleTab]);

  const selectedConsoleRunId = selectedConsoleTab?.runId ?? null;
  const arkLogScope = JSON.stringify([props.details.summary.id, selectedConsoleRunId]);
  const arkLogSource: RuntimeLogSource = arkLogSelection?.scope === arkLogScope ? arkLogSelection.source : "game";
  const selectedGameLog = ark && !readOnly && !startupPending && selectedConsoleRunId !== null && arkLogSource === "game";
  const selectedLogReadScope = JSON.stringify([props.details.summary.id, selectedConsoleRunId, ark ? arkLogSource : null]);
  useEffect(() => {
    if (!ark || startupPending) return;
    setLiveLogSnapshot(null);
  }, [selectedLogReadScope, ark, startupPending]);
  const selectedProcessLog = processLogReadScope === selectedLogReadScope ? processLogSnapshot : null;
  const selectedProcessLogError = processLogError?.scope === selectedLogReadScope ? processLogError.message : null;
  const selectedConsoleIsPrimary = Boolean(selectedConsoleTab?.isPrimary);
  const selectedConsoleUsesDefaultLog = selectedConsoleIsPrimary && !ark;
  const selectedConsoleLogPath = selectedGameLog ? selectedProcessLog?.source_path ?? ""
    : selectedConsoleUsesDefaultLog ? fallbackLogPath : selectedConsoleTab?.logPath ?? "";

  useEffect(() => {
    if (readOnly || startupPending || !selectedConsoleTab) {
      setProcessLogSnapshot(null);
      setProcessLogLoading(false);
      setProcessLogError(null);
      return;
    }

    // Map source selection is resolved by the backend from the owned run;
    // never infer another map's filename or reuse its previous snapshot.
    const refresh = createRuntimeLogRefresh({
      read: () => readInstanceLogDocument(props.details.summary.id, 400,
        selectedConsoleUsesDefaultLog ? undefined : selectedConsoleRunId ?? undefined,
        ark && selectedConsoleRunId !== null ? arkLogSource : undefined),
      publish: (snapshot) => {
          setProcessLogSnapshot(snapshot);
          setProcessLogReadScope(selectedLogReadScope);
          setLiveLogSnapshot((current) => selectedGameLog && snapshot.snapshot_revision !== undefined
            ? retainNewestRuntimeGameDocument(current, snapshot, 400)
            : retainRuntimeLogHistory(null, snapshot, 400));
          setProcessLogError(null);
      },
      failed: (error) => {
          setProcessLogSnapshot(null);
          setProcessLogError({ scope: selectedLogReadScope, message: describeError(error) });
      },
      loading: setProcessLogLoading
    });
    logRefreshRef.current = refresh;
    refresh.request();
    return () => {
      refresh.dispose();
      if (logRefreshRef.current === refresh) logRefreshRef.current = null;
    };
  }, [props.details.summary.id, readOnly, startupPending, selectedConsoleUsesDefaultLog, selectedConsoleRunId,
    selectedConsoleTab?.value, selectedConsoleTab?.logPath, processLogRetryGeneration, selectedLogReadScope, ark, arkLogSource, selectedGameLog,
    selectedConsoleUsesDefaultLog ? fallbackLogPath : null]);

  useEffect(() => {
    // Polls and event invalidations share the same reader, so a late poll cannot
    // overwrite a newer independently completed file read.
    logRefreshRef.current?.request();
  }, [props.runtime?.log_tail]);

  const commandRoute = useMemo(() => resolveRuntimeConsoleTransport(
    props.moduleDetails ?? null, props.details.settings_json
  ), [props.moduleDetails, props.details.settings_json]);
  const commandHint = commandRoute.hint
    ? selectLocaleText(locale, commandRoute.hint.zhCN, commandRoute.hint.en) : null;
  const runtimeCommandEnabled = instanceRunning && !selectedTarget?.disabled && commandRoute.available && Boolean(props.onSendRuntimeCommand);
  const polledLogSnapshot = selectedConsoleUsesDefaultLog
    ? props.runtime?.log_tail ?? null
    : selectedProcessLog;
  const selectedSourcePath = normalizeRuntimeLogPath(selectedConsoleLogPath);
  // A newly selected map can be silent or fail its first read. Never label the
  // previous map's retained output as belonging to that selected command target.
  const selectedLiveLog = (!selectedGameLog || selectedSourcePath) && (!selectedSourcePath || normalizeRuntimeLogPath(liveLogSnapshot?.source_path) === selectedSourcePath)
    ? liveLogSnapshot : null;
  const selectedPolledLog = !selectedSourcePath || normalizeRuntimeLogPath(polledLogSnapshot?.source_path) === selectedSourcePath
    ? polledLogSnapshot : null;
  const startupReadbackBoundary = props.startupBoundary?.instanceId === props.details.summary.id
    ? props.startupBoundary : startupBoundaryRef.current?.value;
  const startupReadback = startupReadbackBoundary?.instanceId === props.details.summary.id
    && selectedPolledLog?.source_path
    && !startupReadbackBoundary.previousLogPaths.has(normalizeRuntimeLogPath(selectedPolledLog.source_path))
    ? selectedPolledLog : null;
  const selectedLogSnapshot = readOnly ? archivedLogSnapshot : startupPending
    // A late mount may already have the new primary's snapshot. Display it only
    // until shard events arrive; never seed their unpositioned aggregate with it.
    ? liveLogSnapshot ?? startupReadback
    : selectedLiveLog ?? selectedPolledLog;
  const runtimeLogLines = selectedLogSnapshot?.lines ?? [];
  const panelLoadState = !readOnly && props.panelLoadState?.instanceId === props.details.summary.id ? props.panelLoadState : null;
  const runtimeReadError = selectedLogSnapshot?.read_error || (!readOnly ? selectedProcessLogError : null);
  const runtimeReadIdentities = new Set(runtimeReadError ? [runtimeReadError] : []);
  const runtimeReadMessages = runtimeReadError ? [t(
    "servers.details.runtimeLogReadError",
    { message: runtimeReadError },
    `Failed to read the runtime log: ${runtimeReadError}`
  )] : [];
  if (streamError && !runtimeReadIdentities.has(streamError)) {
    runtimeReadIdentities.add(streamError);
    runtimeReadMessages.push(streamError);
  }
  const runtimeReadParts = ["runtime", "runtimeWindows", "logDocument"] as const;
  for (const part of runtimeReadParts) {
    const error = panelLoadState?.errors[part];
    if (error && !runtimeReadIdentities.has(error)) {
      runtimeReadIdentities.add(error);
      runtimeReadMessages.push(
        `${t("servers.loading.failed", { part: t(`servers.loading.part.${part}`) })} ${formatInstancePanelError(error, t)}`
      );
    }
  }
  const runtimeErrorText = runtimeReadMessages.map(message => `[LanGameCMD] ${message}`).join("\n");
  const retryReadsPending = (!readOnly && processLogLoading) || runtimeReadParts.some(part => panelLoadState?.pending.includes(part));
  const runtimeLogText = runtimeLogLines.length > 0
    ? runtimeLogLines.join("\n")
    : readOnly
      ? t("servers.archives.workspace.noLog", undefined, "No readable log was retained.")
    : startupPending
      ? t(
          "servers.runtimeSurface.startupAttached",
          undefined,
          "LanGameCMD attached. Preparing the startup session..."
        )
    : processLogLoading
      ? t("servers.runtimeSurface.readingShardLog", undefined, "Reading shard log...")
      : !props.runtime && panelLoadState?.pending.includes("runtime")
        ? t("servers.loading.pending", { part: t("servers.loading.part.runtime") })
        : t("servers.details.runtimeLogFallback");
  const runtimeLog = [runtimeLogText, commandFeedback ? `[LanGameCMD] ${commandFeedback}` : null, runtimeErrorText]
    .filter(Boolean).join("\n\n");
  const runtimeLogSourcePath = String(
    (startupPending ? selectedLogSnapshot?.source_path : selectedConsoleLogPath)
      ?? fallbackLogPath
      ?? ""
  ).trim();
  const runtimeLogSubscriptionPath = startupPending ? "" : runtimeLogSourcePath;
  const consoleScrollScope = JSON.stringify([props.details.summary.id, readOnly, selectedConsoleTab?.value,
    selectedConsoleRunId, ark ? arkLogSource : null, normalizeRuntimeLogPath(runtimeLogSourcePath), startupPending,
    startupPending ? props.startupBoundary?.startedAtUnixMs : null]);
  const runtimeWindowCount = readOnly ? 0 : props.runtimeWindows?.windows.length ?? 0;
  const runtimeWindowSuppressionSupported = props.launchHostSurface !== "external_window";
  const canSuppressRuntimeWindows =
    runtimeWindowSuppressionSupported && instanceRunning && runtimeWindowCount > 0 && !suppressingWindows && Boolean(props.onSuppressRuntimeWindows);
  const runtimeConsolePrompt = selectedTarget?.processKey?.trim() || "server";
  const canCopyRuntimeLog = runtimeLog.trim().length > 0;
  const copyConsoleTitle = consoleCopyState === "copied"
    ? t("servers.runtimeSurface.copyConsoleCopied", undefined, "Copied")
    : consoleCopyState === "failed"
      ? t("servers.runtimeSurface.copyConsoleFailed", undefined, "Copy failed")
      : t("servers.runtimeSurface.copyConsole", undefined, "Copy console");

  useEffect(() => {
    if (!startupPending) {
      startupBoundaryRef.current = null;
      return;
    }
    const source = props.startupBoundary?.instanceId === props.details.summary.id ? props.startupBoundary : undefined;
    if (startupBoundaryRef.current?.source === source && startupBoundaryRef.current?.value.instanceId === props.details.summary.id) return;
    // Freeze only the previous session's paths. Polls may already contain the
    // new run on mount, and later shards must stay eligible for this startup.
    startupBoundaryRef.current = { source, value: {
      instanceId: props.details.summary.id,
      startedAtUnixMs: source?.startedAtUnixMs ?? 0,
      previousLogPaths: new Set([
        ...(source?.previousLogPaths ?? collectRuntimeLogPaths(props.details, props.runtime)),
        ...observedLogPathsRef.current.paths
      ])
    } };
    observedLogPathsRef.current.paths.clear();
    setLiveLogSnapshot(null);
  }, [props.details.summary.id, startupPending, props.startupBoundary]);

  useEffect(() => {
    if (readOnly) return;
    const polledPath = normalizeRuntimeLogPath(polledLogSnapshot?.source_path);
    if (startupPending) {
      if (!polledPath || startupBoundaryRef.current?.value.previousLogPaths.has(polledPath)) {
        return;
      }
    }
    if (polledPath) observedLogPathsRef.current.paths.add(polledPath);
    // Startup combines all shard streams; a primary-only poll must not replace it.
    setLiveLogSnapshot((current) => startupPending ? current
      : selectedGameLog && polledLogSnapshot?.snapshot_revision !== undefined
        ? retainNewestRuntimeGameDocument(current, polledLogSnapshot, 400)
      : retainRuntimeLogHistory(current, polledLogSnapshot, 400));
  }, [polledLogSnapshot, props.details.summary.id, readOnly, startupPending, props.startupBoundary, selectedConsoleTab?.value, selectedGameLog]);

  useEffect(() => {
    if (readOnly || !isTauri()) {
      return;
    }

    let disposed = false;
    setStreamError(null);
    const unlisten = listen<RuntimeLogStreamEvent>(RUNTIME_LOG_STREAM_EVENT, (event) => {
      if (disposed) {
        return;
      }
      if (event.payload.instance_id !== props.details.summary.id) return;
      // Observe unselected shards as well so the next Start can exclude their
      // delayed final deliveries without rendering them in this selected tab.
      observedLogPathsRef.current.paths.add(normalizeRuntimeLogPath(event.payload.log_path));
      // A complete game document can arrive before its read request resolves.
      // Buffer it by the known run/map identity; the response will establish the
      // path without overwriting a newer event. Console deltas cannot enter here.
      if (selectedGameLog && (!event.payload.snapshot || event.payload.run_id !== selectedConsoleRunId
        || event.payload.process_key !== selectedConsoleTab?.processKey)) return;
      if (!startupPending && runtimeLogSubscriptionPath &&
        normalizeRuntimeLogPath(event.payload.log_path) !== normalizeRuntimeLogPath(runtimeLogSubscriptionPath)) return;
      if (!startupPending && event.payload.run_id != null && selectedConsoleRunId != null
        && event.payload.run_id !== selectedConsoleRunId) return;
      const boundary = startupPending ? startupBoundaryRef.current?.value : null;
      if (boundary?.instanceId === props.details.summary.id && (
        boundary.previousLogPaths.has(normalizeRuntimeLogPath(event.payload.log_path))
        || event.payload.emitted_at_unix_ms < boundary.startedAtUnixMs
      )) return;
      if (typeof event.payload.stream_error === "string") {
        setStreamError(event.payload.stream_error || "Console stream reported an unspecified error.");
      }
      if (!startupPending && !event.payload.snapshot) {
        // The retained file may already contain this delayed batch. There is no
        // snapshot byte range/event identity with which to prove an append safe.
        logRefreshRef.current?.request();
        return;
      }

      setLiveLogSnapshot((current) => appendRuntimeLogStreamEvent(
        current ?? {
          source_path: runtimeLogSubscriptionPath,
          lines: [],
          total_lines: 0,
          truncated: false,
          read_error: null
        },
        event.payload,
        {
          instanceId: props.details.summary.id,
          logPath: runtimeLogSubscriptionPath || null,
          followLatestPath: startupPending,
          mergeProcessLogs: startupPending,
          startupBoundary: startupPending ? startupBoundaryRef.current?.value : null,
          maxLines: 400
        }
      ));
    }).catch((error: unknown) => {
      if (!disposed) setStreamError(describeError(error));
      console.warn("Runtime log subscription failed; periodic log refresh remains available.", error);
      return null;
    });

    return () => {
      disposed = true;
      void unlisten.then((dispose) => dispose?.()).catch((error: unknown) => {
        console.warn("Runtime log subscription cleanup failed.", error);
      });
    };
  }, [props.details.summary.id, readOnly, startupPending, runtimeLogSubscriptionPath, props.startupBoundary, processLogRetryGeneration,
    selectedGameLog, selectedConsoleRunId, selectedConsoleTab?.processKey]);

  useEffect(() => {
    if (readOnly || !isTauri()) return;
    let disposed = false;
    const unlisten = listen(RUNTIME_SERVICE_EVENTS_RESET, () => {
      if (disposed) return;
      // A restarted service or an event retention gap requires an authoritative
      // file read. Reuse the panel refresh and selected shard retry paths.
      setLiveLogSnapshot(null);
      setProcessLogRetryGeneration((generation) => generation + 1);
      props.onRetryReads?.();
    }).catch((error: unknown) => {
      console.warn("Runtime event recovery subscription failed; periodic log refresh remains available.", error);
      return null;
    });
    return () => {
      disposed = true;
      void unlisten.then((dispose) => dispose?.()).catch((error: unknown) => {
        console.warn("Runtime event recovery subscription cleanup failed.", error);
      });
    };
  }, [props.onRetryReads, readOnly]);

  useLayoutEffect(() => {
    const follow = consoleFollowRef.current;
    const terminal = runtimeConsoleRef.current;
    if (follow.scope !== consoleScrollScope || (commandFeedback && commandFeedback !== follow.feedback)) {
      follow.following = true;
    } else if (terminal && terminal.scrollTop < follow.scrollTop
      && terminal.scrollHeight - terminal.clientHeight - terminal.scrollTop > 48) {
      // A log update may render before the browser dispatches a user's scroll
      // event. Respect an already changed position instead of snapping it back.
      follow.following = false;
    }
    follow.scope = consoleScrollScope;
    follow.feedback = commandFeedback;
    // Retained history is capped at 400 lines: appended output can replace older
    // lines without changing scrollHeight, so follow content changes as well.
    if (terminal) {
      if (follow.following) terminal.scrollTop = terminal.scrollHeight;
      follow.scrollTop = terminal.scrollTop;
    }
  }, [consoleScrollScope, runtimeLogText, commandFeedback]);

  useEffect(() => {
    revealedReadErrorsRef.current.clear();
  }, [props.details.summary.id, selectedConsoleTab?.value, selectedConsoleRunId]);

  useEffect(() => {
    // A refresh temporarily clears its errors while reads are pending. Keep the
    // revealed messages through that interval so a repeated failure cannot steal
    // the user's scroll position. A successful read allows a later failure anew.
    if ([...runtimeReadIdentities].some(error => !revealedReadErrorsRef.current.has(error)) && runtimeConsoleRef.current) {
      consoleFollowRef.current.following = true;
      runtimeConsoleRef.current.scrollTop = runtimeConsoleRef.current.scrollHeight;
      consoleFollowRef.current.scrollTop = runtimeConsoleRef.current.scrollTop;
    }
    if (!retryReadsPending) revealedReadErrorsRef.current = runtimeReadIdentities;
    else for (const error of runtimeReadIdentities) revealedReadErrorsRef.current.add(error);
  }, [runtimeErrorText, retryReadsPending, props.details.summary.id, selectedConsoleTab?.value, selectedConsoleRunId]);

  useEffect(() => {
    if (consoleCopyState === "idle") {
      return;
    }

    const timeoutId = window.setTimeout(() => setConsoleCopyState("idle"), 1400);
    return () => window.clearTimeout(timeoutId);
  }, [consoleCopyState]);

  async function handleRuntimeCommandSubmit() {
    const command = runtimeCommandDraft.trim();
    if (readOnly || !command || commandPending || !selectedTarget || !runtimeCommandEnabled || !props.onSendRuntimeCommand) {
      return;
    }

    const instanceId = props.details.summary.id;
    const submittedScope = commandScope;
    setCommandPending(true);
    setCommandFeedback(null);
    try {
      const result = await props.onSendRuntimeCommand(instanceId, command, selectedTarget.processKey ?? null,
        commandRoute.options);
      if (commandScopeRef.current !== submittedScope) return;
      if (result) {
        setRuntimeCommandDraft((current) => current === runtimeCommandDraft ? "" : current);
        setCommandFeedback(result.write_confirmation_pending
          ? selectLocaleText(locale, "等待写入确认；尚未确认服务器接收或执行。", "Awaiting write confirmation; server receipt and execution are not confirmed.")
          : result.response_text?.trim()
            ? `${selectLocaleText(locale, "服务器响应", "Server response")}\n${result.response_text.slice(0, 16384)}${result.response_text.length > 16384
              ? selectLocaleText(locale, "\n[响应显示达到 16,384 个字符上限]", "\n[Response display reached its 16,384-character limit]") : ""}`
            : selectLocaleText(locale, "已发送，请查看控制台后续输出确认执行结果。", "Sent. Check subsequent console output to confirm the result."));
      } else {
        setCommandFeedback(selectLocaleText(locale, "命令未确认发送，请检查动态消息和服务器日志。", "Command delivery was not confirmed. Check activity and server output."));
      }
    } catch (error) {
      if (commandScopeRef.current === submittedScope) setCommandFeedback(describeError(error));
    } finally {
      if (commandScopeRef.current === submittedScope) setCommandPending(false);
    }
  }

  async function handleCopyRuntimeLog() {
    if (!canCopyRuntimeLog) {
      return;
    }

    try {
      if (!navigator.clipboard?.writeText) {
        throw new Error("clipboard unavailable");
      }
      await navigator.clipboard.writeText(runtimeLog);
      setConsoleCopyState("copied");
    } catch {
      setConsoleCopyState("failed");
    }
  }

  function handleRuntimeConsoleTabSelect(tab: RuntimeConsoleTab) {
    setRuntimeConsoleTabValue(tab.value);
    setRuntimeTargetValue(tab.processKey ?? PRIMARY_RUNTIME_TARGET);
  }

  function handleRuntimeTargetChange(value: string) {
    setRuntimeTargetValue(value);

    const matchingTab = runtimeConsoleTabs.find((tab) =>
      tab.value === value
      || tab.processKey === value
      || (value === PRIMARY_RUNTIME_TARGET && tab.isPrimary)
    );
    if (matchingTab) {
      setRuntimeConsoleTabValue(matchingTab.value);
    }
  }

  async function handleSuppressRuntimeWindows() {
    if (readOnly || !canSuppressRuntimeWindows || !props.onSuppressRuntimeWindows) {
      return;
    }

    setSuppressingWindows(true);
    try {
      await props.onSuppressRuntimeWindows(props.details.summary.id);
    } finally {
      setSuppressingWindows(false);
    }
  }

  const runtimeHealthSignals = readOnly ? [] : (props.runtime?.diagnostics ?? []).filter((signal) =>
    ["warning", "error"].includes(String(signal.severity).toLowerCase()));
  const nativeHealth = readOnly ? null : props.runtime?.health;
  const instanceFailed = !readOnly && props.details.summary.status === "error";
  const runtimeHealthTone = runtimeErrorText
    ? "danger"
    : startupPending
      ? "warning"
    : instanceFailed || nativeHealth?.status === "error" || runtimeHealthSignals.some((signal) => String(signal.severity).toLowerCase() === "error")
      ? "danger"
      : nativeHealth?.status === "warning" || nativeHealth?.status === "starting" || runtimeHealthSignals.length > 0
        ? "warning"
        : instanceRunning && nativeHealth?.status === "ready" ? "ready" : "idle";
  const runtimeHealthLabel = runtimeErrorText
    ? runtimeErrorText
    : readOnly
      ? t("servers.archives.workspace.caption", undefined, "Archived · Read only; restore to edit")
    : startupPending
      ? t("status.instance.starting", undefined, "Starting")
    : instanceFailed
      ? t("status.instance.error", undefined, "Error")
    : nativeHealth && nativeHealth.status !== "ready"
      ? localizeRuntimeHealthSummary(nativeHealth, locale, t)
    : !instanceRunning
      ? t(`status.instance.${props.details.summary.status}`, undefined, props.details.summary.status)
    : !nativeHealth
      ? t("runtime.health.notAvailable", undefined, "No runtime health summary yet.")
    : runtimeHealthSignals.length > 0
      ? t(
          "servers.details.runtimeDiagnosticsCount",
          { count: runtimeHealthSignals.length },
          `${runtimeHealthSignals.length} signals`
        )
      : t("servers.details.runtimeDiagnosticsClear", undefined, "All clear");

  return (
    <section className="detail-stack server-module-stack">
      <article className="runtime-box workbench-card workbench-card--wide server-runtime-surface-card server-runtime-console-card">
        <div className="server-runtime-console-frame">
          <div className="server-runtime-console-toolbar server-runtime-console-toolbar--primary">
            <div className="server-runtime-console-title">
              <span>{t("servers.runtimeSurface.managedConsole", undefined, "LanGameCMD")}</span>
              <span
                className={`server-runtime-health-light is-${runtimeHealthTone}`}
                role="status"
                aria-label={runtimeHealthLabel}
                title={runtimeHealthLabel}
              />
              {ark || !selectedConsoleTab?.isPrimary ? (
                <span>{selectedConsoleTab?.label ?? props.details.summary.name}</span>
              ) : null}
            </div>
            <div className="server-runtime-terminal-actions">
              {ark && !readOnly && !startupPending && selectedConsoleRunId !== null ? (
                <select className="server-runtime-console-source-select"
                  aria-label={selectLocaleText(locale, "日志来源", "Log source")}
                  value={arkLogSource}
                  onChange={(event) => {
                    const source = event.target.value === "console" ? "console" : "game";
                    setArkLogSelection({ scope: arkLogScope, source });
                    setProcessLogSnapshot(null);
                    setLiveLogSnapshot(null);
                    setProcessLogError(null);
                  }}>
                  <option value="game">{selectLocaleText(locale, "游戏日志", "Game log")}</option>
                  <option value="console">{selectLocaleText(locale, "控制台输出", "Console output")}</option>
                </select>
              ) : null}
              {runtimeTargets.length > 1 ? (
                <select
                  className="server-runtime-console-target-select"
                  value={selectedTarget?.value ?? PRIMARY_RUNTIME_TARGET}
                  onChange={(event) => handleRuntimeTargetChange(event.target.value)}
                  disabled={!instanceRunning}
                  aria-label={t("servers.runtimeSurface.commandTargetProcess", undefined, "Command target process")}
                >
                  {runtimeTargets.map((target) => (
                    <option key={target.value} value={target.value} disabled={target.disabled}>
                      {target.label}
                    </option>
                  ))}
                </select>
              ) : null}
              <button
                type="button"
                className={`server-runtime-console-copy-button ${consoleCopyState !== "idle" ? `is-${consoleCopyState}` : ""}`}
                onClick={() => void handleCopyRuntimeLog()}
                disabled={!canCopyRuntimeLog}
                aria-label={copyConsoleTitle}
                title={copyConsoleTitle}
              >
                <ShellIcon name={consoleCopyState === "copied" ? "check" : "copy"} className="server-runtime-console-copy-icon" />
              </button>
              {runtimeWindowSuppressionSupported && runtimeWindowCount > 0 ? (
                <button
                  type="button"
                  className="secondary-button"
                  onClick={() => void handleSuppressRuntimeWindows()}
                  disabled={!canSuppressRuntimeWindows}
                >
                  {suppressingWindows
                    ? t("servers.runtimeSurface.suppressingWindows", undefined, "Suppressing windows...")
                    : t("servers.runtimeSurface.suppressVisibleWindows", undefined, "Suppress visible windows")}
                </button>
              ) : null}
            </div>
          </div>
          {runtimeConsoleTabs.length > 1 ? (
            <div
              className="server-runtime-console-tabs"
              role="tablist"
              aria-label={t("servers.runtimeSurface.runtimeProcessLogs", undefined, "Runtime process logs")}
            >
              {runtimeConsoleTabs.map((tab) => (
                <button
                  key={tab.value}
                  type="button"
                  role="tab"
                  aria-selected={tab.value === selectedConsoleTab?.value}
                  className={tab.value === selectedConsoleTab?.value
                    ? "server-runtime-console-tab is-active"
                    : "server-runtime-console-tab"}
                  onClick={() => handleRuntimeConsoleTabSelect(tab)}
                >
                  <span>{tab.label}</span>
                  <small>{tab.meta}</small>
                </button>
              ))}
            </div>
          ) : null}
          <pre ref={runtimeConsoleRef} className="log-preview server-log-preview server-runtime-console server-runtime-console--primary"
            data-run-id={selectedConsoleRunId ?? undefined}
            data-process-key={selectedConsoleTab?.processKey ?? undefined}
            data-log-source={ark && !readOnly ? arkLogSource : "default"}
            data-log-path={runtimeLogSourcePath}
            onScroll={(event) => {
              const terminal = event.currentTarget;
              // A refresh can temporarily remove errors and collapse the scroll
              // area. Its clamped scroll event is not a user's return to the tail.
              if (terminal.scrollHeight <= terminal.clientHeight) return;
              consoleFollowRef.current.following = terminal.scrollHeight - terminal.clientHeight - terminal.scrollTop <= 48;
              consoleFollowRef.current.scrollTop = terminal.scrollTop;
            }}
            title={readOnly && archivedLogSnapshot?.truncated ? t("servers.archives.workspace.logTruncated", undefined, "The log display is truncated.") : undefined}>
            {runtimeLogText}
            {commandFeedback ? <span className="server-runtime-command-feedback" role="status">{`\n\n[LanGameCMD] ${commandFeedback}`}</span> : null}
            {runtimeErrorText ? <span className="server-runtime-console-errors" role="alert">
              {`\n\n${runtimeErrorText}`}
              {props.onRetryReads || selectedProcessLogError || streamError ? <>{"\n"}<button
                type="button"
                className="server-runtime-console-retry"
                disabled={retryReadsPending}
                onClick={() => {
                  setProcessLogRetryGeneration(generation => generation + 1);
                  props.onRetryReads?.();
                }}
              >{t("common.retry")}</button></> : null}
            </span> : null}
          </pre>
          <form
            className="server-runtime-console-command-form"
            aria-label={t("servers.runtimeSurface.commandForm", undefined, "Send console commands to the managed process")}
            onSubmit={(event) => {
              event.preventDefault();
              void handleRuntimeCommandSubmit();
            }}
          >
            <span className="server-runtime-console-prompt" aria-hidden="true">{runtimeConsolePrompt}&gt;</span>
            <input
              type="text"
              className="server-runtime-console-command-input"
              value={runtimeCommandDraft}
              placeholder={
                readOnly
                  ? t("servers.archives.workspace.caption", undefined, "Archived · Read only; restore to edit")
                : startupPending
                  ? t("status.instance.starting", undefined, "Starting")
                  : !commandRoute.available && commandHint
                    ? commandHint
                  : instanceRunning
                    ? t("servers.runtimeSurface.commandPlaceholder.running", undefined, "Type a server console command")
                    : t("servers.runtimeSurface.commandPlaceholder.stopped", undefined, "Instance is stopped")
              }
              aria-label={t("servers.runtimeSurface.commandInput", undefined, "Server console command")}
              title={commandHint ?? undefined}
              onChange={(event) => setRuntimeCommandDraft(event.target.value)}
              disabled={!runtimeCommandEnabled || commandPending}
            />
          </form>
        </div>
      </article>
    </section>
  );
}
