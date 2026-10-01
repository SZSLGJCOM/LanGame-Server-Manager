import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export interface DesktopExitStatus {
  requested: boolean;
}

interface DesktopExitHost {
  enabled: boolean;
  readStatus: () => Promise<unknown>;
  listen: (onExit: (payload: unknown) => void) => Promise<() => void>;
}

export const FINAL_EXIT_ERROR = "Application final exit is in progress";

/** The native host hands off server stopping; the UI only latches its exit request. */
export class DesktopExitLifecycle {
  private status: DesktopExitStatus = { requested: false };
  private readonly observers = new Set<() => void>();
  private pendingRead: Promise<void> | null = null;

  constructor(private readonly host: DesktopExitHost) {}

  get enabled() { return this.host.enabled; }
  getSnapshot = () => this.status;
  subscribe = (observer: () => void) => {
    this.observers.add(observer);
    return () => { this.observers.delete(observer); };
  };

  private observe(payload: unknown): void {
    if (!payload || typeof payload !== "object" || !("requested" in payload)
      || typeof payload.requested !== "boolean") {
      throw new Error("Invalid desktop exit status.");
    }
    // A stale status read or a repeated event must never resume desktop work.
    if (!payload.requested || this.status.requested) return;
    this.status = { requested: true };
    this.observers.forEach((observer) => observer());
  }

  listen = () => this.host.listen((payload) => this.observe(payload));

  refresh = (): Promise<void> => {
    if (!this.enabled || this.status.requested) return Promise.resolve();
    if (!this.pendingRead) {
      this.pendingRead = this.host.readStatus().then((payload) => this.observe(payload))
        .finally(() => { this.pendingRead = null; });
    }
    return this.pendingRead;
  };

  async observeOperationError(error: unknown): Promise<void> {
    const message = error instanceof Error ? error.message : error;
    if (typeof message !== "string" || (message !== FINAL_EXIT_ERROR
      && !message.endsWith(" cannot begin while application shutdown is in progress"))) return;
    // Runtime shutdown is also used by recoverable updates. Only the local final-exit
    // receipt may unmount the application; the operation's original error is retained.
    await this.refresh().catch(() => undefined);
  }
}

export const desktopExitLifecycle = new DesktopExitLifecycle({
  enabled: isTauri(),
  readStatus: () => invoke<unknown>("app_exit_status"),
  listen: (onExit) => listen<unknown>("app-exit-requested", (event) => onExit(event.payload))
});
