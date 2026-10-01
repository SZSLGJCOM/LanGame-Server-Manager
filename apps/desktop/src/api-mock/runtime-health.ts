import type {
  InstanceDetails,
  LogTailSnapshot,
  RuntimeDiagnosticSignal,
  RuntimeHealthSummary
} from "../types";

function findMatchingLogLine(lines: string[], patterns: string[]): string | null {
  for (let index = lines.length - 1; index >= 0; index -= 1) {
    const line = lines[index];
    const normalized = line.toLowerCase();
    if (patterns.some((pattern) => normalized.includes(pattern))) {
      return line;
    }
  }
  return null;
}

export function buildMockRuntimeHealth(
  details: InstanceDetails,
  logDocument: LogTailSnapshot
): RuntimeHealthSummary {
  const status = String(details.summary.status).toLowerCase();
  const moduleId = String(details.summary.module_id).toLowerCase();
  const lines = logDocument.lines;
  const readyPatterns = ["server ready", "ready for connections", "listening on udp", "listening on tcp", "heartbeat ok", "server startup complete"];
  const warningPatterns = ["panic:", "retry!", "we will retry", "an error occured", "timeout while"];
  const errorPatterns = ["fatal", "unhandled exception", "segmentation fault", "access violation", "stack traceback", "assertion failed", "moderror"];
  const dstLuaWarningPatterns = ["failed to load modoverrides.lua", "failed to load ../worldgenoverride.lua"];
  const dstReadyPatterns = ["lan server started on port"];
  const abioticErrorPatterns = [
    "world save integrity state: corrupt",
    "will shut down in 5 minutes due to world save corruption"
  ];
  const abioticReadyPatterns = ["session short code:"];
  const abioticLoadingPatterns = ["listening on port", "load map complete", "dedicated server is now loading the main map"];
  const abioticStartingPatterns = [
    "checking world save for corruption",
    "world save:",
    "could not find any files for the save, this is fine if it's a new save"
  ];

  if (status === "stopped") {
    return {
      status: "stopped",
      summary: "Server is currently stopped.",
      reason: { code: "stopped", params: {} },
      matched_line: null
    };
  }

  if (status !== "running") {
    return {
      status: "idle",
      summary: "No runtime health summary yet.",
      reason: { code: "not_available", params: {} },
      matched_line: null
    };
  }

  if (moduleId === "abioticfactor") {
    const abioticErrorLine = findMatchingLogLine(lines, abioticErrorPatterns);
    if (abioticErrorLine) {
      return {
        status: "error",
        summary: "Abiotic Factor detected world save corruption and the dedicated server is not safe to keep online.",
        reason: { code: "abiotic_world_corrupt", params: {} },
        matched_line: abioticErrorLine
      };
    }

    const abioticReadyLine = findMatchingLogLine(lines, abioticReadyPatterns);
    if (abioticReadyLine) {
      return {
        status: "ready",
        summary: "Abiotic Factor is online and has published a session short code for players to join.",
        reason: { code: "abiotic_session_published", params: {} },
        matched_line: abioticReadyLine
      };
    }

    const abioticLoadingLine = findMatchingLogLine(lines, abioticLoadingPatterns);
    if (abioticLoadingLine) {
      const loadingMap = abioticLoadingLine.toLowerCase().includes("dedicated server is now loading the main map");
      return {
        status: "starting",
        summary: loadingMap
          ? "Abiotic Factor is through world validation and is loading the main facility map."
          : "Abiotic Factor is listening and preparing its game session.",
        reason: { code: loadingMap ? "abiotic_loading_map" : "abiotic_listening", params: {} },
        matched_line: abioticLoadingLine
      };
    }

    const abioticStartingLine = findMatchingLogLine(lines, abioticStartingPatterns);
    if (abioticStartingLine) {
      return {
        status: "starting",
        summary: "Abiotic Factor is validating the selected world save and preparing the map.",
        reason: { code: "abiotic_validating_world", params: {} },
        matched_line: abioticStartingLine
      };
    }
  }

  const errorLine = findMatchingLogLine(lines, errorPatterns);
  if (errorLine) {
    return {
      status: "error",
      summary: "A fatal error pattern was detected in the recent log.",
      reason: { code: "fatal_log_pattern", params: {} },
      matched_line: errorLine
    };
  }

  if (moduleId === "dontstarve") {
    const dstWarningLine = findMatchingLogLine(lines, dstLuaWarningPatterns);
    if (dstWarningLine) {
      return {
        status: "warning",
        summary: "DST is running, but shard Lua config files failed to load. Save the instance once to rewrite the cluster files.",
        reason: { code: "dst_lua_config_failed", params: {} },
        matched_line: dstWarningLine
      };
    }
  }

  const warningLine = findMatchingLogLine(lines, warningPatterns);
  if (warningLine) {
    return {
      status: "warning",
      summary: "The server is running, but the recent log contains a warning or retry signal.",
      reason: { code: "retry_signal", params: {} },
      matched_line: warningLine
    };
  }

  const readyLine = moduleId === "dontstarve"
    ? findMatchingLogLine(lines, dstReadyPatterns) ?? findMatchingLogLine(lines, readyPatterns)
    : findMatchingLogLine(lines, readyPatterns);
  if (readyLine) {
    return {
      status: "ready",
      summary: "The server reported a ready or listening signal.",
      reason: { code: "ready_signal", params: {} },
      matched_line: readyLine
    };
  }

  return {
    status: "starting",
    summary: lines.length === 0 ? "The process is running and waiting for startup log output." : "The process is running and still working through startup tasks.",
    reason: { code: lines.length === 0 ? "starting_waiting_logs" : "starting_tasks", params: {} },
    matched_line: null
  };
}

export function buildMockRuntimeDiagnostics(
  details: InstanceDetails,
  logDocument: LogTailSnapshot,
  health: RuntimeHealthSummary
): RuntimeDiagnosticSignal[] {
  const moduleId = String(details.summary.module_id).toLowerCase();
  const lines = logDocument.lines;
  if (moduleId !== "abioticfactor") {
    return [];
  }

  const diagnostics: RuntimeDiagnosticSignal[] = [];
  const remoteConsoleLine = findMatchingLogLine(lines, ["remote console through https is explicitly disabled"]);
  if (remoteConsoleLine) {
    diagnostics.push({
      code: "abiotic_remote_console_https_disabled",
      severity: "info",
      summary:
        "Abiotic Factor has the built-in HTTPS remote console disabled. That is expected for the current desktop-managed host unless you intentionally plan to expose the game's own remote console.",
      matched_line: remoteConsoleLine,
      actionable: false
    });
  }

  const eosInterfaceLine = findMatchingLogLine(lines, [
    "baseuserinterface delegates not bound. base interface not valid",
    "basestoreinterface delegates not bound. base interface not valid",
    "basepurchaseinterface delegates not bound. base interface not valid",
    "baseexternaluiinterface delegates not bound. base interface not valid",
    "basevoiceinterface delegates not bound. base interface not valid",
    "baseusercloudinterface delegates not bound. base interface not valid",
    "unable to call method in base interface. base interface not valid"
  ]);
  if (eosInterfaceLine) {
    diagnostics.push({
      code: "abiotic_headless_eos_interfaces_unavailable",
      severity: "info",
      summary:
        "Abiotic Factor is reporting missing EOS local UI or voice interfaces. This is expected on a headless dedicated server and is not a join blocker by itself.",
      matched_line: eosInterfaceLine,
      actionable: false
    });
  }

  const sessionWarningLine = findMatchingLogLine(lines, [
    "can't start an online game for session (gamesession) that hasn't been created"
  ]);
  if (sessionWarningLine) {
    const joinCodePublished = String(health.matched_line ?? "").toLowerCase().includes("session short code:")
      || Boolean(findMatchingLogLine(lines, ["session short code:"]));
    diagnostics.push({
      code: "abiotic_online_session_start_warning",
      severity: joinCodePublished ? "info" : "warning",
      summary: joinCodePublished
        ? "Abiotic Factor emitted an online-session startup warning after the room had already published a session short code. Treat this as engine noise unless players are still unable to join."
        : "Abiotic Factor reported an online-session startup failure before the room finished publishing a join code. Check the latest log lines if players cannot discover or join this server.",
      matched_line: sessionWarningLine,
      actionable: !joinCodePublished
    });
  }

  return diagnostics;
}
