import type { RuntimeLogStreamEvent } from "../../src/runtime-log-stream";

interface Registration {
  id: number;
  name: string;
  deliver: (payload: unknown) => void;
  complete: () => void;
  pending: boolean;
  released: boolean;
}

export const bridge = {
  hold: false,
  entries: [] as Registration[],
  active: new Set<number>(),
  released: 0,
  resolvePending() {
    for (const entry of this.entries) if (entry.pending) entry.complete();
  },
  emit(payload: RuntimeLogStreamEvent) {
    for (const entry of this.entries) if (entry.name === "runtime-log-stream" && this.active.has(entry.id)) entry.deliver(payload);
  },
  reset() {
    for (const entry of this.entries) if (entry.name === "runtime-service-events-reset" && this.active.has(entry.id)) entry.deliver({});
  }
};

// This is the sole module substituted by the browser test's Vite configuration.
// Real React, ReactDOM, application components and translation code are retained.
export function listen<T>(name: string, callback: (event: { payload: T }) => void): Promise<() => void> {
  if (!["runtime-log-stream", "runtime-service-events-reset"].includes(name)) throw new Error(`Unexpected native event: ${name}`);
  return new Promise((resolve) => {
    const entry: Registration = {
      id: bridge.entries.length,
      name,
      deliver: (payload) => callback({ payload: payload as T }),
      pending: true,
      released: false,
      complete: () => {
        if (!entry.pending) throw new Error("Registration completed twice");
        entry.pending = false;
        bridge.active.add(entry.id);
        resolve(() => {
          if (entry.released || !bridge.active.delete(entry.id)) throw new Error("Listener released twice");
          entry.released = true;
          bridge.released += 1;
        });
      }
    };
    bridge.entries.push(entry);
    if (!bridge.hold) entry.complete();
  });
}
