import { isChineseLocale } from "../i18n";

export function appUpdateCopy(locale: string) {
  return isChineseLocale(locale) ? {
    close: "关闭更新窗口", dismiss: "关闭",
    idle: "检查是否有可用更新", current: "当前已是最新版本", checking: "正在检查更新",
    available: "发现新版本", downloading: "正在下载更新", installing: "正在安装更新",
    failed: "更新未完成", retry: "重新检查",
    download: "前往下载", install: "在线更新", later: "稍后", openingDownload: "正在打开…",
    downloadFailed: "无法打开下载页面。", releaseUnavailable: "此版本的官方下载页面不可用。",
    installImpact: "在线更新：下载后请求保存并停止运行中的服务器，安装并重启 LanGame。",
    downloadingDetail: "下载完成后将请求保存并停止服务器，然后安装更新。",
    installingDetail: "正在停止运行服务并安装更新，完成后 LanGame 将自动重启。",
    failedDetail: "可以重新检查后重试。"
  } : {
    close: "Close updates", dismiss: "Close",
    idle: "Check for available updates", current: "You are up to date", checking: "Checking for updates",
    available: "Update available", downloading: "Downloading update", installing: "Installing update",
    failed: "Update did not complete", retry: "Check again",
    download: "Download installer", install: "Update now", later: "Later", openingDownload: "Opening…",
    downloadFailed: "Could not open the download page.", releaseUnavailable: "The official download page is unavailable for this version.",
    installImpact: "Online update downloads first, requests server saves, stops running servers, then installs and restarts LanGame.",
    downloadingDetail: "Once downloaded, LanGame will request saves and stop servers before installing.",
    installingDetail: "Stopping the runtime service and installing. LanGame will restart when installation completes.",
    failedDetail: "Check again to retry."
  };
}
