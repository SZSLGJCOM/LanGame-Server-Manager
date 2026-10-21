import { ActivityNotice } from "../../components/ActivityNotice";
import { selectLocaleText, useI18n } from "../../i18n";
import { gmExecutionText, type useGmToolExecution } from "./gm-tool-execution";

type GmExecutionResult = NonNullable<ReturnType<typeof useGmToolExecution>["result"]>;

/** Delivery summary followed by each command's server response. */
export function gmExecutionLines(locale: string, result: GmExecutionResult): string[] {
  const text = (zh: string, en: string) => selectLocaleText(locale, zh, en);
  return [gmExecutionText(locale, result), ...result.responses.map(response => [response.command,
    response.write_confirmation_pending ? text("等待写入确认", "Awaiting write confirmation") : text("已发送", "Sent"),
    response.response_text || text("服务器没有返回响应文本，请查看运行日志或游戏内结果。",
      "The server returned no response text. Check the runtime log or in-game result.")
  ].join(" · "))];
}

export function GmExecutionNotice({ result }: { result: GmExecutionResult }) {
  const { locale } = useI18n();
  return <ActivityNotice tone={result.error ? "error" : "info"}>{gmExecutionLines(locale, result).join("\n")}</ActivityNotice>;
}
