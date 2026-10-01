import type { InstanceBroadcastPolicy } from "../../types";

export interface BroadcastPolicySaveQueueOptions {
  instanceId: string;
  execute: (policy: InstanceBroadcastPolicy) => Promise<InstanceBroadcastPolicy>;
  onSaved: (policy: InstanceBroadcastPolicy) => void;
  onSavingChange: (saving: boolean) => void;
  onError: (error: unknown) => void;
}

export interface BroadcastPolicySaveQueueSlot {
  current: BroadcastPolicySaveQueue | null;
}

export class BroadcastPolicySaveQueue {
  private options: BroadcastPolicySaveQueueOptions;
  private pending: InstanceBroadcastPolicy | null = null;
  private running = false;
  private saving = false;
  private disposed = false;
  private emitCallbacks = true;
  private generation = 0;
  private attachmentSequence = 0;
  private activeAttachment = 0;
  private detachedIdleCallback: (() => void) | null = null;
  private readonly idleWaiters = new Set<() => void>();

  constructor(options: BroadcastPolicySaveQueueOptions) {
    this.options = options;
  }

  enqueue(policy: InstanceBroadcastPolicy): void {
    if (this.disposed || policy.instance_id !== this.options.instanceId) {
      return;
    }
    this.pending = policy;
    this.setSaving(true);
    this.flush();
  }

  waitUntilIdle(): Promise<void> {
    if (this.disposed || (!this.running && !this.pending)) {
      return Promise.resolve();
    }
    return new Promise((resolve) => {
      this.idleWaiters.add(resolve);
    });
  }

  dispose(): void {
    if (this.disposed) {
      return;
    }
    this.disposed = true;
    this.generation += 1;
    this.pending = null;
    this.running = false;
    this.emitCallbacks = false;
    this.detachedIdleCallback = null;
    this.resolveIdleWaiters();
  }

  attach(
    options: BroadcastPolicySaveQueueOptions,
    onDetachedIdle: () => void
  ): () => void {
    if (this.disposed || options.instanceId !== this.options.instanceId) {
      throw new Error("cannot attach a broadcast policy queue to a different instance");
    }

    const attachment = ++this.attachmentSequence;
    this.activeAttachment = attachment;
    this.options = options;
    this.emitCallbacks = true;
    this.detachedIdleCallback = null;
    if (this.saving) {
      this.options.onSavingChange(true);
    }

    return () => {
      if (this.activeAttachment !== attachment) {
        return;
      }
      this.activeAttachment = 0;
      this.emitCallbacks = false;
      this.detachedIdleCallback = onDetachedIdle;
      this.finishDetachedIfIdle();
    };
  }

  private flush(): void {
    if (this.disposed || this.running || !this.pending) {
      return;
    }

    const request = this.pending;
    const generation = this.generation;
    this.pending = null;
    this.running = true;

    Promise.resolve()
      .then(() => {
        if (!this.isCurrent(generation)) {
          return null;
        }
        return this.options.execute(request);
      })
      .then((saved) => {
        if (!saved || !this.isCurrent(generation) || this.pending) {
          return;
        }
        if (this.emitCallbacks) {
          this.options.onSaved(saved);
        }
      })
      .catch((error: unknown) => {
        if (this.isCurrent(generation) && !this.pending && this.emitCallbacks) {
          this.options.onError(error);
        }
      })
      .finally(() => {
        if (!this.isCurrent(generation)) {
          return;
        }
        this.running = false;
        if (this.pending) {
          this.flush();
          return;
        }
        this.setSaving(false);
        this.resolveIdleWaiters();
        this.finishDetachedIfIdle();
      });
  }

  private isCurrent(generation: number): boolean {
    return !this.disposed && this.generation === generation;
  }

  private setSaving(saving: boolean): void {
    if (this.saving === saving) {
      return;
    }
    this.saving = saving;
    if (this.emitCallbacks) {
      this.options.onSavingChange(saving);
    }
  }

  private finishDetachedIfIdle(): void {
    if (this.running || this.pending || !this.detachedIdleCallback) {
      return;
    }
    const callback = this.detachedIdleCallback;
    this.detachedIdleCallback = null;
    callback();
  }

  private resolveIdleWaiters(): void {
    if (this.running || this.pending) {
      return;
    }
    for (const resolve of this.idleWaiters) {
      resolve();
    }
    this.idleWaiters.clear();
  }
}

const broadcastPolicySaveQueues = new Map<string, BroadcastPolicySaveQueue>();

export function setupBroadcastPolicySaveQueue(
  slot: BroadcastPolicySaveQueueSlot,
  options: BroadcastPolicySaveQueueOptions
): () => void {
  let queue = broadcastPolicySaveQueues.get(options.instanceId);
  if (!queue) {
    queue = new BroadcastPolicySaveQueue(options);
    broadcastPolicySaveQueues.set(options.instanceId, queue);
  }
  const detach = queue.attach(options, () => {
    if (broadcastPolicySaveQueues.get(options.instanceId) === queue) {
      broadcastPolicySaveQueues.delete(options.instanceId);
      queue.dispose();
    }
  });
  slot.current = queue;

  return () => {
    if (slot.current === queue) {
      slot.current = null;
    }
    detach();
  };
}
