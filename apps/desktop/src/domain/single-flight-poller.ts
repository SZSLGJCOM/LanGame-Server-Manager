export interface SingleFlightPollerOptions<T, TimerHandle> {
  intervalMs: number;
  poll: () => Promise<T>;
  schedule: (callback: () => void, delayMs: number) => TimerHandle;
  cancel: (handle: TimerHandle) => void;
  onValue: (value: T) => void;
  onError?: (error: unknown) => void;
}

export class SingleFlightPoller<T, TimerHandle = unknown> {
  private readonly options: SingleFlightPollerOptions<T, TimerHandle>;
  private timerHandle: TimerHandle | null = null;
  private epoch = 0;
  private inFlight = false;
  private started = false;
  private disposed = false;

  constructor(options: SingleFlightPollerOptions<T, TimerHandle>) {
    if (!Number.isFinite(options.intervalMs) || options.intervalMs < 0) {
      throw new RangeError("poll interval must be a non-negative finite number");
    }
    this.options = options;
  }

  start(): void {
    if (this.started || this.disposed) {
      return;
    }
    this.started = true;
    this.epoch += 1;
    void this.pollNow();
  }

  async pollNow(): Promise<boolean> {
    if (!this.started || this.disposed || this.inFlight) {
      return false;
    }

    const pollEpoch = this.epoch;
    this.cancelTimer();
    this.inFlight = true;

    try {
      const value = await this.options.poll();
      if (!this.isCurrent(pollEpoch)) {
        return false;
      }
      this.options.onValue(value);
      return true;
    } catch (error) {
      if (this.isCurrent(pollEpoch)) {
        this.options.onError?.(error);
      }
      return false;
    } finally {
      if (this.isCurrent(pollEpoch)) {
        this.inFlight = false;
        this.scheduleNext(pollEpoch);
      }
    }
  }

  dispose(): void {
    if (this.disposed) {
      return;
    }
    this.disposed = true;
    this.epoch += 1;
    this.inFlight = false;
    this.cancelTimer();
  }

  private isCurrent(pollEpoch: number): boolean {
    return this.started && !this.disposed && this.epoch === pollEpoch;
  }

  private scheduleNext(pollEpoch: number): void {
    this.timerHandle = this.options.schedule(() => {
      if (!this.isCurrent(pollEpoch)) {
        return;
      }
      this.timerHandle = null;
      void this.pollNow();
    }, this.options.intervalMs);
  }

  private cancelTimer(): void {
    if (this.timerHandle === null) {
      return;
    }
    this.options.cancel(this.timerHandle);
    this.timerHandle = null;
  }
}
