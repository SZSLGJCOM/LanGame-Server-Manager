import { useId, useMemo, useRef, useState, type ReactNode } from "react";
import { ShellIcon, type ShellIconName } from "../../components/ShellIcon";
import { useI18n } from "../../i18n";
import { ConfigurationSectionNavigation } from "../settings/ConfigurationSectionNavigation";
import type { ConfigurationSectionNode } from "../settings/settings-schema";

interface MaintenanceSection {
  id: string;
  title: string;
  icon: ShellIconName;
  content: ReactNode;
  persistentContent?: ReactNode;
}

interface Props {
  active: boolean;
  sections: MaintenanceSection[];
  broadcast: ReactNode;
}

export function MaintenanceWorkspace({ active, sections, broadcast }: Props) {
  const { t } = useI18n();
  const navigationId = useId();
  const navigationToggle = useRef<HTMLButtonElement>(null);
  const content = useRef<HTMLDivElement>(null);
  const [navigationOpen, setNavigationOpen] = useState(false);
  const [selectedId, setSelectedId] = useState("backups");
  const allSections: MaintenanceSection[] = [...sections, {
    id: "broadcast", title: t("servers.maintenance.broadcast"), icon: "message-square", content: broadcast
  }];
  const selectedSectionId = allSections.some((section) => section.id === selectedId) ? selectedId : "backups";
  const roots = useMemo<ConfigurationSectionNode[]>(() => [
    ...sections.map(({ id, title, icon }, order) => ({
      id, title, icon, order, breadcrumb: [title], items: [], actionable: true, children: []
    })),
    { id: "broadcast", title: t("servers.maintenance.broadcast"), icon: "message-square", order: sections.length,
      breadcrumb: [t("servers.maintenance.broadcast")], items: [], actionable: true, children: [] }
  ], [sections, t]);

  return <section className="configuration-workspace maintenance-workspace" hidden={!active}>
    <div className="configuration-workspace__body" data-navigation-open={navigationOpen || undefined}
      onKeyDown={(event) => {
        if (event.key === "Escape" && !event.nativeEvent.isComposing && navigationOpen) {
          event.stopPropagation();
          setNavigationOpen(false);
          navigationToggle.current?.focus();
        }
      }}>
      <button type="button" ref={navigationToggle} className="configuration-workspace__navigation-toggle"
        aria-expanded={navigationOpen} aria-controls={navigationId}
        onClick={() => setNavigationOpen((open) => !open)}>
        <ShellIcon name="list" aria-hidden="true" />
        {t("servers.maintenance.sections")}
        <ShellIcon name="chevron-right" aria-hidden="true" />
      </button>
      <aside id={navigationId} className="configuration-workspace__sidebar">
        <ConfigurationSectionNavigation roots={roots} selectedSectionId={selectedSectionId}
          ariaLabel={t("servers.tabs.maintenance")}
          onSelectSection={(id) => {
            setSelectedId(id);
            setNavigationOpen(false);
            content.current?.scrollTo({ top: 0 });
            if (navigationToggle.current?.getClientRects().length) navigationToggle.current.focus();
          }} />
      </aside>
      <div ref={content} className="configuration-workspace__main" data-configuration-scroll-owner>
        {allSections.map((section) => <div key={section.id}
          className="configuration-workspace__content maintenance-workspace__section settings-schema-section"
          data-maintenance-section={section.id} hidden={!active || selectedSectionId !== section.id}>
          {/* Keep immediate-save results across tabs without retaining expensive
              maintenance readers. The parent instance key owns this lifetime. */}
          {section.persistentContent}
          {active || section.id === "broadcast" ? section.content : null}
        </div>)}
      </div>
    </div>
  </section>;
}
