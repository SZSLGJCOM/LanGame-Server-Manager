import { useState, type ReactNode } from "react";
import { ActivityBar } from "../../src/components/ActivityBar";
import { ActivityNoticeTarget } from "../../src/components/ActivityNotice";
import { createActivityNoticeStore } from "../../src/components/activity-notice-store";

export function ConfigurationActivityHost({ children }: { children: ReactNode }) {
  const [store] = useState(createActivityNoticeStore);
  return <ActivityNoticeTarget.Provider value={{ element: null, store, dismissLabel: "Close" }}>
    <div style={{ height: "100dvh", display: "grid", gridTemplateRows: "minmax(0, 1fr) auto" }}>
      <div style={{ minHeight: 0, overflow: "hidden" }}>{children}</div>
      <footer className="shell-activity-bar"><ActivityBar store={store} entries={[]} /></footer>
    </div>
  </ActivityNoticeTarget.Provider>;
}
