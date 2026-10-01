import type { MemoryModuleSnapshot } from "../../types";
import { displayVolumePath } from "../../domain/system-volume-label";

function hardwareText(value: string | null | undefined) {
  const text = (value ?? "").replace(/[\u0000-\u001f\u007f]/g, " ").replace(/\s+/g, " ").trim();
  return /^(?:unknown|undefined|none|n\/a|not specified|to be filled by o\.?e\.?m\.?)$/i.test(text) ? "" : text;
}

/** Report the module's actual configured data rate; do not infer a brand from its part number. */
export function memoryHardwareCopy(modules: MemoryModuleSnapshot[] | null | undefined) {
  const models = new Set<string>();
  const speeds = new Set<number>();
  for (const module of modules ?? []) {
    const manufacturer = hardwareText(module.manufacturer);
    const part = hardwareText(module.part_number);
    const model = part && manufacturer && part.toLowerCase().startsWith(manufacturer.toLowerCase())
      ? part : [manufacturer, part].filter(Boolean).join(" ");
    const fallback = hardwareText(module.memory_type);
    if (model || fallback) models.add(model || fallback);
    const configured = module.configured_clock_mts;
    const speed = Number.isFinite(configured) && configured > 0 ? configured : module.speed_mts;
    if (Number.isFinite(speed) && speed > 0) speeds.add(Math.round(speed));
  }
  return {
    model: [...models].join(" + ") || null,
    speed: speeds.size ? `${[...speeds].sort((left, right) => left - right).join(" / ")} MT/s` : null
  };
}

export function diskHardwareCopy(model: string | null | undefined, label: string | null | undefined,
  paths: string[] = []) {
  const name = hardwareText(model);
  if (name && !/Volume\{[^}]+\}|^\\\\\?\\/i.test(name)) return name;
  return displayVolumePath(label) ?? paths.map(displayVolumePath).find(Boolean) ?? null;
}
