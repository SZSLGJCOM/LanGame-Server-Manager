import { ActivityNotice } from "../../components/ActivityNotice";
import { gmExecutionLines } from "./GmExecutionNotice";
import { useI18n } from "../../i18n";
import type { DstGmCommand } from "./use-dst-gm-command";

/** One compact line for the rail; the complete delivery report stays in its tooltip. */
export function DstGmCommandStatus({ command }: { command: DstGmCommand }) {
  const { locale } = useI18n();
  if (command.issue) return <ActivityNotice tone="warning" onDismiss={command.dismissIssue}>{command.issue}</ActivityNotice>;
  if (!command.result) return null;
  const lines = gmExecutionLines(locale, command.result);
  return <p className={`dst-gm-result${command.result.error ? " is-error" : ""}`} role={command.result.error ? "alert" : "status"} title={lines.join("\n")}>
    {lines.length > 1 ? `${lines[0]} · ${lines[lines.length - 1]}` : lines[0]}
  </p>;
}
