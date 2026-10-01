let cachedWebGl2Support: boolean | null = null;

export function supportsWebGl2(): boolean {
  if (cachedWebGl2Support !== null) {
    return cachedWebGl2Support;
  }
  if (typeof document === "undefined") {
    return false;
  }

  try {
    const canvas = document.createElement("canvas");
    const context = canvas.getContext("webgl2", { powerPreference: "high-performance" });
    cachedWebGl2Support = Boolean(context);
    context?.getExtension("WEBGL_lose_context")?.loseContext();
  } catch {
    cachedWebGl2Support = false;
  }

  return cachedWebGl2Support;
}
