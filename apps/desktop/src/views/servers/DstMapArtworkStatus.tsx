import type { DstMapArt } from "../../dst-world-map";
import { selectLocaleText, useI18n } from "../../i18n";

export function DstMapArtworkStatus({ art }: { art: DstMapArt | null }) {
  const { locale } = useI18n();
  const text = (zh: string, en: string) => selectLocaleText(locale, zh, en);
  if (!art) return null;
  return <>
    {art.iconFallbacks.length ? <details className="dst-map-art-issues dst-map-art-completions">
      <summary>{text(`图标补全（${art.iconFallbacks.length}）`, `Completed icons (${art.iconFallbacks.length})`)}</summary>
      <p>{text("以下图标由实体声明的物品素材或唯一匹配的已安装图集补全，可查看各自来源。",
        "These icons use entity-declared inventory artwork or unique installed atlas matches; each source is listed below.")}</p>
      <ul>{art.iconFallbacks.map(source => <li key={source.icon}>
        <code>{source.icon}</code>{source.element !== source.icon ? <> → <code>{source.element}</code></> : null}
        {" · "}{source.kind === "entity_inventory" ? text("实体声明的物品图标", "Entity-declared inventory icon")
          : source.kind === "registered_alias" ? text("已注册图集", "Registered atlas") : text("已安装图集", "Installed atlas")}
        {" · "}<code>{source.atlas}</code>
      </li>)}</ul>
    </details> : null}
    {art.issues.length ? <details className="dst-map-art-issues">
      <summary>{text(`游戏素材提示（${art.issues.length}）`, `Game artwork notices (${art.issues.length})`)}</summary>
      <p>{text("部分素材使用替代显示，地形坐标与玩家位置仍来自服务器。软件会在下次地图刷新时重新读取已安装的游戏与 Mod 素材。",
        "Some artwork uses a fallback. Terrain coordinates and player positions still come from the server. Installed game and Mod artwork will be checked again on the next map refresh.")}</p>
      <ul>{art.issues.map((issue, index) => <li key={index}>{issue}</li>)}</ul>
    </details> : null}
  </>;
}
