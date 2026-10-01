import inventory from "../../../../../../modules/dontstarve/world-options.json";
import type { TranslateFn } from "../../../i18n";
import type { SettingsModuleFieldGroup } from "../module-types";
import type { GuidedSettingsField } from "../settings-schema";

type Category = "worldgen" | "settings";
const WORLD_PAGES: Record<string, { category: Category; location: string; master: boolean }> = {
  mastergen: { category: "worldgen", location: "forest", master: true },
  mastersettings: { category: "settings", location: "forest", master: true },
  cavesgen: { category: "worldgen", location: "cave", master: false },
  cavessettings: { category: "settings", location: "cave", master: false }
};

const GROUP_TITLES: Record<Category, Record<string, string>> = {
  worldgen: {
    global: "Global", misc: "World", resources: "Resources",
    animals: "Creatures and Spawners", monsters: "Hostile Creatures and Spawners"
  },
  settings: {
    global: "Global", events: "Events", survivors: "Survivors", misc: "World",
    resources: "Resource Regrowth", portal_resources: "Unnatural Portal Resources", animals: "Creatures",
    monsters: "Hostile Creatures", giants: "Giants", lunar_mutations: "Lunar Mutations"
  }
};
const options = new Map(inventory.options.map((option) => [`${option.category}:${option.key}`, option]));

/** Group by the verified native binding, never by substrings in our field names. */
export function buildDontStarveWorldFieldGroups(
  sectionId: string,
  fields: GuidedSettingsField[],
  locale: string,
  t: TranslateFn
): SettingsModuleFieldGroup[] | undefined {
  const page = WORLD_PAGES[sectionId];
  if (!page) return undefined;

  const optionFor = (field: GuidedSettingsField) => {
    if (!field.sourceKey?.startsWith("overrides.")) return undefined;
    const option = options.get(`${page.category}:${field.sourceKey.slice("overrides.".length)}`);
    return option && option.locations.includes(page.location) && (page.master || !option.masterControlled)
      ? option : undefined;
  };
  const grouped = new Map<string, GuidedSettingsField[]>();
  for (const field of fields) {
    const id = field.key.endsWith("_preset") ? "presets" : optionFor(field)?.group ?? "other";
    const members = grouped.get(id);
    if (members) members.push(field);
    else grouped.set(id, [field]);
  }

  const nativeGroups = [...inventory.groups[page.category]].sort((a, b) => a.order - b.order);
  const collator = new Intl.Collator(locale, { numeric: true });
  const result: SettingsModuleFieldGroup[] = [];
  for (const id of ["presets", ...nativeGroups.map((group) => group.id), "other"]) {
    const members = grouped.get(id);
    if (!members) continue;
    members.sort((a, b) => {
      const left = optionFor(a)?.order ?? Number.MAX_SAFE_INTEGER;
      const right = optionFor(b)?.order ?? Number.MAX_SAFE_INTEGER;
      return left - right || collator.compare(a.title, b.title) || a.key.localeCompare(b.key);
    });
    result.push({
      id,
      title: id === "presets"
        ? t(`dst.settings.fieldGroups.${sectionId}.presets.title`, undefined, "Preset")
        : id === "other"
          ? t("dst.settings.worldGroups.other", undefined, "Other settings")
          : t(`dst.settings.worldGroups.${page.category}.${id}`, undefined, GROUP_TITLES[page.category][id] ?? id),
      layoutClass: `dst-${sectionId}-${id}`,
      fields: members
    });
  }
  return result;
}
