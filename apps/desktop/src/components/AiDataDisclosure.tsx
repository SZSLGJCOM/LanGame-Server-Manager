import type { AiSettings } from "../ai-settings";
import { describeAiDataRecipient } from "../ai-data-recipient";
import { isChineseLocale, useI18n } from "../i18n";
import "./privacy-disclosure.css";

export function AiDataDisclosure({ settings }: {
  settings: Pick<AiSettings, "baseUrl">;
}) {
  const { locale } = useI18n();
  const chinese = isChineseLocale(locale);
  const recipient = describeAiDataRecipient(settings.baseUrl);
  return <div className="ai-data-disclosure">
    <p className="ai-data-recipient">
      <span>{chinese ? "AI 接收地址：" : "AI recipient: "}</span>
      {recipient ? <><bdi>{recipient.origin}</bdi>{recipient.loopback
        ? <span>{chinese ? "（管理端本机）" : " (management host)"}</span> : null}
        {!recipient.encrypted ? <span>{chinese ? " · HTTP 未加密" : " · HTTP is unencrypted"}</span> : null}</>
        : <span>{chinese ? "未设置有效的 HTTP(S) 地址" : "No valid HTTP(S) address configured"}</span>}
    </p>
    <p>{chinese ? "发送范围：对话，以及相关主机与实例信息、配置、扩展、日志和知识库片段；可能含个人信息。"
      : "Sends conversation and relevant host/instance details, configuration, extensions, logs and knowledge excerpts; these may contain personal data."}</p>
    <p>{chinese ? "OpenAI 兼容、Anthropic 兼容和 Ollama 表示接口协议；接收方由服务地址决定。"
      : "OpenAI-compatible, Anthropic-compatible and Ollama describe API protocols. The service URL determines the recipient."}</p>
  </div>;
}
