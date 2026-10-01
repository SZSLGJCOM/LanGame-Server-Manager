import { useEffect, useState } from "react";

export const ASSISTANT_PROMPT_ROTATION_MS = 5_000;
const REDUCED_MOTION_QUERY = "(prefers-reduced-motion: reduce)";

export function useAssistantPromptRotation<T extends { id: string }>(prompts: T[], paused = false) {
  const [activeId, setActiveId] = useState<string | null>(() => prompts[0]?.id ?? null);
  const [hidden, setHidden] = useState(() => typeof document !== "undefined" && document.hidden);
  const [reducedMotion, setReducedMotion] = useState(() =>
    typeof window !== "undefined" && window.matchMedia(REDUCED_MOTION_QUERY).matches
  );
  const currentIndex = Math.max(0, prompts.findIndex((prompt) => prompt.id === activeId));
  const current = prompts[currentIndex] ?? null;
  const nextId = prompts[(currentIndex + 1) % prompts.length]?.id ?? null;
  const promptIds = JSON.stringify(prompts.map((prompt) => prompt.id));
  const rotationPaused = paused || hidden || reducedMotion;

  useEffect(() => {
    const media = window.matchMedia(REDUCED_MOTION_QUERY);
    const updateVisibility = () => setHidden(document.hidden);
    const updateMotion = () => setReducedMotion(media.matches);
    updateVisibility();
    updateMotion();
    document.addEventListener("visibilitychange", updateVisibility);
    media.addEventListener("change", updateMotion);
    return () => {
      document.removeEventListener("visibilitychange", updateVisibility);
      media.removeEventListener("change", updateMotion);
    };
  }, []);

  useEffect(() => {
    if (rotationPaused || prompts.length < 2) return;
    const timer = window.setTimeout(() => setActiveId(nextId), ASSISTANT_PROMPT_ROTATION_MS);
    return () => window.clearTimeout(timer);
  }, [current?.id, nextId, promptIds, prompts.length, rotationPaused]);

  return current;
}
