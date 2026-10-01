import type { AssistantBuildInput, AssistantCapsuleModel, AssistantSeverity } from "./assistant-types";
import { buildAssistantLaunchIssues } from "./assistant-launch-diagnostics";

function highestSeverity(issueSeverities: AssistantSeverity[]): AssistantSeverity {
  if (issueSeverities.includes("critical")) {
    return "critical";
  }
  if (issueSeverities.includes("warning")) {
    return "warning";
  }
  return "info";
}

function collectIssueSeverities(input: AssistantBuildInput): AssistantSeverity[] {
  const severities: AssistantSeverity[] = [];

  if (!input.storageReady) {
    return severities;
  }

  const healthStatus = String(input.selectedRuntime?.health.status ?? "").toLowerCase();

  if (input.runtimeAutoRefreshPaused) {
    severities.push("warning");
  }
  if (healthStatus === "error") {
    severities.push("critical");
  } else if (healthStatus === "warning") {
    severities.push("warning");
  }
  severities.push(...buildAssistantLaunchIssues(input).map((issue) => issue.severity));
  if (input.activeView === "library" && input.steamCmdStatus && !input.steamCmdStatus.ready) {
    severities.push("warning");
  }
  if (input.activeJobsCount > 0) {
    severities.push("info");
  }

  return severities;
}

export function buildAssistantCapsuleModel(input: AssistantBuildInput): AssistantCapsuleModel {
  const issueSeverities = collectIssueSeverities(input);

  return {
    panelTitle: "LAN",
    tone: issueSeverities.length ? highestSeverity(issueSeverities) : "info"
  };
}
