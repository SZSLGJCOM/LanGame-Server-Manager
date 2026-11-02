import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { ShellIcon } from "../../components/ShellIcon";
import { useI18n } from "../../i18n";
import { GmToolsNavigation } from "./GmToolsNavigation";
import type { GmToolDefinition } from "./gm-tools";

interface Props {
  instanceId: string; moduleId: string; tools: readonly GmToolDefinition[]; selectedId: string;
  onSelect: (id: string) => void; map: boolean; children: ReactNode;
}

/** Shared left navigation; changing panels keeps the map and its command owners mounted. */
export function GmToolsWorkspace({ instanceId, moduleId, tools, selectedId, onSelect, map, children }: Props) {
  const { t } = useI18n();
  const navigationId = useId();
  const navigationToggle = useRef<HTMLButtonElement>(null);
  const content = useRef<HTMLDivElement>(null);
  const [navigationOpen, setNavigationOpen] = useState(false);
  useEffect(() => setNavigationOpen(false), [instanceId, moduleId, selectedId]);
  return <section className={`configuration-workspace gmt-workbench${map ? " gmt-workbench--map" : ""}`}>
    <div className="configuration-workspace__body" data-navigation-open={navigationOpen || undefined}
      onKeyDown={event => {
        if (event.key === "Escape" && !event.nativeEvent.isComposing && navigationOpen) {
          event.stopPropagation(); setNavigationOpen(false); navigationToggle.current?.focus();
        }
      }}>
      <button type="button" ref={navigationToggle} className="configuration-workspace__navigation-toggle"
        aria-expanded={navigationOpen} aria-controls={navigationId} onClick={() => setNavigationOpen(open => !open)}>
        <ShellIcon name="list" aria-hidden="true" />{t("servers.gmTools.actions", undefined, "GM actions")}
        <ShellIcon name="chevron-right" aria-hidden="true" />
      </button>
      <aside id={navigationId} className="configuration-workspace__sidebar gmt-tools-sidebar"
        aria-label={t("servers.gmTools.actions", undefined, "GM actions")}>
        <GmToolsNavigation moduleId={moduleId} tools={tools} selectedId={selectedId} onSelect={id => {
          onSelect(id); setNavigationOpen(false); content.current?.scrollTo({ top: 0 });
          if (navigationToggle.current?.getClientRects().length) navigationToggle.current.focus();
        }} />
      </aside>
      <div ref={content} className="configuration-workspace__main" data-configuration-scroll-owner>
        <div className="configuration-workspace__content">{children}</div>
      </div>
    </div>
  </section>;
}
