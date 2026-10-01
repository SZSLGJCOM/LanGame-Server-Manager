import { isChineseLocale, type LocaleCode, type TranslateFn } from "../../i18n";
import type { ModuleDetails } from "../../types";
import {
  resolveConfigurationFieldPresentation,
  resolveConfigurationRendererContract
} from "./configuration-presentation";
import { resolveSettingsModuleDefinition } from "./module-registry";
import { buildGuidedSections, withCommonSectionParent } from "./configuration-sections";
import { localizeSchemaTitleFallback } from "./schema-title-localization";
import { GUIDED_CORE_SECTION_IDS } from "./settings-schema";
import { isOptionalBooleanOverride } from "./guided-setting-values";
export {
  findEnumOptionIndex,
  isOptionalBooleanOverride,
  parseWorkshopIdList,
  serializeWorkshopIdList
} from "./guided-setting-values";
import type {
  ConfigurationFieldBehavior,
  ConfigurationFieldPresentation,
  ConfigurationPresentationField,
  GuidedControlType,
  GuidedDefaultSource,
  GuidedEnumOption,
  GuidedFieldCopy,
  GuidedFieldDefaultContext,
  GuidedFieldType,
  GuidedSectionId,
  GuidedSettingsField,
  GuidedSettingsSchema,
  GuidedSettingsSection,
  GuidedSettingsValidationIssue,
  SettingsObject
} from "./settings-schema";

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function readSchemaExtensionString(property: Record<string, unknown>, key: string): string | undefined {
  const value = property[key];
  return typeof value === "string" && value.trim().length > 0 ? value.trim() : undefined;
}

function readSchemaExtensionStrings(property: Record<string, unknown>, key: string): string[] | undefined {
  const value = property[key];
  if (!Array.isArray(value)) {
    return undefined;
  }
  const strings = value
    .filter((item): item is string => typeof item === "string")
    .map((item) => item.trim())
    .filter(Boolean);
  return strings.length > 0 ? strings : undefined;
}

function containsHanText(value: string): boolean {
  return /[\u3400-\u9fff]/.test(value);
}

function readSchemaDisplayString(value: unknown, locale: LocaleCode): string | undefined {
  if (typeof value !== "string" || value.trim().length === 0) {
    return undefined;
  }

  const normalized = value.trim();
  if (isChineseLocale(locale) && !containsHanText(normalized)) {
    return undefined;
  }

  if (!isChineseLocale(locale) && containsHanText(normalized)) {
    return undefined;
  }

  return normalized;
}

const SETTINGS_COPY_KEY_RE = /^(?:[a-z0-9-]+\.settings\.|settings\.(?:schema|guided)\.)/i;

function isSettingsCopyKey(key: string): boolean {
  return SETTINGS_COPY_KEY_RE.test(key);
}

function isMinimalEnumLabel(value: string): boolean {
  return /^(?:[\u3400-\u9fff]|[+-]?\d+(?:\.\d+)?)$/.test(value);
}

function isReadableSettingsCopy(value: string | undefined, locale: LocaleCode): value is string {
  if (!value) {
    return false;
  }

  const normalized = value.trim();
  return normalized.length > 0 &&
    containsHanText(normalized) === isChineseLocale(locale) &&
    !isPlaceholderSettingsCopy(normalized);
}

function extractNativeDescriptionText(value: string): string | undefined {
  const match = value.match(/Native description:\s*(.+)$/i);
  if (!match) {
    return undefined;
  }

  const candidate = match[1].trim();
  if (!containsHanText(candidate)) {
    return undefined;
  }

  return tidyNativeDescriptionText(candidate);
}

function tidyNativeDescriptionText(value: string): string {
  return value
    .replace(/^\s*[:：]\s*/, "")
    .replace(/\s*[,，]\s*/g, "，")
    .replace(/\s*[;；]\s*/g, "；")
    .replace(/\s+/g, " ")
    .trim();
}

function isPlaceholderSettingsCopy(value: string): boolean {
  const compact = value.replace(/\s+/g, "");

  if (compact.length <= 1) {
    return true;
  }

  if (
    compact === "配置项" ||
    compact === "配置项配置项" ||
    compact === "配置项配置项分组" ||
    compact === "配置项分组"
  ) {
    return true;
  }

  if (/^配置项配置项分组/.test(compact) || (/^配置项/.test(compact) && compact.length <= 14)) {
    return true;
  }

  return false;
}

function coalesceChineseFallback(fallback: string | undefined, raw: string | undefined): string | undefined {
  const candidates = [fallback, raw]
    .map((candidate) => candidate?.trim())
    .filter((candidate): candidate is string => Boolean(candidate && candidate.length > 0));

  for (const candidate of candidates) {
    const localized = localizeSchemaTitleFallback(candidate);
    if (localized) {
      return localized;
    }
  }

  return candidates[0];
}

function sanitizeSettingsCopyValue(
  key: string,
  locale: LocaleCode,
  value: string,
  fallback: string
): string {
  if (!isChineseLocale(locale) || !isSettingsCopyKey(key)) {
    return value;
  }

  const normalized = value.trim();
  if (!normalized) {
    return "";
  }

  // Enum choices may legitimately be one character or numeric (for example 短, 无, 0).
  if (key.includes(".option.") && isMinimalEnumLabel(normalized)) {
    return normalized;
  }

  if (isPlaceholderSettingsCopy(normalized)) {
    return coalesceChineseFallback(fallback, normalized) ?? "";
  }

  if (!containsHanText(normalized) && /[A-Za-z]/.test(normalized)) {
    return coalesceChineseFallback(normalized, fallback) ?? normalized;
  }

  return normalized;
}

function sanitizeSettingsCopyTranslator(
  locale: LocaleCode,
  t: TranslateFn
): TranslateFn {
  if (!isChineseLocale(locale)) {
    return t;
  }

  return (key, params, fallback) => {
    const raw = t(key, params, fallback);
    if (typeof raw !== "string") {
      return raw;
    }

    if (!isSettingsCopyKey(key)) {
      return raw;
    }

    return sanitizeSettingsCopyValue(key, locale, raw, typeof fallback === "string" ? fallback : "");
  };
}

function readSchemaExtensionNumber(property: Record<string, unknown>, key: string): number | undefined {
  const value = property[key];
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

function readSchemaDefaultSource(
  property: Record<string, unknown>
): GuidedDefaultSource | undefined {
  const value = property["x-lsgm-default-source"];
  return value === "instance_id" || value === "instance_name" ? value : undefined;
}

function humanizeKey(key: string): string {
  const acronymMap: Record<string, string> = {
    api: "API",
    fps: "FPS",
    hp: "HP",
    id: "ID",
    ip: "IP",
    json: "JSON",
    pvp: "PvP",
    rcon: "RCON",
    rest: "REST",
    ssd: "SSD",
    ui: "UI",
    url: "URL"
  };

  const normalized = key
    .replace(/([a-z0-9])([A-Z])/g, "$1 $2")
    .replace(/([A-Z]+)([A-Z][a-z])/g, "$1 $2")
    .replace(/[_-]+/g, " ")
    .replace(/\s+/g, " ")
    .trim();

  const segments = normalized.split(" ").filter(Boolean);
  if (segments.length === 0) {
    return "";
  }

  const normalizedSegments =
    segments.length > 1 && ["b", "is"].includes(segments[0].toLowerCase())
      ? segments.slice(1)
      : segments;

  return normalizedSegments
    .map((segment) => {
      const lowered = segment.toLowerCase();
      if (acronymMap[lowered]) {
        return acronymMap[lowered];
      }
      return lowered.charAt(0).toUpperCase() + lowered.slice(1);
    })
    .join(" ");
}

function humanizeEnumValue(value: unknown): string {
  if (typeof value === "string") {
    return humanizeKey(value);
  }
  if (typeof value === "number" || typeof value === "boolean") {
    return String(value);
  }
  return JSON.stringify(value);
}

function normalizeFieldType(typeValue: unknown): GuidedFieldType | null {
  if (typeof typeValue === "string") {
    return ["string", "integer", "number", "boolean"].includes(typeValue)
      ? (typeValue as GuidedFieldType)
      : null;
  }

  if (Array.isArray(typeValue)) {
    for (const candidate of typeValue) {
      const normalized = normalizeFieldType(candidate);
      if (normalized) {
        return normalized;
      }
    }
  }

  return null;
}

function buildGenericSections(t: TranslateFn): GuidedSettingsSection[] {
  return [
    {
      id: "world",
      title: t("settings.guided.sections.world", undefined, "World & Gameplay"),
      description: t("settings.guided.sections.worldDescription", undefined, "World generation, difficulty, game rules, and shard-related options.")
    },
    {
      id: "advanced",
      title: t("settings.guided.sections.advanced", undefined, "Advanced"),
      description: t("settings.guided.sections.advancedDescription", undefined, "Less common fields that still need a direct UI.")
    }
  ];
}

function resolveSectionSortIndex(sectionOrder: GuidedSectionId[], sectionId: GuidedSectionId): number {
  const index = sectionOrder.indexOf(sectionId);
  return index >= 0 ? index : sectionOrder.length;
}

function comparePresentationFields(
  sectionOrder: GuidedSectionId[],
  left: ConfigurationPresentationField,
  right: ConfigurationPresentationField
): number {
  const sectionDiff =
    resolveSectionSortIndex(sectionOrder, left.sectionId) -
    resolveSectionSortIndex(sectionOrder, right.sectionId);
  if (sectionDiff !== 0) return sectionDiff;
  const weightDiff =
    (left.sortWeight ?? Number.MAX_SAFE_INTEGER) -
    (right.sortWeight ?? Number.MAX_SAFE_INTEGER);
  return weightDiff || left.title.localeCompare(right.title);
}

function appendMissingSections(
  sections: GuidedSettingsSection[],
  fields: ConfigurationPresentationField[],
  t: TranslateFn
): GuidedSettingsSection[] {
  const sectionIds = new Set(sections.map((section) => section.id));
  const missingSectionIds = Array.from(
    new Set(fields.map((field) => field.sectionId).filter((sectionId) => !sectionIds.has(sectionId)))
  );

  if (missingSectionIds.length === 0) {
    return sections;
  }

  const extraSections: GuidedSettingsSection[] = missingSectionIds.map((sectionId) => {
    const fallbackTitle = humanizeKey(sectionId);
    const description =
      t(`settings.guided.sections.${sectionId}Description`, undefined, "").trim() || undefined;

    return withCommonSectionParent({
      id: sectionId,
      title: t(`settings.guided.sections.${sectionId}`, undefined, fallbackTitle || sectionId),
      description
    });
  });

  return [...sections, ...extraSections];
}

function resolveLocalizedSchemaCopy(
  moduleId: string | undefined,
  key: string,
  title: string | undefined,
  locale: LocaleCode,
  t: TranslateFn
): Partial<GuidedFieldCopy> | undefined {
  if (!moduleId || !isChineseLocale(locale)) {
    return undefined;
  }

  const baseKey = `settings.schema.${moduleId}.${key}`;
  const titleKey = `${baseKey}.title`;
  const descriptionKey = `${baseKey}.description`;
  const rawTitle = t(titleKey, undefined, "");
  const rawDescription = t(descriptionKey, undefined, "");
  const fallbackTitle = coalesceChineseFallback(localizeSchemaTitleFallback(title), title) ?? "";
  const localizedTitle = sanitizeSettingsCopyValue(
    titleKey,
    locale,
    rawTitle,
    fallbackTitle
  );
  const description = (() => {
    if (!rawDescription) {
      return "";
    }

    if (isPlaceholderSettingsCopy(rawDescription)) {
      return "";
    }

    const nativeText = extractNativeDescriptionText(rawDescription) ?? rawDescription.trim();
    if (!nativeText || !containsHanText(nativeText)) {
      return "";
    }

    return nativeText;
  })();

  const resolvedTitle = isReadableSettingsCopy(localizedTitle, locale) ? localizedTitle : undefined;
  const resolvedDescription = description || undefined;

  if (!resolvedTitle && !resolvedDescription) {
    return undefined;
  }

  return {
    ...(resolvedTitle ? { title: resolvedTitle } : {}),
    description: description || undefined
  };
}

function buildSchemaEnumOptionKey(value: unknown): string {
  const raw = String(value).trim();
  const prefix = raw.startsWith("-") ? "minus_" : "";
  const normalized = raw
    .replace(/^-+/, "")
    .replace(/[^A-Za-z0-9]+/g, "_")
    .replace(/^_+|_+$/g, "")
    .toLowerCase();

  return `${prefix}${normalized || "empty"}`;
}

function resolveLocalizedSchemaEnumOptionLabel(
  moduleId: string | undefined,
  fieldKey: string,
  value: unknown,
  locale: LocaleCode,
  t: TranslateFn
): string | undefined {
  if (!moduleId || !isChineseLocale(locale)) {
    return undefined;
  }

  const rawLabel = t(
    `settings.schema.${moduleId}.${fieldKey}.option.${buildSchemaEnumOptionKey(value)}`,
    undefined,
    ""
  );

  if (!rawLabel) {
    return undefined;
  }

  const sanitized = sanitizeSettingsCopyValue(
    `settings.schema.${moduleId}.${fieldKey}.option.${buildSchemaEnumOptionKey(value)}`,
    locale,
    rawLabel,
    ""
  );

  return isMinimalEnumLabel(sanitized) || isReadableSettingsCopy(sanitized, locale) ? sanitized : undefined;
}

function resolveFieldControl(
  type: GuidedFieldType,
  enumOptions: GuidedEnumOption[],
  behavior?: ConfigurationFieldBehavior
): GuidedControlType {
  if (type === "boolean") {
    return "checkbox";
  }

  if (enumOptions.length > 0) {
    return "select";
  }

  if (type === "integer" || type === "number") {
    return "number";
  }

  if (behavior === "secret") {
    return "password";
  }

  if (behavior === "multiline" || behavior === "raw") {
    return "textarea";
  }

  return "text";
}

function resolveEnumOptions(property: Record<string, unknown>): GuidedEnumOption[] {
  if (!Array.isArray(property.enum)) {
    return [];
  }

  const enumValues = property.enum as unknown[];
  const rawLabels = Array.isArray(property.enum_labels)
    ? property.enum_labels.filter((label): label is string => typeof label === "string")
    : null;

  return enumValues.map((value, index) => ({
    value,
    label: rawLabels && rawLabels.length === enumValues.length
      ? rawLabels[index]
      : humanizeEnumValue(value)
  }));
}

function resolveSuggestionOptions(property: Record<string, unknown>, locale: LocaleCode): GuidedEnumOption[] {
  const suggestions = property["x-lsgm-suggestions"];
  if (!Array.isArray(suggestions)) {
    return [];
  }

  return suggestions.flatMap((suggestion) => {
    if (typeof suggestion === "string" && suggestion.trim().length > 0) {
      const value = suggestion.trim();
      return [{ value, label: value }];
    }

    if (!isRecord(suggestion)) {
      return [];
    }

    const value = suggestion.value;
    if (
      typeof value !== "string" &&
      typeof value !== "number" &&
      typeof value !== "boolean"
    ) {
      return [];
    }

    const localizedLabel = readSchemaDisplayString(suggestion["x-lsgm-label-zh-CN"], locale);
    const label = localizedLabel ?? (typeof suggestion.label === "string" && suggestion.label.trim().length > 0
      ? suggestion.label.trim()
      : humanizeEnumValue(value));

    return [{ value, label }];
  });
}

export function parseSettingsObject(
  settingsJson: string,
  t: TranslateFn = (key, _params, fallback) => fallback ?? key
): {
  value: SettingsObject | null;
  error: string | null;
} {
  try {
    const parsed = JSON.parse(settingsJson);
    if (!isRecord(parsed)) {
      return { value: null, error: t("settings.errors.settingsObjectExpected", undefined, "Settings JSON must be an object.") };
    }

    return { value: parsed, error: null };
  } catch (error) {
    return { value: null, error: String((error as Error).message || error) };
  }
}

export type GuidedSettingsSurface = "general" | "mods" | "player_access" | "maintenance";

export interface ParseGuidedSettingsSchemaOptions {
  surface?: GuidedSettingsSurface;
  fieldIcons?: Readonly<Record<string, string>>;
}

const DST_WORLD_ICON_SECTIONS = new Set(["mastergen", "mastersettings", "cavesgen", "cavessettings"]);

function presentationBelongsToSurface(
  presentation: ConfigurationFieldPresentation,
  surface: GuidedSettingsSurface
): boolean {
  if (presentation.state !== "editable" && presentation.state !== "specialized") {
    return false;
  }
  if (surface === "mods") {
    return presentation.owner === "mods";
  }
  if (surface === "player_access") {
    return presentation.owner === "player_access";
  }
  if (surface === "maintenance") {
    return presentation.owner === "maintenance";
  }
  return presentation.owner === "configuration";
}

export function parseGuidedSettingsSchema(
  moduleDetails?: ModuleDetails | null,
  locale: LocaleCode = "en-US",
  t: TranslateFn = (key, _params, fallback) => fallback ?? key,
  options: ParseGuidedSettingsSchemaOptions = {}
): GuidedSettingsSchema {
  const fallbackTitle = moduleDetails?.summary.name ?? "Module Settings";
  const moduleId = moduleDetails?.summary.id;
  const surface = options.surface ?? "general";
  const moduleDefinition = resolveSettingsModuleDefinition(moduleId);
  const localizationModuleId = moduleDefinition?.id ?? moduleId;
  const settingsText = sanitizeSettingsCopyTranslator(locale, t);
  const sections = buildGuidedSections(
    moduleDefinition?.getSections?.(settingsText, locale) ?? moduleDefinition?.sections ?? buildGenericSections(settingsText),
    settingsText
  );
  const schemaJson = moduleDetails?.schema_json;

  if (!schemaJson) {
    return { title: fallbackTitle, sections, fields: [], parseError: null };
  }

  try {
    const parsed = JSON.parse(schemaJson);
    if (!isRecord(parsed)) {
      return { title: fallbackTitle, sections, fields: [], parseError: t("settings.errors.schemaObjectExpected", undefined, "Schema JSON must be an object.") };
    }

    const properties = isRecord(parsed.properties) ? parsed.properties : null;
    if (!properties) {
      return { title: String(parsed.title ?? fallbackTitle), sections, fields: [], parseError: null };
    }

    const requiredKeys = new Set(
      Array.isArray(parsed.required)
        ? parsed.required.filter((item): item is string => typeof item === "string")
        : []
    );
    const sectionOrder = sections.map((section) => section.id);

    const presentationFields: ConfigurationPresentationField[] = [];
    const fields: GuidedSettingsField[] = Object.entries(properties)
      .filter(([, property]) => isRecord(property))
      .flatMap(([key, property]) => {
        if (!isRecord(property)) {
          return [];
        }

        const presentation = resolveConfigurationFieldPresentation(key, property, moduleDefinition);
        const type = normalizeFieldType(property.type);
        const enumOptions = resolveEnumOptions(property);
        const suggestionOptions = resolveSuggestionOptions(property, locale);
        const schemaTitle = readSchemaDisplayString(property.title, locale);
        const schemaDescription = readSchemaDisplayString(property.description, locale);
        const moduleFieldCopy = moduleDefinition?.getFieldCopy?.(key, settingsText, locale);
        const localizedSchemaCopy = resolveLocalizedSchemaCopy(
          localizationModuleId,
          key,
          schemaTitle,
          locale,
          settingsText
        );
        const fieldTitleKey = `settings.schema.${localizationModuleId}.${key}.title`;
        const fieldDescriptionKey = `settings.schema.${localizationModuleId}.${key}.description`;
        const schemaFallbackTitle = (isChineseLocale(locale)
          ? coalesceChineseFallback(localizeSchemaTitleFallback(schemaTitle), schemaTitle)
          : schemaTitle) ?? humanizeKey(key);
        const sanitizedModuleTitle = sanitizeSettingsCopyValue(
          fieldTitleKey,
          locale,
          moduleFieldCopy?.title ?? "",
          localizedSchemaCopy?.title ?? schemaFallbackTitle
        );
        const sanitizedModuleDescription = sanitizeSettingsCopyValue(
          fieldDescriptionKey,
          locale,
          moduleFieldCopy?.description ?? "",
          localizedSchemaCopy?.description ?? ""
        );
        const hasUsableModuleCopy =
          isReadableSettingsCopy(sanitizedModuleTitle, locale) ||
          isReadableSettingsCopy(sanitizedModuleDescription, locale);
        const hasUsableLocalizedCopy =
          isReadableSettingsCopy(localizedSchemaCopy?.title, locale) ||
          isReadableSettingsCopy(localizedSchemaCopy?.description, locale);
        const localizedTitle = localizedSchemaCopy?.title;
        const localizedDescription = localizedSchemaCopy?.description;
        const fieldTitle =
          isReadableSettingsCopy(sanitizedModuleTitle, locale)
            ? sanitizedModuleTitle
            : isReadableSettingsCopy(localizedTitle, locale)
              ? localizedTitle
              : schemaFallbackTitle;
        const fieldDescription =
          isReadableSettingsCopy(sanitizedModuleDescription, locale)
            ? sanitizedModuleDescription
            : isReadableSettingsCopy(localizedDescription, locale)
              ? localizedDescription
              : hasUsableModuleCopy || hasUsableLocalizedCopy
              ? null
              : schemaDescription ?? null;
        const localizedEnumOptions = enumOptions.map((option) => ({
          ...option,
          label:
            moduleDefinition?.getEnumOptionLabel?.(key, option.value, locale, settingsText) ??
            resolveLocalizedSchemaEnumOptionLabel(localizationModuleId, key, option.value, locale, settingsText) ??
            option.label
        }));
        const localizedSuggestionOptions = suggestionOptions.map((option) => ({
          ...option,
          label:
            moduleDefinition?.getEnumOptionLabel?.(key, option.value, locale, settingsText) ??
            resolveLocalizedSchemaEnumOptionLabel(localizationModuleId, key, option.value, locale, settingsText) ??
            option.label
        }));
        const schemaSortWeight = readSchemaExtensionNumber(property, "x-lsgm-order");
        const sourceId = readSchemaExtensionString(property, "x-lsgm-source");
        const sourceKey = readSchemaExtensionString(property, "x-lsgm-source-key");
        const sourceSurface = readSchemaExtensionString(property, "x-lsgm-source-surface");
        const defaultSource = readSchemaDefaultSource(property);
        const preserveNativeWhenUnset = property["x-lsgm-preserve-native-when-unset"] === true;
        if (preserveNativeWhenUnset && (requiredKeys.has(key) || defaultSource ||
          sourceSurface !== "materializer" ||
          !(type === "boolean" || type === "integer" && enumOptions.length > 0))) {
          throw new Error(`Native preservation requires an optional materialized boolean or integer enum: ${key}`);
        }
        const minLength = type === "string" && typeof property.minLength === "number" ? property.minLength : undefined;
        const maxLength = type === "string" && typeof property.maxLength === "number" ? property.maxLength : undefined;
        const pattern = type === "string" && typeof property.pattern === "string" ? property.pattern : undefined;
        const disallowedLinePrefixes = type === "string"
          ? readSchemaExtensionStrings(property, "x-lsgm-disallowed-line-prefixes")
          : undefined;
        const minimum = typeof property.minimum === "number" ? property.minimum : undefined;
        const maximum = typeof property.maximum === "number" ? property.maximum : undefined;
        const step = typeof property.multipleOf === "number"
          ? property.multipleOf
          : (type === "integer" ? 1 : undefined);

        const sectionId = presentation.sectionId;
        const specializedRenderer = presentation.rendererId
          ? resolveConfigurationRendererContract(presentation.rendererId, moduleDefinition)
          : undefined;
        const specializedEditorVariant = specializedRenderer?.kind === "guided-field"
          ? specializedRenderer.editorVariant
          : undefined;

        const presentationField: ConfigurationPresentationField = {
          key,
          title: fieldTitle,
          description: fieldDescription,
          sectionId,
          sortWeight: schemaSortWeight ?? moduleDefinition?.resolveFieldSortWeight?.(key),
          sourceId,
          sourceKey,
          sourceSurface,
          icon: (moduleId === "dontstarve" && DST_WORLD_ICON_SECTIONS.has(sectionId)
            ? options.fieldIcons?.[key]
            : undefined) ?? moduleDefinition?.resolveFieldIcon?.(key) ?? null,
          presentation
        };
        presentationFields.push(presentationField);
        if (!presentationBelongsToSurface(presentation, surface) || !type) {
          return [];
        }

        return [{
          ...presentationField,
          type,
          control: resolveFieldControl(type, enumOptions, presentation.behavior),
          editorVariant: specializedEditorVariant ?? moduleDefinition?.resolveFieldEditorVariant?.(key),
          required: requiredKeys.has(key),
          defaultValue: property.default,
          defaultSource,
          preserveNativeWhenUnset,
          enumOptions: localizedEnumOptions.length > 0 ? localizedEnumOptions : undefined,
          suggestions: localizedSuggestionOptions.length > 0 ? localizedSuggestionOptions : undefined,
          minLength,
          maxLength,
          pattern,
          disallowedLinePrefixes,
          minimum,
          maximum,
          step,
        }];
      })
      .sort((left, right) => comparePresentationFields(sectionOrder, left, right));
    presentationFields.push(
      ...(moduleDefinition?.getAdditionalPresentationFields?.({ locale, t: settingsText }) ?? [])
    );
    presentationFields.sort((left, right) => comparePresentationFields(sectionOrder, left, right));

    const visibleSections = surface === "mods"
      ? sections.filter(
        (section) =>
          GUIDED_CORE_SECTION_IDS.includes(section.id as typeof GUIDED_CORE_SECTION_IDS[number]) ||
          fields.some((field) => field.sectionId === section.id)
      )
      : sections;
    const workspacePresentationFields = surface === "general" ? presentationFields : fields;
    const normalizedSections = appendMissingSections(visibleSections, workspacePresentationFields, settingsText);

    return {
      title: typeof parsed.title === "string" ? parsed.title : fallbackTitle,
      sections: normalizedSections,
      fields,
      presentationFields: workspacePresentationFields,
      parseError: null
    };
  } catch (error) {
    return {
      title: fallbackTitle,
      sections,
      fields: [],
      parseError: String((error as Error).message || error)
    };
  }
}

export function resolveGuidedFieldDefaultValue(
  field: GuidedSettingsField,
  context?: GuidedFieldDefaultContext
): unknown {
  if (field.preserveNativeWhenUnset) return undefined;
  switch (field.defaultSource) {
    case "instance_id":
      if (context?.instanceId) {
        return context.instanceId;
      }
      break;
    case "instance_name":
      if (context?.instanceName) {
        return context.instanceName;
      }
      break;
    default:
      break;
  }

  if (field.defaultValue !== undefined) {
    return field.defaultValue;
  }

  return field.type === "boolean" ? false : "";
}

export function readGuidedFieldValue(
  field: GuidedSettingsField,
  settings: SettingsObject,
  context?: GuidedFieldDefaultContext
): unknown {
  if (Object.prototype.hasOwnProperty.call(settings, field.key)) {
    return settings[field.key];
  }

  return resolveGuidedFieldDefaultValue(field, context);
}

export function validateGuidedSettingsObject(
  schema: GuidedSettingsSchema,
  settings: SettingsObject,
  context?: GuidedFieldDefaultContext,
  t?: TranslateFn
): GuidedSettingsValidationIssue[] {
  const issues: GuidedSettingsValidationIssue[] = [];

  for (const field of schema.fields) {
    const value = readGuidedFieldValue(field, settings, context);

    if (field.required && (value === null || value === undefined || value === "")) {
      issues.push({
        fieldKey: field.key,
        reason: "required",
        message: t?.(
          "settings.configuration.validation.required",
          { field: field.title },
          `${field.title} is required.`
        ) ?? `${field.title} is required.`
      });
      continue;
    }

    const hasEnumValue = value !== null && value !== undefined &&
      (Object.prototype.hasOwnProperty.call(settings, field.key) || field.defaultValue !== undefined || field.defaultSource !== undefined);
    if (hasEnumValue && field.enumOptions?.length &&
        !field.enumOptions.some((option) => option.value === value)) {
      issues.push({
        fieldKey: field.key,
        reason: "enum",
        message: t?.(
          "settings.configuration.validation.enum",
          { field: field.title },
          `${field.title} must use one of the available options.`
        ) ?? `${field.title} must use one of the available options.`
      });
      continue;
    }

    if (field.type === "string") {
      const text = typeof value === "string" ? value : String(value ?? "");
      if (field.minLength !== undefined && text.length < field.minLength) {
        issues.push({
          fieldKey: field.key,
          reason: "minLength",
          message: t?.(
            "settings.configuration.validation.minLength",
            { field: field.title, minimum: field.minLength },
            `${field.title} must be at least ${field.minLength} characters.`
          ) ?? `${field.title} must be at least ${field.minLength} characters.`
        });
      }
      if (field.maxLength !== undefined && text.length > field.maxLength) {
        issues.push({
          fieldKey: field.key,
          reason: "maxLength",
          message: t?.(
            "settings.configuration.validation.maxLength",
            { field: field.title, maximum: field.maxLength },
            `${field.title} must be at most ${field.maxLength} characters.`
          ) ?? `${field.title} must be at most ${field.maxLength} characters.`
        });
      }
      if (field.pattern) {
        try {
          if (!new RegExp(field.pattern).test(text)) {
            issues.push({
              fieldKey: field.key,
              reason: "pattern",
              message: t?.(
                "settings.configuration.validation.pattern",
                { field: field.title },
                `${field.title} does not match the required format.`
              ) ?? `${field.title} does not match the required format.`
            });
          }
        } catch {
          issues.push({
            fieldKey: field.key,
            reason: "pattern",
            message: t?.(
              "settings.configuration.validation.invalidPattern",
              { field: field.title },
              `${field.title} has an invalid schema pattern.`
            ) ?? `${field.title} has an invalid schema pattern.`
          });
        }
      }
      if (field.disallowedLinePrefixes) {
        const prefixes = field.disallowedLinePrefixes.map((prefix) => prefix.toLocaleLowerCase());
        for (const [index, line] of text.split(/\r?\n/).entries()) {
          const trimmed = line.trimStart();
          if (!trimmed || trimmed.startsWith(";") || trimmed.startsWith("#") || trimmed.startsWith("//")) {
            continue;
          }
          const lower = trimmed.toLocaleLowerCase();
          const prefix = prefixes.find((candidate) => lower.startsWith(candidate));
          if (prefix) {
            issues.push({
              fieldKey: field.key,
              reason: "managedDirective",
              message: t?.(
                "settings.configuration.validation.managedDirective",
                { field: field.title, line: index + 1, prefix },
                `${field.title} line ${index + 1} duplicates managed directive ${prefix}.`
              ) ?? `${field.title} line ${index + 1} duplicates managed directive ${prefix}.`
            });
          }
        }
      }
      continue;
    }

    if (field.type === "integer" || field.type === "number") {
      if (value === "" || value === null || value === undefined) {
        continue;
      }
      if (typeof value !== "number" || !Number.isFinite(value)) {
        issues.push({
          fieldKey: field.key,
          reason: "number",
          message: t?.(
            "settings.configuration.validation.number",
            { field: field.title },
            `${field.title} must be a number.`
          ) ?? `${field.title} must be a number.`
        });
        continue;
      }
      const numericValue = value;
      if (field.type === "integer" && !Number.isSafeInteger(numericValue)) {
        issues.push({
          fieldKey: field.key,
          reason: "number",
          message: t?.(
            "settings.configuration.validation.integer",
            { field: field.title },
            `${field.title} must be a whole number within the supported range.`
          ) ?? `${field.title} must be a whole number within the supported range.`
        });
        continue;
      }
      if (field.minimum !== undefined && numericValue < field.minimum) {
        issues.push({
          fieldKey: field.key,
          reason: "minimum",
          message: t?.(
            "settings.configuration.validation.minimum",
            { field: field.title, minimum: field.minimum },
            `${field.title} must be at least ${field.minimum}.`
          ) ?? `${field.title} must be at least ${field.minimum}.`
        });
      }
      if (field.maximum !== undefined && numericValue > field.maximum) {
        issues.push({
          fieldKey: field.key,
          reason: "maximum",
          message: t?.(
            "settings.configuration.validation.maximum",
            { field: field.title, maximum: field.maximum },
            `${field.title} must be at most ${field.maximum}.`
          ) ?? `${field.title} must be at most ${field.maximum}.`
        });
      }
    }
  }

  return issues;
}

export function writeGuidedFieldValue(
  settings: SettingsObject,
  field: GuidedSettingsField,
  rawValue: unknown
): SettingsObject {
  const next: SettingsObject = { ...settings };

  switch (field.type) {
    case "boolean":
      if (rawValue === undefined && isOptionalBooleanOverride(field)) {
        delete next[field.key];
        return next;
      }
      next[field.key] = Boolean(rawValue);
      return next;
    case "integer":
    case "number": {
      if (rawValue === "" || rawValue === null || rawValue === undefined) {
        delete next[field.key];
        return next;
      }

      // Keep incomplete edits visible; validation prevents these drafts from reaching the server.
      if (typeof rawValue === "string" && !/^[+-]?(?:\d+(?:\.\d+)?|\.\d+)(?:[eE][+-]?\d+)?$/.test(rawValue)) {
        next[field.key] = rawValue;
        return next;
      }

      const numericValue = Number(rawValue);
      next[field.key] = Number.isFinite(numericValue) ? numericValue : String(rawValue);
      return next;
    }
    case "string":
    default:
      next[field.key] = typeof rawValue === "string" ? rawValue : String(rawValue ?? "");
      return next;
  }
}

export function serializeSettingsObject(settings: SettingsObject): string {
  return JSON.stringify(settings, null, 2);
}
