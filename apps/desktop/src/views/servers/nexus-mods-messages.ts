import { NexusApiError } from "../../nexus-mods-api";

const copy = {
  manualImportHelp: ["从 Nexus Mods 官网下载适用于此服务器的文件，然后在“商店”中导入。请按作者说明准备加载器和依赖。", "Download server-compatible files from the Nexus Mods website, then import them in Store. Follow the author's loader and dependency requirements."],
  nexus_api_unavailable: ["此版本不提供 Nexus API 下载。请从 Nexus Mods 官网下载后导入本地文件。", "Nexus API downloads are not available in this release. Download files from the Nexus Mods website and import them locally."],
  auth: ["Nexus 授权", "Nexus authorization"], key: ["API Key", "API Key"],
  authTitle: ["Nexus Mods 授权", "Nexus Mods authorization"],
  authHelp: ["个人 API Key 保存在这台电脑的 Windows 凭据存储中，供此应用的所有 Nexus 来源共用。不会写入服务器配置。", "Your personal API key is kept in this computer's Windows credential store and shared by this app's Nexus sources. It is never written to server settings."],
  reading: ["正在读取授权状态…", "Reading authorization…"],
  statusUnknown: ["未能读取授权状态。可以重试，或移除本机已保存的授权。", "Authorization status could not be read. Retry or remove the authorization saved on this computer."],
  openFailed: ["无法打开系统浏览器，请检查本机默认浏览器设置后重试。", "The system browser could not be opened. Check this computer's default browser and retry."],
  configured: ["已保存 API Key", "API key saved"], missing: ["尚未配置 API Key", "No API key configured"],
  premium: ["Premium 账户", "Premium account"], standard: ["普通账户；部分文件需要 Premium 或官网 nxm 授权，以 Nexus 返回为准。", "Standard account; some files require Premium or an official nxm authorization link, as determined by Nexus."],
  keyHelp: ["填写新的个人 API Key。已保存的 Key 不会回显。", "Enter a new personal API key. Saved keys are never shown."],
  application: ["前往 Nexus 获取个人 API Key", "Get your personal API key from Nexus"],
  save: ["验证并保存", "Verify and save"], clear: ["移除授权", "Remove authorization"], close: ["关闭", "Close"],
  cancel: ["取消", "Cancel"], retry: ["重试", "Retry"], busy: ["处理中…", "Working…"],
  saved: ["授权已保存，可以继续安装。", "Authorization saved. You can continue installing."],
  cleared: ["已移除本机 Nexus 授权。", "Nexus authorization removed from this computer."],
  pickerTitle: ["选择 Nexus 安装文件", "Select Nexus files to install"],
  pickerHelp: ["确认所选文件适用于此游戏的专用服务器，并按作者说明准备加载器和依赖。应用不会自动判断服务器兼容性。", "Confirm that each selected file supports this game's dedicated server and follow the author's loader and dependency requirements. The app does not infer server compatibility."],
  noFiles: ["该项目没有可供选择的文件。请查看作者页面。", "This project has no files to select. Check the author's page."],
  unavailable: ["此文件当前无法安装。", "This file cannot currently be installed."],
  install: ["安装所选文件", "Install selected files"], projectPage: ["作者页面", "Mod page"],
  selectRequired: ["请为每个项目明确选择一个可安装文件。", "Choose an installable file for each project."],
  nexus_auth_required: ["请先在“Nexus 授权”中保存个人 API Key，然后重试安装。", "Save your personal API key under Nexus authorization, then retry the installation."],
  nexus_auth_invalid: ["API Key 无效或已失效，请检查后重试。", "The API key is invalid or expired. Check it and retry."],
  nexus_premium_required: ["此下载需要 Premium 账户或 Nexus 官方提供的有效签名下载链接。", "This download needs a Premium account or a valid signed download link supplied by Nexus."],
  nexus_rate_limited: ["Nexus 请求次数已达限制，请稍后重试。", "Nexus has limited requests. Retry later."],
  nexus_game_mismatch: ["该链接属于其它游戏，请使用当前游戏的 Nexus ID 或链接。", "This link belongs to another game. Use a Nexus ID or link for the current game."],
  nexus_reference_invalid: ["无法识别此 Nexus ID 或链接，请检查后重试。", "This Nexus ID or link could not be recognized. Check it and retry."],
  nexus_download_expired: ["签名下载链接已失效，请在 Nexus 重新获取链接后重试。", "The signed download link has expired. Get a new link from Nexus and retry."],
  nexus_file_unavailable: ["所选文件已不可用，请重新读取项目并选择文件。", "The selected file is unavailable. Review the project again and select a file."],
  nexus_network_error: ["无法连接 Nexus，请检查网络后重试。", "Nexus could not be reached. Check your connection and retry."],
  nexus_local_desktop_required: ["请在服务器所在电脑的 LanGame 桌面应用中配置 Nexus 授权并安装。局域网管理页不接收 API Key 或执行 Nexus 下载。", "Configure Nexus authorization and install from LanGame's desktop app on the server computer. The LAN management page does not accept API keys or perform Nexus downloads."],
  nexus_invalid_response: ["Nexus 返回的数据格式不受支持，请更新应用后重试。", "The Nexus response is not supported. Update the app and retry."],
  nexus_operation_failed: ["操作未完成。请确认授权、网络和安装文件符合要求后重试。", "The operation did not complete. Check authorization, connectivity and the installation files, then retry."],
  nexus_installation_failed: ["安装未完成", "Installation did not complete"],
} as const;
export type NexusCopyKey = keyof typeof copy;
export function nexusText(locale: string, key: NexusCopyKey): string { return copy[key][locale === "zh-CN" ? 0 : 1]; }
export function nexusErrorText(locale: string, error: unknown): string {
  if (error instanceof NexusApiError && error.code === "nexus_installation_failed" && error.safeMessage) return `${nexusText(locale, error.code)}: ${error.safeMessage}`;
  return nexusText(locale, error instanceof NexusApiError ? error.code : "nexus_operation_failed");
}
