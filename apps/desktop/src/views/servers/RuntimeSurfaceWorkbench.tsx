import { useEffect, useId, useLayoutEffect, useMemo, useRef, useState, type SetStateAction } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { describeError } from "../../app-state";
import { moduleRequiresSteamCmd } from "../../module-installation-dependency";
import { readInstanceLogDocument, recordStartupSessionDiagnostic } from "../../api";
import { createRuntimeLogRefresh } from "../../runtime-log-refresh";
import { createRuntimeLogFrameBatch } from "../../runtime-log-frame-batch";
import { runtimeLogDocumentPatchNeedsRefresh } from "../../runtime-log-document-patch";
import { markRuntimeConsoleSourceForRefresh, mergeRuntimeConsoleSnapshot, runtimeConsoleDisplayLines,
  runtimeConsoleEventNeedsRefresh, runtimeConsoleSourceNeedsReadback, selectRuntimeConsoleSource } from "../../runtime-console-stream";
import { runtimeConsoleHistory, runtimeConsoleStartupHistoryScope } from "../../runtime-console-history";
import { appendRuntimeStartupEvent, mergeRuntimeStartupSession, restoreRuntimeStartupConsole, runtimeStartupBeforeCursor, runtimeStartupEventNeedsRefresh,
  runtimeStartupLines, startupDiagnosticId, startupSessionMatchesBoundary, withoutRuntimeStartupSource,
  type RuntimeStartupConsole } from "../../runtime-console-startup-session";
import { formatInstancePanelError, type InstancePanelLoadState } from "../../instance-panel-loader";
import { selectLocaleText, useI18n, type TranslateFn } from "../../i18n";
import {
  appendRuntimeLogStreamEvent,
  collectRuntimeLogPaths,
  normalizeRuntimeLogPath,
  retainRuntimeLogHistory,
  retainRuntimeLogStreamDiagnostic,
  retainNewestRuntimeGameDocument,
  RUNTIME_LOG_STREAM_EVENT,
  RUNTIME_SERVICE_EVENTS_RESET,
  type RuntimeLogStreamEvent,
  type RuntimeLogStreamDiagnostic,
  type RuntimeLogStartupBoundary
} from "../../runtime-log-stream";
import { ShellIcon } from "../../components/ShellIcon";
import { SteamWorkshopAccountConsole } from "./SteamWorkshopAccountConsole";
import type { SteamWorkshopAccountState } from "./useSteamWorkshopAccount";
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
  active?: boolean;
  steamAccount?: SteamWorkshopAccountState;
  steamCmdSelected?: boolean;
  onSteamCmdSelected?: (selected: boolean) => void;
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
  runId: number | null;
  logPath: string;
  isPrimary: boolean;
}

const PRIMARY_RUNTIME_TARGET = "__primary__";

type RuntimeConsoleCopyState = "idle" | "copied" | "failed";
type RuntimeConsoleRead = { snapshot: LogTailSnapshot; recoveryGeneration: number;
  startupGeneration: string | null; startupRetainedStartCursor: string | null; receiptOnly: boolean };
type RuntimeConsoleDiagnosticScope = { instanceId: string; attemptId: string; kind: "startup-read" | "runtime-read";
  runId: number | null; source: RuntimeLogSource | "default" };
type RuntimeConsoleDiagnosticOccurrence = { id: string; scope: string; recovered: boolean; retained: boolean;
  inFlight: Promise<void> | null };

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
  const nativeDisplayName = String(process.display_name ?? "");
  const displayName = nativeDisplayName.trim();
  const key = processKey.toLowerCase();

  if (ark && displayName) return nativeDisplayName;

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
    const logPath = String(process.log_path ?? "").trim();

    tabs.push({
      value,
      processKey: processKey || null,
      label: formatRuntimeProcessLabel(process, t, ark),
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
  const showSteamCmd = !props.archive && (props.moduleDetails?.workshop?.provider === "steam"
    || moduleRequiresSteamCmd(props.moduleDetails?.summary ?? {}, props.moduleDetails?.install)) && props.steamAccount?.available
    && props.steamCmdSelected && props.onSteamCmdSelected;
  // Startup output belongs to the in-flight operation, including while its
  // SteamCMD or another detail tab is visible. Keep its listener mounted.
  return <>
    {!showSteamCmd || props.startupPending ? <GameRuntimeSurfaceWorkbench {...props}
      active={props.active !== false && !showSteamCmd} /> : null}
    {showSteamCmd && props.steamAccount ? <SteamWorkshopAccountConsole account={props.steamAccount}
      active={props.active} onShowServer={() => props.onSteamCmdSelected?.(false)} /> : null}
  </>;
}

function GameRuntimeSurfaceWorkbench(props: RuntimeSurfaceWorkbenchProps) {
  const { t, locale } = useI18n();
  const targetTooltipId = useId();
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
  const [liveLogSnapshot, setRenderedLogSnapshot] = useState<LogTailSnapshot | null>(null);
  const liveLogSnapshotRef = useRef<LogTailSnapshot | null>(null);
  const logFrames = useMemo(() => createRuntimeLogFrameBatch<LogTailSnapshot | null>({
    schedule: callback => window.requestAnimationFrame(callback),
    cancel: handle => window.cancelAnimationFrame(handle),
    publish: setRenderedLogSnapshot
  }), []);
  function setLiveLogSnapshot(update: SetStateAction<LogTailSnapshot | null>, nextFrame = false) {
    const current = liveLogSnapshotRef.current;
    const next = typeof update === "function" ? update(current) : update;
    if (next === current) return;
    liveLogSnapshotRef.current = next;
    if (nextFrame) logFrames.push(next);
    else {
      logFrames.clear();
      setRenderedLogSnapshot(next);
    }
  }
  const [processLogLoading, setProcessLogLoading] = useState(false);
  const [processLogError, setProcessLogError] = useState<{ scope: string; message: string } | null>(null);
  const [processLogRetryGeneration, setProcessLogRetryGeneration] = useState(0);
  const [streamDiagnostic, setStreamDiagnostic] = useState<RuntimeLogStreamDiagnostic | null>(null);
  const streamError = streamDiagnostic?.message ?? null;
  const [commandFeedback, setCommandFeedback] = useState<string | null>(null);
  const [commandPending, setCommandPending] = useState(false);
  const logRefreshRef = useRef<ReturnType<typeof createRuntimeLogRefresh<RuntimeConsoleRead>> | null>(null);
  const logRecoveryGenerationRef = useRef(0);
  const commandScope = `${props.details.summary.id}:${props.details.active_run?.run_id ?? ""}:${runtimeTargetValue}`;
  const commandScopeRef = useRef(commandScope);
  commandScopeRef.current = commandScope;
  const [suppressingWindows, setSuppressingWindows] = useState(false);
  const [consoleCopyState, setConsoleCopyState] = useState<RuntimeConsoleCopyState>("idle");
  const [targetTooltipDismissed, setTargetTooltipDismissed] = useState(false);
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
  const [startupConsole, renderStartupConsole] = useState<RuntimeStartupConsole | null>(null);
  const [startupReceiptLoaded, setStartupReceiptLoaded] = useState(false);
  const startupConsoleRef = useRef<RuntimeStartupConsole | null>(null);
  const startupOwnerRef = useRef(props.details.summary.id);
  const startupRunSelectionRef = useRef<{ attemptId: string; mode: "startup" | "history" } | null>(null);
  const [startupPagePending, setStartupPagePending] = useState(false);
  const [startupPageError, setStartupPageError] = useState<string | null>(null);
  const startupPageGenerationRef = useRef(0);
  const startupPageAnchorRef = useRef<{ height: number; top: number } | null>(null);
  const startupDiagnosticIdsRef = useRef(new Map<string, RuntimeConsoleDiagnosticOccurrence>());
  const startupDiagnosticSequenceRef = useRef(0);
  function setStartupConsole(update: (current: RuntimeStartupConsole | null) => RuntimeStartupConsole | null) {
    const next = update(startupConsoleRef.current);
    if (next === startupConsoleRef.current) return;
    startupConsoleRef.current = next;
    renderStartupConsole(next);
  }

  async function retainReadDiagnostic(scope: RuntimeConsoleDiagnosticScope, message: string) {
    if (startupOwnerRef.current !== scope.instanceId || startupConsoleRef.current?.session.attempt_id !== scope.attemptId
      || scope.attemptId.startsWith("local:")) return;
    const scopeKey = JSON.stringify(scope);
    const key = `${scopeKey}:${startupDiagnosticId("console-read", message)}`;
    let occurrence = startupDiagnosticIdsRef.current.get(key);
    if (!occurrence || occurrence.recovered) {
      occurrence = { id: `${startupDiagnosticId("console-read", message)}:${Date.now()}:${++startupDiagnosticSequenceRef.current}`,
        scope: scopeKey, recovered: false, retained: false, inFlight: null };
      startupDiagnosticIdsRef.current.set(key, occurrence);
    }
    if (occurrence.inFlight) return occurrence.inFlight;
    if (occurrence.retained) return;
    const currentOccurrence = occurrence;
    const contextualMessage = `[${scope.kind}${scope.runId === null ? "" : ` run=${scope.runId}`} source=${scope.source}] ${message}`;
    currentOccurrence.inFlight = (async () => {
      try {
        await recordStartupSessionDiagnostic(scope.instanceId, scope.attemptId, currentOccurrence.id, contextualMessage);
        const current = startupConsoleRef.current?.session;
        if (startupOwnerRef.current !== scope.instanceId || current?.attempt_id !== scope.attemptId) return;
        // A diagnostic can outlive the reader that failed. Read its owned receipt
        // directly, never repeat a now-obsolete run/source request to recover it.
        const readBasis = { readGeneration: current.line_generation, readRetainedStartCursor: current.retained_start_cursor };
        const snapshot = await readInstanceLogDocument(scope.instanceId, 400, undefined, undefined, scope.attemptId);
        if (snapshot.startup_session?.attempt_id === scope.attemptId && startupOwnerRef.current === scope.instanceId
          && startupConsoleRef.current?.session.attempt_id === scope.attemptId) {
          const session = snapshot.startup_session;
          setStartupConsole(value => mergeRuntimeStartupSession(value, session, readBasis));
          setStartupPageError(value => value?.includes(contextualMessage) ? null : value);
          currentOccurrence.retained = true;
        }
      } catch (error: unknown) {
        // A lost write acknowledgement is not evidence that nothing was saved.
        // Keep this occurrence's ID for retries and merge concurrent callers.
        if (startupOwnerRef.current === scope.instanceId && startupConsoleRef.current?.session.attempt_id === scope.attemptId) {
          setStartupPageError(`[${new Date().toISOString()}] [LanGameCMD/history] ${contextualMessage}\n${describeError(error)}`);
        }
        console.warn("Runtime read diagnostic could not be retained or read back.", error);
      } finally {
        currentOccurrence.inFlight = null;
        if (currentOccurrence.recovered && currentOccurrence.retained && startupDiagnosticIdsRef.current.get(key) === currentOccurrence) {
          startupDiagnosticIdsRef.current.delete(key);
        }
      }
    })();
    return currentOccurrence.inFlight;
  }

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
    setStreamDiagnostic(null);
    setSuppressingWindows(false);
    setConsoleCopyState("idle");
    setCommandFeedback(null);
    setCommandPending(false);
    startupOwnerRef.current = props.details.summary.id;
    setStartupReceiptLoaded(false);
    startupRunSelectionRef.current = null;
    setStartupConsole(() => null);
    startupPageGenerationRef.current++;
    startupPageAnchorRef.current = null;
    setStartupPagePending(false);
    setStartupPageError(null);
    startupDiagnosticIdsRef.current.clear();
  }, [props.details.summary.id, readOnly]);

  useEffect(() => {
    if (!startupPending || !props.startupBoundary) return;
    const boundary = props.startupBoundary;
    setStartupConsole(current => current && startupSessionMatchesBoundary(current.session, boundary) ? current : null);
    startupPageGenerationRef.current++;
    setStartupPagePending(false);
    setStartupPageError(null);
  }, [startupPending, props.startupBoundary]);

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
  // Reading a stopped map's history must never reroute a command to another
  // live map. Non-ARK primary consoles retain their default transport target.
  const selectedTargetKey = !ark && selectedConsoleTab?.isPrimary
    ? PRIMARY_RUNTIME_TARGET : selectedConsoleTab?.processKey ?? PRIMARY_RUNTIME_TARGET;
  const selectedTarget = runtimeTargets.find((target) => target.value === selectedTargetKey) ?? null;

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
  const selectedStartupConsole = !readOnly && startupOwnerRef.current === props.details.summary.id && startupConsole
    && (!startupPending || startupSessionMatchesBoundary(startupConsole.session, props.startupBoundary)) ? startupConsole : null;
  const startupSession = selectedStartupConsole?.session;
  const startupReceiptKnown = startupReceiptLoaded || Boolean(startupSession);
  const startupRunSelection = startupSession ? (startupRunSelectionRef.current?.attemptId === startupSession.attempt_id
    ? startupRunSelectionRef.current : { attemptId: startupSession.attempt_id, mode: "startup" as const }) : null;
  useEffect(() => {
    if (startupRunSelectionRef.current?.attemptId !== startupRunSelection?.attemptId) startupDiagnosticIdsRef.current.clear();
    startupRunSelectionRef.current = startupRunSelection;
  }, [startupSession?.attempt_id]);
  // A completed attempt can reach this view before its new run details. The
  // previous recent_run is history, including after reloading during startup.
  // An explicit process/history selection below opts out of this handoff guard.
  const awaitingStartupRun = !startupPending && startupRunSelection?.mode === "startup"
    && (startupSession?.status === "starting" || startupSession?.status === "succeeded")
    && (selectedConsoleRunId === null || !startupSession.run_ids.includes(selectedConsoleRunId));
  const arkLogScope = JSON.stringify([props.details.summary.id, selectedConsoleRunId]);
  const arkLogSource: RuntimeLogSource = arkLogSelection?.scope === arkLogScope ? arkLogSelection.source : "game";
  const selectedGameLog = ark && !readOnly && !startupPending && selectedConsoleRunId !== null && arkLogSource === "game";
  const selectedLogReadScope = JSON.stringify([props.details.summary.id, selectedConsoleTab?.processKey, selectedConsoleRunId, ark ? arkLogSource : null]);
  useEffect(() => () => logFrames.clear(), [logFrames, selectedLogReadScope, selectedConsoleTab?.logPath, startupPending, readOnly]);
  const historyScope = useMemo(() => ({ instanceId: props.details.summary.id, moduleId: props.details.summary.module_id,
    processKey: selectedConsoleTab?.processKey ?? null, runId: selectedConsoleRunId, source: ark ? arkLogSource : "default" }),
  [props.details.summary.id, props.details.summary.module_id, selectedConsoleTab?.processKey, selectedConsoleRunId, ark, arkLogSource]);
  const startupHistoryScope = useMemo(() => startupPending && props.startupBoundary?.instanceId === props.details.summary.id
    ? runtimeConsoleStartupHistoryScope(props.details.summary.module_id, props.startupBoundary) : null,
  [startupPending, props.startupBoundary, props.details.summary.id, props.details.summary.module_id]);
  const retainedConsolePath = selectedGameLog ? "" : selectedConsoleTab?.isPrimary && !ark
    ? fallbackLogPath : selectedConsoleTab?.logPath ?? "";
  useEffect(() => {
    if (startupPending) {
      const cached = !readOnly && startupHistoryScope ? runtimeConsoleHistory.read(startupHistoryScope) : null;
      setLiveLogSnapshot(cached);
      setStartupConsole(current => restoreRuntimeStartupConsole(current, cached, props.startupBoundary));
      setStreamDiagnostic(cached?.startup_stream_diagnostic ?? null);
      return;
    }
    const cached = readOnly ? null : runtimeConsoleHistory.read(historyScope);
    setProcessLogSnapshot(cached);
    setProcessLogReadScope(cached ? selectedLogReadScope : "");
    // Registration changes run/process identity without changing the owned
    // managed log. Keep that source while the new scope's read is in flight.
    setLiveLogSnapshot(current => !readOnly && retainedConsolePath
      ? selectRuntimeConsoleSource(current, retainedConsolePath) ?? cached : cached);
  }, [selectedLogReadScope, historyScope, readOnly, startupPending, startupHistoryScope, retainedConsolePath]);
  const selectedProcessLog = processLogReadScope === selectedLogReadScope ? processLogSnapshot : null;
  const selectedProcessLogError = processLogError?.scope === selectedLogReadScope ? processLogError.message : null;
  const selectedConsoleIsPrimary = Boolean(selectedConsoleTab?.isPrimary);
  const selectedConsoleUsesDefaultLog = selectedConsoleIsPrimary && !ark;
  const selectedConsoleLogPath = selectedGameLog ? selectedProcessLog?.source_path ?? ""
    : selectedConsoleUsesDefaultLog ? fallbackLogPath : selectedConsoleTab?.logPath ?? "";
  const selectedRunIsActive = selectedConsoleRunId !== null && (
    (instanceRunning && props.details.active_run?.run_id === selectedConsoleRunId)
    || props.details.active_run?.processes?.some(process => process.run_id === selectedConsoleRunId && runtimeProcessIsRunning(process)));
  const diagnosticScope: RuntimeConsoleDiagnosticScope | null = startupSession
    && (((startupPending || awaitingStartupRun) && startupRunSelection?.mode !== "history")
      || (selectedRunIsActive && selectedConsoleRunId !== null && startupSession.run_ids.includes(selectedConsoleRunId)))
    ? { instanceId: props.details.summary.id, attemptId: startupSession.attempt_id,
      kind: startupPending || awaitingStartupRun ? "startup-read" : "runtime-read",
      runId: startupPending || awaitingStartupRun ? null : selectedConsoleRunId,
      source: startupPending || awaitingStartupRun ? "default" : ark ? arkLogSource : "default" } : null;
  const diagnosticReadScope = JSON.stringify(diagnosticScope);

  useEffect(() => {
    if (readOnly || !selectedConsoleTab) {
      setProcessLogSnapshot(null);
      setProcessLogLoading(false);
      setProcessLogError(null);
      return;
    }

    // Map source selection is resolved by the backend from the owned run;
    // never infer another map's filename or reuse its previous snapshot.
    let disposed = false;
    const refresh = createRuntimeLogRefresh({
      read: async () => {
        const recoveryGeneration = logRecoveryGenerationRef.current;
        const startupGeneration = startupConsoleRef.current?.session.line_generation ?? null;
        const startupRetainedStartCursor = startupConsoleRef.current?.session.retained_start_cursor ?? null;
        const receiptOnly = !startupPending && (!startupReceiptKnown || awaitingStartupRun);
        const retainFailure = async (message: string) => {
          if (!diagnosticScope) return;
          await retainReadDiagnostic(diagnosticScope, message);
        };
        try {
          const snapshot = await readInstanceLogDocument(props.details.summary.id, 400,
            startupPending || receiptOnly || selectedConsoleUsesDefaultLog ? undefined : selectedConsoleRunId ?? undefined,
            !startupPending && !receiptOnly && ark && selectedConsoleRunId !== null ? arkLogSource : undefined);
          if (snapshot.read_error) await retainFailure(snapshot.read_error);
          else if (!disposed && diagnosticScope) {
            // Only this original source's successful read ends its failure
            // occurrence. Reading the diagnostic receipt never marks recovery.
            for (const [key, occurrence] of startupDiagnosticIdsRef.current) {
              if (occurrence.scope === diagnosticReadScope) {
                occurrence.recovered = true;
                if (occurrence.retained && !occurrence.inFlight) startupDiagnosticIdsRef.current.delete(key);
              }
            }
          }
          return { snapshot, recoveryGeneration, startupGeneration, startupRetainedStartCursor, receiptOnly };
        } catch (error: unknown) {
          // Disposal suppresses UI publication, not an owned failure that arrived
          // after startup succeeded or its selected run details changed.
          await retainFailure(describeError(error));
          throw error;
        }
      },
      publish: ({ snapshot, recoveryGeneration, startupGeneration, startupRetainedStartCursor, receiptOnly }) => {
          if (recoveryGeneration !== logRecoveryGenerationRef.current) return;
          const session = snapshot.startup_session;
          if (session) setStartupConsole(current => mergeRuntimeStartupSession(current, session,
            { boundary: startupPending ? props.startupBoundary : null,
              readGeneration: startupGeneration, readRetainedStartCursor: startupRetainedStartCursor }));
          setStartupReceiptLoaded(true);
          if (receiptOnly) { setProcessLogError(null); return; }
          setProcessLogSnapshot(snapshot);
          setProcessLogReadScope(selectedLogReadScope);
          setLiveLogSnapshot((current) => snapshot.console_sources
            ? mergeRuntimeConsoleSnapshot(current, snapshot, { maxLines: 400, mergeProcessLogs: startupPending,
              startupBoundary: startupPending ? startupBoundaryRef.current?.value : null })
            : startupPending || (current?.console_sources?.length
              && normalizeRuntimeLogPath(current.source_path) === normalizeRuntimeLogPath(snapshot.source_path)
              && !runtimeConsoleSourceNeedsReadback(current, snapshot.source_path)) ? current
            : selectedGameLog && snapshot.snapshot_revision !== undefined
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
      disposed = true;
      refresh.dispose();
      if (logRefreshRef.current === refresh) logRefreshRef.current = null;
    };
  }, [props.details.summary.id, props.startupBoundary, readOnly, startupPending, selectedConsoleUsesDefaultLog, selectedConsoleRunId,
    selectedConsoleTab?.value, selectedConsoleTab?.logPath, processLogRetryGeneration, selectedLogReadScope, ark, arkLogSource, selectedGameLog,
    awaitingStartupRun, startupReceiptKnown, diagnosticReadScope,
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
  const polledLogSnapshot = startupPending || selectedConsoleUsesDefaultLog
    ? props.runtime?.log_tail ?? null
    : selectedProcessLog;
  const selectedSourcePath = normalizeRuntimeLogPath(selectedConsoleLogPath);
  // A newly selected map can be silent or fail its first read. Never label the
  // previous map's retained output as belonging to that selected command target.
  const selectedLiveLog = useMemo(() => {
    if (!startupPending && !selectedGameLog && selectedSourcePath) {
      return selectRuntimeConsoleSource(liveLogSnapshot, selectedSourcePath);
    }
    return (!selectedGameLog || selectedSourcePath) && (!selectedSourcePath || normalizeRuntimeLogPath(liveLogSnapshot?.source_path) === selectedSourcePath)
      ? liveLogSnapshot : null;
  }, [liveLogSnapshot, selectedGameLog, selectedSourcePath, startupPending]);
  const selectedPolledLog = !selectedSourcePath || normalizeRuntimeLogPath(polledLogSnapshot?.source_path) === selectedSourcePath
    ? polledLogSnapshot : null;
  const startupReadbackBoundary = props.startupBoundary?.instanceId === props.details.summary.id
    ? props.startupBoundary : startupBoundaryRef.current?.value;
  const startupReadback = startupReadbackBoundary?.instanceId === props.details.summary.id
    && selectedPolledLog?.source_path
    && !startupReadbackBoundary.previousLogPaths.has(normalizeRuntimeLogPath(selectedPolledLog.source_path))
    ? selectedPolledLog : null;
  // After a UI reload, the backend identifies the pending operation's log even
  // though this webview no longer owns its original startup boundary/capture.
  const recoveredStartupReadback = !props.startupBoundary && props.runtime?.health.reason?.code === "startup_pending"
    ? props.runtime.log_tail : null;
  const selectedLogSnapshot = readOnly ? archivedLogSnapshot : startupPending
    // A late mount may already have the new primary's snapshot. Display it only
    // until shard events arrive; never seed their unpositioned aggregate with it.
    ? liveLogSnapshot ?? startupReadback ?? recoveredStartupReadback
    : awaitingStartupRun ? null : selectedLiveLog ?? selectedPolledLog;
  const startupHistoryLines = runtimeStartupLines(selectedStartupConsole,
    t("servers.runtimeSurface.startupHistoryGap", undefined, "[LanGameCMD] More startup history is available above."));
  const gameLogSnapshot = withoutRuntimeStartupSource(selectedLogSnapshot, startupSession?.source_path ?? null, startupPending);
  const gameLogLines = runtimeConsoleDisplayLines(gameLogSnapshot, startupPending);
  const managedConsoleSource = Boolean(gameLogSnapshot?.console_sources?.length)
    || normalizeRuntimeLogPath(gameLogSnapshot?.source_path).includes("/managed-console/");
  const startupHeading = startupSession ? `[LanGameCMD] ${t("servers.runtimeSurface.startupHistory", undefined, "Startup process (checks / updates / game startup)")} · ${new Date(startupSession.started_at_unix_ms).toLocaleString(locale)} · ${t(`servers.runtimeSurface.startupStatus.${startupSession.status}`, undefined, startupSession.status)}` : "";
  const startupPartialLines = startupSession?.status === "starting" ? selectedStartupConsole?.live?.console_sources?.[0]?.partial_lines ?? [] : [];
  const startupCompletedLines = startupPartialLines.length ? startupHistoryLines.slice(0, -startupPartialLines.length) : startupHistoryLines;
  // The outcome is normally a retained log row. Keep its receipt fallback
  // readable when that row itself could not be written or read.
  const startupOutcomeError = startupSession?.error && !startupCompletedLines.some(line => line.includes(startupSession.error ?? ""))
    ? `[${new Date(startupSession.finished_at_unix_ms ?? startupSession.started_at_unix_ms).toISOString()}] [LanGameCMD/startup] ${startupSession.error}` : null;
  const runtimeHeading = `[LanGameCMD] ${managedConsoleSource
    ? t("servers.runtimeSurface.processConsoleOutput", undefined, "Process console output")
    : t("servers.runtimeSurface.gameRuntimeLogs", undefined, "Game runtime log")}`;
  const runtimeLogLines = startupSession ? [startupHeading, ...startupHistoryLines,
    ...(startupOutcomeError ? [startupOutcomeError] : []), ...(startupSession.reader_error ? [startupSession.reader_error] : []),
    ...(gameLogLines.length ? ["", runtimeHeading, ...gameLogLines] : [])] : gameLogLines;
  const startupBeforeCursor = runtimeStartupBeforeCursor(selectedStartupConsole);
  useEffect(() => {
    if (!readOnly && !startupPending && processLogReadScope === selectedLogReadScope && selectedLogSnapshot) {
      runtimeConsoleHistory.write(historyScope, selectedLogSnapshot);
    }
  }, [historyScope, selectedLogSnapshot, selectedLogReadScope, processLogReadScope, readOnly, startupPending]);
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
  useEffect(() => {
    const session = startupConsoleRef.current?.session;
    if (readOnly || !session || session.attempt_id.startsWith("local:")) return;
    if (!diagnosticScope) return;
    const messages = startupPending || awaitingStartupRun ? runtimeReadIdentities
      : new Set([selectedProcessLogError, selectedLogSnapshot?.read_error].filter((value): value is string => Boolean(value)));
    for (const message of messages) {
      // The service already records producer failures in its owned receipt.
      if (message === streamError) continue;
      void retainReadDiagnostic(diagnosticScope, message);
    }
  }, [runtimeErrorText, startupPending, awaitingStartupRun, startupSession?.attempt_id, props.details.summary.id, readOnly, diagnosticReadScope]);
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
    startupSession?.attempt_id ?? selectedConsoleRunId, ark ? arkLogSource : null,
    startupSession ? null : normalizeRuntimeLogPath(runtimeLogSourcePath), startupSession ? null : startupPending,
    startupSession ? null : startupPending ? props.startupBoundary?.startedAtUnixMs : null]);
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
    const previousLogPaths = new Set(source?.previousLogPaths ?? collectRuntimeLogPaths(props.details, props.runtime));
    if (!source && props.runtime?.health.reason?.code === "startup_pending") {
      previousLogPaths.delete(normalizeRuntimeLogPath(props.runtime.log_tail.source_path));
    }
    // Freeze only the previous session's paths. Polls may already contain the
    // new run on mount, and later shards must stay eligible for this startup.
    startupBoundaryRef.current = { source, value: {
      instanceId: props.details.summary.id,
      startupAttemptId: source?.startupAttemptId,
      startedAtUnixMs: source?.startedAtUnixMs ?? 0,
      previousLogPaths: new Set([
        ...previousLogPaths,
        ...observedLogPathsRef.current.paths
      ])
    } };
    observedLogPathsRef.current.paths.clear();
    setLiveLogSnapshot(runtimeConsoleHistory.read(runtimeConsoleStartupHistoryScope(
      props.details.summary.module_id, startupBoundaryRef.current.value
    )));
  }, [props.details.summary.id, props.details.summary.module_id, startupPending, props.startupBoundary]);

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
    setLiveLogSnapshot((current) => polledLogSnapshot?.console_sources
      ? mergeRuntimeConsoleSnapshot(current, polledLogSnapshot, { maxLines: 400, mergeProcessLogs: startupPending,
        startupBoundary: startupPending ? startupBoundaryRef.current?.value : null })
      : startupPending ? current
      : selectedGameLog && polledLogSnapshot?.snapshot_revision !== undefined
        ? retainNewestRuntimeGameDocument(current, polledLogSnapshot, 400)
      : retainRuntimeLogHistory(current, polledLogSnapshot, 400));
  }, [polledLogSnapshot, props.details.summary.id, readOnly, startupPending, props.startupBoundary, selectedConsoleTab?.value, selectedGameLog]);

  useEffect(() => {
    if (readOnly || !isTauri()) {
      return;
    }

    let disposed = false;
    setStreamDiagnostic(startupHistoryScope ? runtimeConsoleHistory.read(startupHistoryScope)?.startup_stream_diagnostic ?? null : null);
    const unlisten = listen<RuntimeLogStreamEvent>(RUNTIME_LOG_STREAM_EVENT, (event) => {
      if (disposed) {
        return;
      }
      if (event.payload.instance_id !== props.details.summary.id) return;
      if (event.payload.process_key === "startup") {
        const current = startupConsoleRef.current;
        const boundary = startupPending ? props.startupBoundary ?? (current ? null : startupBoundaryRef.current?.value) : null;
        const samePath = current && normalizeRuntimeLogPath(current.session.source_path) === normalizeRuntimeLogPath(event.payload.log_path);
        if (!boundary && !samePath) return;
        if (typeof event.payload.stream_error === "string" || runtimeStartupEventNeedsRefresh(current, event.payload)) {
          logRefreshRef.current?.request();
        } else setStartupConsole(value => appendRuntimeStartupEvent(value, event.payload, boundary));
        return;
      }
      if (awaitingStartupRun || (!startupPending && !startupReceiptKnown)) return;
      // Startup is the operation-owned console transcript. Native game files
      // only have a positioned baseline after selecting their running source.
      if (startupPending && (event.payload.snapshot || event.payload.document_patch)) return;
      // Observe unselected shards as well so the next Start can exclude their
      // delayed final deliveries without rendering them in this selected tab.
      observedLogPathsRef.current.paths.add(normalizeRuntimeLogPath(event.payload.log_path));
      // A complete game document can arrive before its read request resolves.
      // Buffer it by the known run/map identity; the response will establish the
      // path without overwriting a newer event. Console deltas cannot enter here.
      if (selectedGameLog && ((!event.payload.snapshot && !event.payload.document_patch) || event.payload.run_id !== selectedConsoleRunId
        || event.payload.process_key !== selectedConsoleTab?.processKey)) return;
      if (!startupPending && runtimeLogSubscriptionPath &&
        normalizeRuntimeLogPath(event.payload.log_path) !== normalizeRuntimeLogPath(runtimeLogSubscriptionPath)) return;
      if (!startupPending && event.payload.run_id != null && selectedConsoleRunId != null
        && event.payload.run_id !== selectedConsoleRunId) return;
      const boundary = startupPending ? startupBoundaryRef.current?.value : null;
      if (boundary?.instanceId === props.details.summary.id && (
        boundary.previousLogPaths.has(normalizeRuntimeLogPath(event.payload.log_path))
        || (event.payload.console?.started_at_unix_ms ?? event.payload.emitted_at_unix_ms) < boundary.startedAtUnixMs
      )) return;
      setStreamDiagnostic(current => retainRuntimeLogStreamDiagnostic(current, event.payload));
      if (typeof event.payload.stream_error === "string") {
        logRecoveryGenerationRef.current++;
        setLiveLogSnapshot(current => markRuntimeConsoleSourceForRefresh(current, event.payload.log_path));
        logRefreshRef.current?.request();
        return;
      }
      if (runtimeConsoleEventNeedsRefresh(liveLogSnapshotRef.current, event.payload)
        || runtimeLogDocumentPatchNeedsRefresh(liveLogSnapshotRef.current, event.payload)) {
        logRefreshRef.current?.request();
        return;
      }
      if (!startupPending && !event.payload.snapshot && !event.payload.console && !event.payload.document_patch) {
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
      ), Boolean(event.payload.console || event.payload.document_patch));
    }).catch((error: unknown) => {
      if (!disposed) setStreamDiagnostic({ instanceId: props.details.summary.id,
        processKey: selectedConsoleTab?.processKey ?? null, runId: selectedConsoleRunId,
        logPath: normalizeRuntimeLogPath(runtimeLogSubscriptionPath), message: describeError(error) });
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
    selectedGameLog, selectedConsoleRunId, selectedConsoleTab?.processKey, startupHistoryScope, awaitingStartupRun, startupReceiptKnown]);

  useEffect(() => {
    if (readOnly || !isTauri()) return;
    let disposed = false;
    const unlisten = listen(RUNTIME_SERVICE_EVENTS_RESET, () => {
      if (disposed) return;
      // A restarted service or an event retention gap requires an authoritative
      // file read. Reuse the panel refresh and selected shard retry paths.
      logRefreshRef.current?.dispose();
      logRefreshRef.current = null;
      runtimeConsoleHistory.clearInstance(props.details.summary.id);
      setLiveLogSnapshot(null);
      setProcessLogSnapshot(null);
      setProcessLogReadScope("");
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
  }, [props.details.summary.id, props.onRetryReads, readOnly]);

  useLayoutEffect(() => {
    if (props.active === false) return;
    const follow = consoleFollowRef.current;
    const terminal = runtimeConsoleRef.current;
    const anchor = startupPageAnchorRef.current;
    if (terminal && anchor) {
      terminal.scrollTop = anchor.top + terminal.scrollHeight - anchor.height;
      startupPageAnchorRef.current = null;
      follow.scope = consoleScrollScope;
      follow.following = false;
      follow.scrollTop = terminal.scrollTop;
      return;
    }
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
  }, [consoleScrollScope, runtimeLogText, commandFeedback, props.active]);

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

  async function loadOlderStartupHistory() {
    if (!startupSession || !startupBeforeCursor || startupPagePending) return;
    const generation = ++startupPageGenerationRef.current;
    const attemptId = startupSession.attempt_id;
    const readBasis = { readGeneration: startupSession.line_generation, readRetainedStartCursor: startupSession.retained_start_cursor };
    setStartupPagePending(true);
    setStartupPageError(null);
    try {
      const snapshot = await readInstanceLogDocument(props.details.summary.id, 400, undefined, undefined, attemptId, startupBeforeCursor);
      if (generation !== startupPageGenerationRef.current || startupConsoleRef.current?.session.attempt_id !== attemptId) return;
      if (!snapshot.startup_session || snapshot.startup_session.attempt_id !== attemptId) throw new Error(
        t("servers.runtimeSurface.startupHistoryChanged", undefined, "The startup session changed. Refresh its history."));
      if (snapshot.startup_session.snapshot.read_error) throw new Error(snapshot.startup_session.snapshot.read_error);
      const page = snapshot.startup_session;
      const next = mergeRuntimeStartupSession(startupConsoleRef.current, page, { page: true, ...readBasis });
      if (next === startupConsoleRef.current) return;
      const terminal = runtimeConsoleRef.current;
      if (terminal) startupPageAnchorRef.current = { height: terminal.scrollHeight, top: terminal.scrollTop };
      setStartupConsole(() => next);
    } catch (error) {
      if (generation === startupPageGenerationRef.current) setStartupPageError(describeError(error));
    } finally {
      if (generation === startupPageGenerationRef.current) setStartupPagePending(false);
    }
  }

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

  function handleRuntimeTargetChange(value: string) {
    if (startupSession) startupRunSelectionRef.current = { attemptId: startupSession.attempt_id, mode: "history" };
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
    <section className="detail-stack server-module-stack" hidden={props.active === false}
      style={props.active === false ? { display: "none" } : undefined}>
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
            </div>
            <div className="server-runtime-terminal-actions">
              {props.steamAccount?.available && (props.moduleDetails?.workshop?.provider === "steam"
                || moduleRequiresSteamCmd(props.moduleDetails?.summary ?? {}, props.moduleDetails?.install)) && props.onSteamCmdSelected ? <select
                className="server-runtime-console-target-select server-steamcmd-source" value="server"
                aria-label={t("servers.mods.steamAccount.consoleSource")} onChange={() => props.onSteamCmdSelected?.(true)}>
                <option value="server">{t("servers.mods.steamAccount.serverConsole")}</option><option value="steamcmd">SteamCMD</option>
              </select> : null}
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
              {runtimeConsoleTabs.length > 1 ? (
                <span className={`server-runtime-console-target-picker${targetTooltipDismissed ? " is-tooltip-dismissed" : ""}`}
                  onMouseEnter={() => setTargetTooltipDismissed(false)} onFocus={() => setTargetTooltipDismissed(false)}
                  onKeyDown={(event) => { if (event.key === "Escape") setTargetTooltipDismissed(true); }}>
                  <select
                    className="server-runtime-console-target-select"
                    value={selectedConsoleTab?.value ?? PRIMARY_RUNTIME_TARGET}
                    onChange={(event) => handleRuntimeTargetChange(event.target.value)}
                    aria-label={t("servers.runtimeSurface.runtimeProcessLogs", undefined, "Runtime process logs")}
                    aria-describedby={targetTooltipId}
                  >
                    {runtimeConsoleTabs.map((tab) => (
                      <option key={tab.value} value={tab.value}>
                        {tab.label}
                      </option>
                    ))}
                  </select>
                  <span id={targetTooltipId} className="server-runtime-console-target-tooltip" role="tooltip">{selectedConsoleTab?.label}</span>
                </span>
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
          <pre ref={runtimeConsoleRef} className="log-preview server-log-preview server-runtime-console server-runtime-console--primary"
            data-run-id={selectedConsoleRunId !== null && !awaitingStartupRun
              && (!startupPending || startupSession?.run_ids.includes(selectedConsoleRunId)) ? selectedConsoleRunId : undefined}
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
            {startupSession ? <>
              {startupBeforeCursor ? <><button type="button" className="server-runtime-console-retry"
                data-startup-history-more="true" disabled={startupPagePending} onClick={() => void loadOlderStartupHistory()}>
                {startupPagePending ? t("servers.runtimeSurface.loadingStartupHistory", undefined, "Loading startup history...")
                  : t("servers.runtimeSurface.loadStartupHistory", undefined, "Load earlier startup history")}
              </button>{"\n"}</> : null}
              {startupPageError ? <span className="server-runtime-console-errors" role="alert">{`[LanGameCMD] ${startupPageError}\n`}</span> : null}
              <span data-log-region="startup" data-startup-attempt-id={startupSession.attempt_id}
                data-source-path={startupSession.source_path} data-startup-status={startupSession.status}
                data-stream-revision={selectedStartupConsole?.live?.console_sources?.[0]?.revision}>
                <span className={startupSession.status === "failed" || startupSession.status === "interrupted" ? "server-runtime-console-errors" : undefined}>{startupHeading}</span>
                {startupCompletedLines.length ? "\n" : ""}<span data-startup-log-content="true">{startupCompletedLines.join("\n")}</span>
                <span data-startup-progress="true">{startupPartialLines.length ? `\n${startupPartialLines.join("\n")}` : ""}</span>
                {startupOutcomeError ? <span className="server-runtime-console-errors" role="alert">{`\n${startupOutcomeError}`}</span> : null}
                {startupSession.reader_error ? <span className="server-runtime-console-errors" role="alert">{`\n${startupSession.reader_error}`}</span> : null}
              </span>
              {gameLogLines.length ? `\n\n${runtimeHeading}\n` : ""}
              <span data-log-region="runtime" data-source-path={runtimeLogSourcePath}>{gameLogLines.join("\n")}</span>
            </> : <span data-log-region="runtime" data-source-path={runtimeLogSourcePath}>{runtimeLogText}</span>}
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
