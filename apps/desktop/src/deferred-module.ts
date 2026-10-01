export interface DeferredModule<T> {
  peek(): T | null;
  peekError(): Error | null;
  canRetry(): boolean;
  load(): Promise<T>;
  retry(): Promise<T>;
}

export function createDeferredModule<T>(
  loader: () => Promise<T>,
  recoveryLoader?: () => Promise<T>
): DeferredModule<T> {
  let value: T | null = null;
  let failure: Error | null = null;
  let pending: Promise<T> | null = null;
  let recoveryUsed = false;

  function start(load: () => Promise<T>): Promise<T> {
    failure = null;
    pending = Promise.resolve().then(load).then(
      (loaded) => {
        value = loaded;
        pending = null;
        return loaded;
      },
      (error: unknown) => {
        failure = error instanceof Error ? error : new Error(String(error));
        pending = null;
        throw failure;
      }
    );
    return pending;
  }

  const source: DeferredModule<T> = {
    peek: () => value,
    peekError: () => failure,
    canRetry: () => failure !== null && recoveryLoader !== undefined && !recoveryUsed,
    load() {
      if (value !== null) return Promise.resolve(value);
      if (pending) return pending;
      if (failure) return Promise.reject(failure);
      return start(loader);
    },
    retry() {
      if (!source.canRetry() || pending) return source.load();
      recoveryUsed = true;
      // Browsers retain failed ESM requests. Only a separately bundled entry can
      // recover an entry fetch; shared dependency failures still require restart.
      return start(recoveryLoader ?? loader);
    }
  };
  return source;
}
