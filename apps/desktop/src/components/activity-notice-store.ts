import type { ReactNode } from "react";

export interface ActivityNoticeEntry {
  id: string;
  text: string;
  tone: "error" | "warning" | "success" | "info";
  action?: ReactNode;
  dismiss: () => void;
}

// Only the display subscribes: publishing a notice must not rerender its owner
// and recursively publish fresh action elements on every render.
export function createActivityNoticeStore() {
  let entries: ActivityNoticeEntry[] = [];
  const listeners = new Set<() => void>();
  const emit = () => listeners.forEach((listener) => listener());
  return {
    getSnapshot: () => entries,
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => { listeners.delete(listener); };
    },
    publish(entry: ActivityNoticeEntry) {
      const index = entries.findIndex((item) => item.id === entry.id);
      entries = index < 0 ? [...entries, entry] : entries.map((item, i) => i === index ? entry : item);
      emit();
    },
    remove(id: string) {
      if (!entries.some((item) => item.id === id)) return;
      entries = entries.filter((item) => item.id !== id);
      emit();
    }
  };
}

export type ActivityNoticeStore = ReturnType<typeof createActivityNoticeStore>;
