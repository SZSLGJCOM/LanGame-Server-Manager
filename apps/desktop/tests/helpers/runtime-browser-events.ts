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
  failNext: null as string | null,
  failures: 0,
  failNextRelease: null as string | null,
  releaseFailures: 0,
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

const supportedEvents = ["runtime-log-stream", "runtime-service-events-reset", "nexus-website-progress"];

export async function emit<T>(name: string, payload: T): Promise<void> {
  if (!supportedEvents.includes(name)) throw new Error(`Unexpected native event: ${name}`);
  for (const entry of bridge.entries) if (entry.name === name && bridge.active.has(entry.id)) entry.deliver(payload);
}

// This is the sole module substituted by the browser test's Vite configuration.
// Real React, ReactDOM, application components and translation code are retained.
export function listen<T>(name: string, callback: (event: { payload: T }) => void): Promise<() => void> {
  if (!supportedEvents.includes(name)) throw new Error(`Unexpected native event: ${name}`);
  if (bridge.failNext === name) {
    bridge.failNext = null;
    bridge.failures++;
    return Promise.reject(new Error("Synthetic native event listener unavailable"));
  }
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
          if (bridge.failNextRelease === entry.name) {
            bridge.failNextRelease = null;
            bridge.releaseFailures++;
            return Promise.reject(new Error("Synthetic asynchronous listener release failure"));
          }
        });
      }
    };
    bridge.entries.push(entry);
    if (!bridge.hold) entry.complete();
  });
}
