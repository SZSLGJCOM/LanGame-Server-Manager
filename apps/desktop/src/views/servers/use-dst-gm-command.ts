import { useState } from "react";
import { describeError } from "../../app-state";
import { sendInstanceGmCommand } from "../../api";
import { selectLocaleText, useI18n } from "../../i18n";
import { runtimeProcessKeyIsRunning } from "../../runtime-action-state";
import type { InstanceDetails } from "../../types";
import { buildGmToolCommand } from "./gm-tools";
import { useGmToolExecution } from "./gm-tool-execution";

export type DstGmCommand = ReturnType<typeof useDstGmCommand>;

/** One sequential DST GM channel shared by the map's tool panels, so only one console write is in flight. */
export function useDstGmCommand(details: InstanceDetails) {
  const { locale, t } = useI18n();
  const execution = useGmToolExecution(details.summary.id, sendInstanceGmCommand, describeError);
  const [issue, setIssue] = useState<string | null>(null);
  async function run(toolId: string, values: Record<string, string>) {
    if (execution.busy) return;
    const preview = buildGmToolCommand("dontstarve", toolId, values, t);
    const target = preview.processKey ?? "master";
    const problem = preview.error ?? (runtimeProcessKeyIsRunning(details.active_run, target) ? null
      : selectLocaleText(locale, `分片 ${target} 未运行，请先启动它。`, `Shard ${target} is not running. Start it first.`));
    setIssue(problem);
    if (!problem) await execution.run(toolId, preview);
  }
  return { run, busy: execution.busy, sendingToolId: execution.sendingToolId, result: execution.result, issue, dismissIssue: () => setIssue(null) };
}
