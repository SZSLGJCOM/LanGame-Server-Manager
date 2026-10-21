import { describeError } from "../../app-state";
import { ActivityNotice } from "../../components/ActivityNotice";
import { dstMapErrorCode } from "../../dst-map-errors";
import { selectLocaleText, useI18n } from "../../i18n";

export function DstMapCollectionStatus({ error }: { error: unknown }) {
  const { locale } = useI18n();
  if (!error) return null;
  const message = describeError(error);
  const code = dstMapErrorCode(message);
  const descriptions: Record<string, [string, string]> = {
    BUSY: ["地图正在采集中，稍后自动刷新。", "Map collection is busy. It will refresh automatically."],
    WORLD_NOT_READY: ["服务器的世界尚未就绪，地图会在世界可读取后显示。", "The server world is not ready yet. Its map will appear when it becomes readable."],
    RESPONSE_TIMEOUT: ["分片暂未返回完整地图。暂停或繁忙的分片恢复响应后会自动刷新。", "The shard has not returned a complete map. It will refresh when the paused or busy shard responds."],
    SHARD_NOT_RUNNING: ["所选分片未运行，当前保留上次采集结果。", "The selected shard is not running. Keeping its last collection."],
    SHARD_NOT_ENABLED: ["所选分片未启用，请切换到已启用的分片。", "The selected shard is not enabled. Choose an enabled shard."],
    SHARD_CHANGED: ["服务器运行已变化，等待状态刷新后重新采集。", "The server run changed. Collection will resume after its status updates."],
    CONSOLE_UNAVAILABLE: ["无法读取所选分片的控制台，请在控制台查看服务器状态。", "The shard console is unavailable. Check its server status in Console."],
    ARTWORK_WORKER_FAILED: ["地形素材处理未完成，将自动重试；上次地图仍可查看。", "Artwork processing did not complete and will retry. The previous map remains available."],
  };
  const description = code ? descriptions[code] : null;
  return <ActivityNotice tone={code === "BUSY" || code === "WORLD_NOT_READY" ? "info" : "warning"} details={message}>
    {description ? selectLocaleText(locale, ...description)
      : `${selectLocaleText(locale, "地图采集未完成，将自动重试。", "Map collection did not complete and will retry.")} ${message}`}
  </ActivityNotice>;
}
