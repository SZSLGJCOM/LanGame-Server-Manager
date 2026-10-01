import type { RuntimeLivePlayerSnapshot } from "../types";
import { isAuthoritativeLivePlayerSnapshot } from "./live-player-state";

export interface LivePlayerRefreshContext {
  instanceId: string;
  visible: boolean;
  snapshot: RuntimeLivePlayerSnapshot | null;
}

export interface LivePlayerRefreshControllerOptions<TimerHandle> {
  now: () => number;
  schedule: (callback: () => void, delayMs: number) => TimerHandle;
  cancel: (handle: TimerHandle) => void;
  refresh: (instanceId: string) => Promise<RuntimeLivePlayerSnapshot>;
  onSnapshot: (snapshot: RuntimeLivePlayerSnapshot) => void;
  onError?: (error: unknown, instanceId: string) => void;
}

type LivePlayerSnapshotRequest = (
  instanceId: string
) => Promise<RuntimeLivePlayerSnapshot>;

export class LivePlayerRefreshController<TimerHandle = unknown> {
  private readonly options: LivePlayerRefreshControllerOptions<TimerHandle>;
  private instanceId: string | null = null;
  private visible = false;
  private snapshot: RuntimeLivePlayerSnapshot | null = null;
  private generation = 0;
  private timerHandle: TimerHandle | null = null;
  private disposed = false;
  private recoveryAttempts = 0;

  constructor(options: LivePlayerRefreshControllerOptions<TimerHandle>) {
    this.options = options;
  }

  setContext(context: LivePlayerRefreshContext): void {
    if (this.disposed) {
      return;
    }

    const nextSnapshot = context.snapshot?.instance_id === context.instanceId
      ? context.snapshot
      : null;
    const changed = context.instanceId !== this.instanceId
      || context.visible !== this.visible
      || nextSnapshot !== this.snapshot;
    if (!changed) {
      return;
    }

    if (context.instanceId !== this.instanceId) {
      this.recoveryAttempts = 0;
    }
    this.generation += 1;
    this.cancelTimer();
    this.instanceId = context.instanceId;
    this.visible = context.visible;
    this.snapshot = nextSnapshot;
    this.scheduleFromCurrentSnapshot();
  }

  read(readSnapshot: LivePlayerSnapshotRequest): Promise<boolean> {
    this.recoveryAttempts = 0;
    return this.runRequest(readSnapshot);
  }

  refreshNow(): Promise<boolean> {
    this.recoveryAttempts = 0;
    return this.runRequest(this.options.refresh);
  }

  dispose(): void {
    if (this.disposed) {
      return;
    }
    this.disposed = true;
    this.visible = false;
    this.generation += 1;
    this.cancelTimer();
  }

  private async runRequest(request: LivePlayerSnapshotRequest): Promise<boolean> {
    if (this.disposed || !this.visible || !this.instanceId) {
      return false;
    }

    const instanceId = this.instanceId;
    const requestGeneration = this.generation + 1;
    this.generation = requestGeneration;
    this.cancelTimer();

    let nextSnapshot: RuntimeLivePlayerSnapshot;
    try {
      nextSnapshot = await request(instanceId);
    } catch (error) {
      if (this.requestIsCurrent(instanceId, requestGeneration)) {
        this.options.onError?.(error, instanceId);
        this.scheduleRecovery();
      }
      return false;
    }

    if (
      !this.requestIsCurrent(instanceId, requestGeneration)
      || nextSnapshot.instance_id !== instanceId
    ) {
      return false;
    }

    this.snapshot = nextSnapshot;
    this.options.onSnapshot(nextSnapshot);
    if (isAuthoritativeLivePlayerSnapshot(nextSnapshot, this.options.now())) {
      this.recoveryAttempts = 0;
      this.scheduleFromCurrentSnapshot();
    } else if (nextSnapshot.status === "failed" || nextSnapshot.status === "refreshing"
      || nextSnapshot.status === "ready") {
      this.scheduleRecovery();
    }
    return true;
  }

  private requestIsCurrent(instanceId: string, requestGeneration: number): boolean {
    return !this.disposed
      && this.visible
      && this.instanceId === instanceId
      && this.generation === requestGeneration;
  }

  private scheduleFromCurrentSnapshot(): void {
    const snapshot = this.snapshot;
    const now = this.options.now();
    if (
      this.disposed
      || !this.visible
      || !this.instanceId
      || !isAuthoritativeLivePlayerSnapshot(snapshot, now)
    ) {
      return;
    }

    const delayMs = snapshot.expires_at_unix_ms - now;
    this.scheduleRefresh(delayMs);
  }

  private scheduleRecovery(): void {
    // A transient failure must not stop updates permanently or create endless polling.
    if (this.recoveryAttempts >= 3 || this.snapshot?.status === "misconfigured"
      || this.snapshot?.status === "stopped" || this.snapshot?.status === "unsupported"
      || this.snapshot?.issue?.code === "authentication_failed") {
      return;
    }
    const delayMs = 5_000 * (2 ** this.recoveryAttempts);
    this.recoveryAttempts += 1;
    this.scheduleRefresh(delayMs);
  }

  private scheduleRefresh(delayMs: number): void {
    if (this.disposed || !this.visible || !this.instanceId) {
      return;
    }
    this.cancelTimer();
    const scheduledInstanceId = this.instanceId;
    const scheduledGeneration = this.generation;
    this.timerHandle = this.options.schedule(() => {
      if (
        this.disposed
        || !this.visible
        || this.instanceId !== scheduledInstanceId
        || this.generation !== scheduledGeneration
      ) {
        return;
      }
      this.timerHandle = null;
      void this.runRequest(this.options.refresh);
    }, delayMs);
  }

  private cancelTimer(): void {
    if (this.timerHandle === null) {
      return;
    }
    this.options.cancel(this.timerHandle);
    this.timerHandle = null;
  }
}
