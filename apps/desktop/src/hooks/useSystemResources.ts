import { useEffect, useReducer, useRef } from "react";
import type { SystemSnapshot } from "../types";
import { evaluateSystemResources, type ResourceObservation } from "../domain/system-resources";

/** Clock ticks expire retained observations even if polling is paused or fails. */
export function useSystemResources(snapshot: SystemSnapshot) {
  const [, updateClock] = useReducer((revision: number) => revision + 1, 0);
  const observation = useRef<ResourceObservation | undefined>(undefined);
  const assessment = evaluateSystemResources(snapshot, Date.now(), observation.current);
  useEffect(() => {
    observation.current = assessment.observation;
  }, [assessment.observation]);
  useEffect(() => {
    const update = () => updateClock();
    const timer = window.setInterval(update, 5_000);
    window.addEventListener("focus", update);
    document.addEventListener("visibilitychange", update);
    return () => {
      window.clearInterval(timer);
      window.removeEventListener("focus", update);
      document.removeEventListener("visibilitychange", update);
    };
  }, []);
  return assessment;
}
