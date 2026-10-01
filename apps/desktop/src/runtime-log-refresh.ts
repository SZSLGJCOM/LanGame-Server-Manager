/** A selected console has one authoritative reader and one coalesced follow-up.
 * Events are invalidations, not positioned deltas that can be spliced into a
 * file snapshot. Disposal prevents old scopes from publishing or reading again.
 */
export function createRuntimeLogRefresh<T>(options: {
  read: () => Promise<T>;
  publish: (value: T) => void;
  failed: (error: unknown) => void;
  loading: (active: boolean) => void;
}) {
  let disposed = false;
  let running = false;
  let pending = false;

  async function drain() {
    try {
      while (pending && !disposed) {
        pending = false;
        try {
          const value = await options.read();
          if (!disposed) options.publish(value);
        } catch (error) {
          if (!disposed) options.failed(error);
        }
      }
    } finally {
      running = false;
      if (!disposed) options.loading(false);
    }
  }

  return {
    request() {
      if (disposed) return;
      pending = true;
      if (running) return;
      running = true;
      options.loading(true);
      void drain();
    },
    dispose() {
      disposed = true;
      pending = false;
    }
  };
}
