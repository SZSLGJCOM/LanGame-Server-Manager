import type { UpdateInstanceInput } from "../../types";
import { normalizeConfigurationSaveError } from "./configuration-save-error";

export interface InstanceSettingsSaveRequest {
  input: UpdateInstanceInput;
  signature: string;
}

export type InstanceSettingsSaveStatus =
  | { state: "saved" }
  | { state: "dirty" }
  | { state: "saving" }
  | { state: "failed" | "conflict"; message: string };

export type InstanceSettingsSaveExecutor = (
  input: UpdateInstanceInput,
  expectedSettingsJson: string
) => string | Promise<string>;

export class InstanceSettingsDraftInvalidError extends Error {
  readonly code = "instance_settings_draft_invalid";

  constructor() {
    super("The current settings draft is invalid. Correct its validation errors before starting the server.");
    this.name = "InstanceSettingsDraftInvalidError";
  }
}

interface InstanceSettingsSaveQueueOptions {
  instanceId: string;
  settingsBaseline: string;
  savedSignature: string;
  execute: InstanceSettingsSaveExecutor;
  onStatusChange?: (status: InstanceSettingsSaveStatus) => void;
}

export class InstanceSettingsSaveQueue {
  private instanceId: string;
  private settingsBaseline: string;
  private savedSignature: string;
  private desiredSignature: string;
  private readonly execute: InstanceSettingsSaveExecutor;
  private readonly onStatusChange?: (status: InstanceSettingsSaveStatus) => void;
  private pending: InstanceSettingsSaveRequest | null = null;
  private failedRequest: InstanceSettingsSaveRequest | null = null;
  private activeSignature: string | null = null;
  private running = false;
  private closing = false;
  private disposed = false;
  private emitStatusChanges = true;
  private generation = 0;
  private status: InstanceSettingsSaveStatus = { state: "saved" };
  private readonly idleWaiters = new Set<{ resolve(): void; reject(error: Error): void }>();

  constructor(options: InstanceSettingsSaveQueueOptions) {
    this.instanceId = options.instanceId;
    this.settingsBaseline = options.settingsBaseline;
    this.savedSignature = options.savedSignature;
    this.desiredSignature = options.savedSignature;
    this.execute = options.execute;
    this.onStatusChange = options.onStatusChange;
  }

  reset(instanceId: string, settingsBaseline: string, savedSignature: string) {
    this.rejectIdleWaiters(new Error("Settings save was reset before persistence completed."));
    this.generation += 1;
    this.instanceId = instanceId;
    this.settingsBaseline = settingsBaseline;
    this.savedSignature = savedSignature;
    this.desiredSignature = savedSignature;
    this.pending = null;
    this.failedRequest = null;
    this.activeSignature = null;
    this.running = false;
    this.closing = false;
    this.disposed = false;
    this.emitStatusChanges = true;
    this.setStatus({ state: "saved" }, true);
  }

  dispose() {
    this.rejectIdleWaiters(new Error("Settings save was disposed before persistence completed."));
    this.generation += 1;
    this.pending = null;
    this.failedRequest = null;
    this.activeSignature = null;
    this.running = false;
    this.closing = false;
    this.disposed = true;
  }

  flushAndDispose(request: InstanceSettingsSaveRequest | null) {
    if (this.disposed || this.closing) {
      return;
    }

    this.closing = true;
    this.emitStatusChanges = false;
    this.pending = null;
    this.failedRequest = null;
    if (request?.input.id === this.instanceId) {
      this.desiredSignature = request.signature;
      if (this.activeSignature === request.signature) {
        this.pending = null;
      } else if (!this.running && request.signature === this.savedSignature) {
        this.pending = null;
      } else {
        this.pending = request;
      }
    }

    this.flush();
    this.finishClosingIfIdle();
  }

  isSaved(signature: string): boolean {
    return !this.disposed
      && !this.running
      && !this.pending
      && signature === this.savedSignature;
  }

  async flushLatest(request: InstanceSettingsSaveRequest | null): Promise<void> {
    if (!request) {
      throw new InstanceSettingsDraftInvalidError();
    }
    if (request.input.id !== this.instanceId || this.disposed || this.closing) {
      throw new Error("The settings editor changed before its draft could be saved.");
    }
    if (this.markDirty(request.signature)) {
      this.enqueue(request);
    }
    await this.whenIdle();
  }

  whenIdle(): Promise<void> {
    if (!this.running && !this.pending) {
      const error = this.idleError();
      return error ? Promise.reject(error) : Promise.resolve();
    }
    return new Promise((resolve, reject) => {
      this.idleWaiters.add({ resolve, reject });
    });
  }

  markDirty(signature: string): boolean {
    if (this.disposed || this.closing) {
      return false;
    }
    this.desiredSignature = signature;
    // A queued draft must not outlive the edit that replaced it, including invalid edits.
    if (this.pending?.signature !== signature) {
      this.pending = null;
    }
    if (this.failedRequest?.signature === signature) {
      return false;
    }
    if (this.failedRequest) {
      this.failedRequest = null;
    }
    if (this.isSaved(signature)) {
      this.setStatus({ state: "saved" });
      return false;
    }
    if (this.activeSignature === signature && !this.pending) {
      this.setStatus({ state: "saving" });
      return false;
    }
    if (this.pending?.signature === signature) {
      this.setStatus({ state: "dirty" });
      return false;
    }
    this.setStatus({ state: "dirty" });
    return true;
  }

  enqueue(request: InstanceSettingsSaveRequest) {
    if (this.disposed || this.closing || request.input.id !== this.instanceId) {
      return;
    }
    this.desiredSignature = request.signature;
    this.failedRequest = null;
    this.pending = request;
    this.setStatus({ state: "dirty" });
    this.flush();
  }

  retry() {
    if (this.disposed || this.closing || !this.failedRequest) {
      return;
    }
    const request = this.failedRequest;
    this.failedRequest = null;
    this.desiredSignature = request.signature;
    this.pending = request;
    this.setStatus({ state: "dirty" });
    this.flush();
  }

  private flush() {
    if (this.disposed || this.running || !this.pending) {
      return;
    }

    const pending = this.pending;
    this.pending = null;
    if (pending.signature === this.savedSignature) {
      this.finishIdleStatus();
      this.finishClosingIfIdle();
      return;
    }

    const generation = this.generation;
    const instanceId = this.instanceId;
    const expectedSettingsJson = this.settingsBaseline;
    this.running = true;
    this.activeSignature = pending.signature;
    this.setStatus({ state: "saving" });

    Promise.resolve()
      .then(() => {
        if (!this.isCurrent(generation, instanceId)) {
          return;
        }
        return this.execute(pending.input, expectedSettingsJson);
      })
      .then((savedSettingsJson) => {
        if (!this.isCurrent(generation, instanceId)) {
          return;
        }
        if (typeof savedSettingsJson !== "string") {
          throw new Error("Saved settings were not returned by the server.");
        }
        this.savedSignature = pending.signature;
        // Storage may restore defaults or normalize fields before committing.
        this.settingsBaseline = savedSettingsJson;
      })
      .catch((error: unknown) => {
        if (!this.isCurrent(generation, instanceId)) {
          return;
        }
        this.failedRequest = pending;
        if (!this.pending && this.desiredSignature === pending.signature) {
          this.setStatus(normalizeConfigurationSaveError(error));
        }
      })
      .finally(() => {
        if (!this.isCurrent(generation, instanceId)) {
          return;
        }
        this.running = false;
        this.activeSignature = null;
        if (this.pending) {
          this.failedRequest = null;
          this.flush();
          return;
        }
        if (this.failedRequest && this.desiredSignature === this.failedRequest.signature) {
          this.finishClosingIfIdle();
          return;
        }
        this.failedRequest = null;
        this.finishIdleStatus();
        this.finishClosingIfIdle();
      });
  }

  private finishIdleStatus() {
    this.setStatus(
      this.desiredSignature === this.savedSignature
        ? { state: "saved" }
        : { state: "dirty" }
    );
  }

  private setStatus(status: InstanceSettingsSaveStatus, force = false) {
    if (!force && this.status.state === status.state) {
      if (!("message" in status) || ("message" in this.status && this.status.message === status.message)) {
        return;
      }
    }
    this.status = status;
    if (this.emitStatusChanges) {
      this.onStatusChange?.(status);
    }
  }

  private finishClosingIfIdle() {
    if (this.running || this.pending) {
      return;
    }
    const error = this.idleError();
    for (const waiter of this.idleWaiters) {
      if (error) waiter.reject(error);
      else waiter.resolve();
    }
    this.idleWaiters.clear();
    if (!this.closing) return;
    this.generation += 1;
    this.failedRequest = null;
    this.activeSignature = null;
    this.disposed = true;
  }

  private idleError(): Error | null {
    if (this.status.state === "failed" || this.status.state === "conflict") {
      return new Error(this.status.message);
    }
    return this.desiredSignature === this.savedSignature
      ? null
      : new Error("The current settings draft has not been saved.");
  }

  private rejectIdleWaiters(error: Error) {
    for (const waiter of this.idleWaiters) waiter.reject(error);
    this.idleWaiters.clear();
  }

  private isCurrent(generation: number, instanceId: string): boolean {
    return !this.disposed && generation === this.generation && instanceId === this.instanceId;
  }
}
