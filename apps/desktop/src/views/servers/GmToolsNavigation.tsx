import { useMemo } from "react";
import type { ShellIconName } from "../../components/ShellIcon";
import { selectLocaleText, useI18n } from "../../i18n";
import { ConfigurationSectionNavigation } from "../settings/ConfigurationSectionNavigation";
import type { ConfigurationSectionNode } from "../settings/settings-schema";
import type { GmToolDefinition } from "./gm-tools";
import type { DstGmPanel } from "./DstWorldMapWorkbench";

interface Props {
  moduleId: string;
  tools: readonly GmToolDefinition[];
  selectedId: string;
  onSelect: (id: string) => void;
}

function categoryIcon(category: string): ShellIconName {
  const lower = category.toLowerCase();
  if (lower === "players") return "users";
  if (lower === "world") return "sun";
  if (lower === "server") return "shield";
  if (lower === "items") return "database";
  return "zap";
}

export function GmToolsNavigation({ moduleId, tools, selectedId, onSelect }: Props) {
  const { locale, t } = useI18n();
  const groups = useMemo(() => {
    const result: { id: string; title: string; nodes: ConfigurationSectionNode[] }[] = [];
    if (moduleId === "dontstarve") {
      // The map is always the main surface; these entries only switch its right-hand tool panel.
      const title = selectLocaleText(locale, "GM 工具", "GM tools");
      const panels: { id: DstGmPanel; icon: ShellIconName; zh: string; en: string; zhHelp: string; enHelp: string }[] = [
        { id: "world", icon: "sun", zh: "世界工具", en: "World tools", zhHelp: "季节、降水、复活玩家与重新生成世界", enHelp: "Season, precipitation, revival and world regeneration" },
        { id: "items", icon: "package", zh: "生成物品", en: "Spawn items", zhHelp: "给地图上选中的玩家发放物品或生成生物", enHelp: "Give items or spawn creatures for the player selected on the map" }
      ];
      result.push({ id: "dst-gm-panels", title, nodes: panels.map((panel, order) => {
        const name = selectLocaleText(locale, panel.zh, panel.en);
        return { id: panel.id, title: name, icon: panel.icon, order, description: selectLocaleText(locale, panel.zhHelp, panel.enHelp),
          breadcrumb: [title, name], items: [], actionable: true, children: [] };
      }) });
      return result;
    }
    for (const tool of tools) {
      const id = `category:${tool.category}`;
      let group = result.find(value => value.id === id);
      if (!group) {
        const key = tool.category.replace(/[^A-Za-z0-9]+/g, "_").replace(/^_+|_+$/g, "").toLowerCase();
        group = { id, title: t(`servers.gmTools.categories.${key}`, undefined, tool.category), nodes: [] };
        result.push(group);
      }
      const title = t(`servers.gmTools.tools.${tool.id}.title`, undefined, tool.title);
      group.nodes.push({ id: tool.id, title, icon: categoryIcon(tool.category), order: group.nodes.length,
        description: t(`servers.gmTools.tools.${tool.id}.description`, undefined, tool.description),
        breadcrumb: [group.title, title], items: [], actionable: true, children: [] });
    }
    return result;
  }, [moduleId, tools, locale, t]);

  // Keep the catalog's existing category order and every action directly visible.
  // The shared navigation owns button styling, selection, keyboard access and help.
  return <>{groups.map(group => <section key={group.id} className="gmt-nav-group">
    <ConfigurationSectionNavigation roots={group.nodes} selectedSectionId={selectedId}
      ariaLabel={group.title} onSelectSection={onSelect} />
  </section>)}</>;
}
