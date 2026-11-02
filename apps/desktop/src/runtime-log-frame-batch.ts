/** Keep only the newest already-merged document for a browser frame. Protocol
 * revisions are applied before enqueueing, so batching never drops an event.
 */
export function createRuntimeLogFrameBatch<T>(options: {
  schedule: (callback: () => void) => number;
  cancel: (handle: number) => void;
  publish: (value: T) => void;
}) {
  let handle: number | null = null;
  let pending: { value: T } | null = null;
  let generation = 0;
  return {
    push(value: T) {
      pending = { value };
      if (handle !== null) return;
      const scheduledGeneration = ++generation;
      handle = options.schedule(() => {
        if (scheduledGeneration !== generation) return;
        handle = null;
        const next = pending;
        pending = null;
        if (next) options.publish(next.value);
      });
    },
    clear() {
      generation++;
      if (handle !== null) options.cancel(handle);
      handle = null;
      pending = null;
    }
  };
}
