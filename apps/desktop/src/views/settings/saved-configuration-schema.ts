import type { TranslateFn } from "../../i18n";
import type { GuidedSettingsField, GuidedSettingsSchema, SettingsObject } from "./settings-schema";

// Saved instances use current field metadata, but never initialize defaults or
// drop keys that are no longer described by the module.
export function buildSavedConfigurationSchema(
  schema: GuidedSettingsSchema, settings: SettingsObject, t: TranslateFn
): GuidedSettingsSchema {
  const known = new Map((schema.presentationFields ?? schema.fields).map((field) => [field.key, field]));
  const controls = new Map(schema.fields.map((field) => [field.key, field]));
  const fields = Object.keys(settings).flatMap<GuidedSettingsField>((key) => {
    const metadata = known.get(key);
    if (metadata && !["configuration", "instance_network"].includes(metadata.presentation.owner)) return [];
    if (!metadata && ["runtime_performance", "runtime_restart"].includes(key)) return [];
    const source = controls.get(key);
    const value = settings[key];
    const field: GuidedSettingsField = source ? { ...source } : {
      key, title: metadata?.title ?? key, description: metadata?.description,
      sectionId: metadata?.sectionId ?? "saved_settings", icon: metadata?.icon,
      sourceKey: metadata?.sourceKey, sourceId: metadata?.sourceId, sourceSurface: metadata?.sourceSurface,
      type: typeof value === "boolean" ? "boolean" : typeof value === "number" ? "number" : "string",
      control: typeof value === "boolean" ? "checkbox" : typeof value === "number" ? "number" :
        typeof value === "object" ? "textarea" : "text", required: false,
      presentation: metadata?.presentation ?? { owner: "configuration", state: "editable", sectionId: "saved_settings" }
    };
    const { defaultValue: _defaultValue, defaultSource: _defaultSource, ...savedField } = field;
    return [{ ...savedField, presentation: { ...field.presentation, owner: "configuration", state: "editable" } }];
  });
  const sections = [...schema.sections];
  if (!sections.some((section) => section.id === "network")) sections.push({ id: "network",
    title: t("servers.archives.configuration.networkSection", undefined, "Connection and ports"), order: 10 });
  if (fields.some((field) => field.sectionId === "saved_settings") &&
    !sections.some((section) => section.id === "saved_settings")) sections.push({ id: "saved_settings",
    title: t("servers.archives.configuration.unknownSection", undefined, "Other saved settings"), order: Number.MAX_SAFE_INTEGER });
  return { ...schema, fields, presentationFields: fields, sections };
}
