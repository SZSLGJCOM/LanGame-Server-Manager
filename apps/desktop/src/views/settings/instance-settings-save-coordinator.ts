interface SaveRegistration {
  owner: object | string;
  kind: "editor" | "operation";
  flush(): Promise<void>;
  completion: Promise<void> | null;
  settled: boolean;
}

/** Owned by one app tree; detached editors remain tracked until their writes settle. */
export class InstanceSettingsSaveCoordinator {
  private readonly registrations = new Map<string, Set<SaveRegistration>>();

  register(instanceId: string, flush: () => Promise<void>, owner: object = flush): { detach(completion: Promise<void>): void } {
    return this.createRegistration(instanceId, flush, owner, "editor");
  }

  /** Register before invoking the operation, including any preliminary read or download. */
  runOperation(instanceId: string, operation: () => Promise<void>, owner: string): Promise<void> {
    const completion = Promise.resolve().then(operation);
    this.createRegistration(instanceId, () => completion, owner, "operation").detach(completion);
    return completion;
  }

  private createRegistration(
    instanceId: string, flush: () => Promise<void>, owner: object | string, kind: SaveRegistration["kind"]
  ): { detach(completion: Promise<void>): void } {
    const entries = this.registrations.get(instanceId) ?? new Set<SaveRegistration>();
    // Reopening the editor loads persisted settings and replaces abandoned, settled drafts.
    // An older write still running must remain part of the instance's start barrier.
    // Reusing an owner follows queue.reset(), including StrictMode's effect replay.
    for (const entry of entries) {
      const sameEditor = kind === "editor" && entry.kind === "editor" && entry.owner === owner;
      const replacedFailure = entry.completion && entry.settled && (kind === "editor" || entry.owner === owner);
      if (sameEditor || replacedFailure) entries.delete(entry);
    }
    const entry: SaveRegistration = { owner, kind, flush, completion: null, settled: false };
    entries.add(entry);
    this.registrations.set(instanceId, entries);
    return {
      detach: (completion) => {
        if (entry.completion) return;
        entry.completion = completion;
        void completion.then(() => {
          entry.settled = true;
          entries.delete(entry);
          if (entries.size === 0 && this.registrations.get(instanceId) === entries) {
            this.registrations.delete(instanceId);
          }
        }, () => {
          // Keep failures available to future starts even when no editor is mounted.
          entry.settled = true;
        });
      }
    };
  }

  async flush(instanceId: string): Promise<void> {
    for (;;) {
      const entries = [...(this.registrations.get(instanceId) ?? [])];
      const completions = entries.map((entry) => entry.completion);
      await Promise.all(entries.map(async (entry) => {
        const completion = entry.completion;
        try {
          await (completion ?? entry.flush());
        } catch (error) {
          if (!completion && entry.completion) await entry.completion;
          else throw error;
        }
      }));
      const current = [...(this.registrations.get(instanceId) ?? [])];
      if (current.every((entry) => {
        const index = entries.indexOf(entry);
        return index >= 0 && entry.completion === completions[index];
      })) return;
    }
  }
}
