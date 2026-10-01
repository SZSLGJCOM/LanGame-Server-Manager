import { useCallback, useEffect, useState } from "react";
import type { DeferredModule } from "../deferred-module";

type ModuleState<T> =
  | { status: "idle" | "loading"; value: null; error: null }
  | { status: "ready"; value: T; error: null }
  | { status: "error"; value: null; error: Error };

export type DeferredModuleState<T> = ModuleState<T> & { canRetry: boolean; retry(): void };

export function useDeferredModule<T>(source: DeferredModule<T>, enabled = true): DeferredModuleState<T> {
  const [attempt, setAttempt] = useState(0);
  const [settled, setSettled] = useState<{
    source: DeferredModule<T>;
    attempt: number;
    state: ModuleState<T>;
  } | null>(null);
  const retry = useCallback(() => {
    void source.retry().catch(() => undefined);
    setAttempt((current) => current + 1);
  }, [source]);

  useEffect(() => {
    if (!enabled) {
      setSettled(null);
      return;
    }
    let active = true;
    setSettled(null);
    void source.load().then(
      (value) => {
        if (active) setSettled({ source, attempt, state: { status: "ready", value, error: null } });
      },
      (error: unknown) => {
        if (active) {
          setSettled({ source, attempt, state: {
            status: "error", value: null,
            error: error instanceof Error ? error : new Error(String(error))
          } });
        }
      }
    );
    return () => { active = false; };
  }, [source, enabled, attempt]);

  const controls = { canRetry: source.canRetry(), retry };
  if (!enabled) return { status: "idle", value: null, error: null, ...controls };
  const cached = source.peek();
  if (cached !== null) return { status: "ready", value: cached, error: null, ...controls };
  const failure = source.peekError();
  if (failure) return { status: "error", value: null, error: failure, ...controls };
  if (settled?.source === source && settled.attempt === attempt) return { ...settled.state, ...controls };
  return { status: "loading", value: null, error: null, ...controls };
}
