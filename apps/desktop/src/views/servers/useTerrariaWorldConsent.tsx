import { useEffect, useRef, useState } from "react";
import type { TerrariaWorldConflict } from "../../api-terraria-world";

interface Request {
  world: TerrariaWorldConflict;
  isCurrent(): boolean;
  resolve(accepted: boolean): void;
}

export function useTerrariaWorldConsent(selectedId: string | null) {
  const [request, setRequest] = useState<Request | null>(null);
  const active = useRef<Request | null>(null);
  const mounted = useRef(true);
  const selected = useRef(selectedId);
  selected.current = selectedId;

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      active.current?.resolve(false);
      active.current = null;
    };
  }, []);

  function finish(expected: Request, accepted: boolean) {
    if (active.current !== expected) return;
    active.current = null;
    setRequest(null);
    expected.resolve(accepted && expected.isCurrent() && selected.current === expected.world.instance_id);
  }

  useEffect(() => {
    if (request && (selectedId !== request.world.instance_id || !request.isCurrent())) finish(request, false);
  });

  function ask(world: TerrariaWorldConflict, isCurrent: () => boolean): Promise<boolean> {
    if (!mounted.current || !isCurrent() || world.instance_id !== selected.current) return Promise.resolve(false);
    active.current?.resolve(false);
    return new Promise(resolve => {
      const next = { world, isCurrent, resolve };
      active.current = next;
      setRequest(next);
    });
  }

  const review = request && request.world.instance_id === selectedId ? {
    world: request.world, respond: (accepted: boolean) => finish(request, accepted)
  } : null;
  return { ask, review };
}
