import type { ModuleDetails } from "../../types";
import { useI18n } from "../../i18n";
import { ConfigurationField, type ConfigurationFieldCopy } from "./ConfigurationField";
import { readGuidedFieldValue } from "./guided-settings";
import { resolveSettingsModuleDefinition } from "./module-registry";
import type {
  GuidedFieldChange,
  GuidedFieldDefaultContext,
  GuidedSettingsField,
  GuidedSettingsSchema,
  GuidedSettingsValidationIssue,
  SettingsObject
} from "./settings-schema";

interface GuidedSettingsFormProps {
  schema: GuidedSettingsSchema;
  settings: SettingsObject;
  moduleDetails?: ModuleDetails | null;
  selectedSectionId?: string | null;
  disabled?: boolean;
  readOnly?: boolean;
  idPrefix?: string;
  defaultContext?: GuidedFieldDefaultContext;
  validationIssues?: GuidedSettingsValidationIssue[];
  onChange: (field: GuidedSettingsField, value: unknown) => void;
  onBatchChange: (changes: GuidedFieldChange[]) => void;
}

const DST_HALF_WIDTH_TEXTAREA_FIELDS = new Set([
  "admin_list",
  "whitelist",
  "blocklist",
  "master_world_overrides_extra",
  "master_worldgenoverride_lua",
  "caves_world_overrides_extra",
  "caves_worldgenoverride_lua",
  "master_modoverrides_lua",
  "caves_modoverrides_lua"
]);

function shouldUseFullWidth(field: GuidedSettingsField, moduleId?: string | null): boolean {
  if (field.editorVariant || field.control === "textarea") {
    return !(moduleId === "dontstarve" && DST_HALF_WIDTH_TEXTAREA_FIELDS.has(field.key));
  }
  return false;
}

function sanitizeClassSegment(value: string): string {
  return value.replace(/[^a-z0-9]+/gi, "-").replace(/^-+|-+$/g, "").toLowerCase();
}

function fieldClassName(field: GuidedSettingsField, isFullWidth: boolean): string {
  const classes = [`settings-schema-field--field-${sanitizeClassSegment(field.key)}`];
  if (field.icon && field.control !== "checkbox") classes.push("settings-schema-field--with-icon");
  if (isFullWidth) classes.push("settings-schema-field--full");
  return classes.filter(Boolean).join(" ");
}

export function buildConfigurationFieldCopy(t: ReturnType<typeof useI18n>["t"]): ConfigurationFieldCopy {
  return {
    concealSecret: t("settings.configuration.secret.hide", undefined, "Hide value"),
    revealSecret: t("settings.configuration.secret.show", undefined, "Show value"),
    restartScopes: {
      none: t("settings.configuration.restart.none", undefined, "No restart required"),
      server: t("settings.configuration.restart.server", undefined, "Restart server"),
      world: t("settings.configuration.restart.world", undefined, "Restart world"),
      cluster: t("settings.configuration.restart.cluster", undefined, "Restart cluster")
    },
    showSuggestions: t("settings.guided.suggestions.toggle", undefined, "Show suggestions"),
    notSaved: t("servers.archives.configuration.notSaved", undefined, "Not saved"),
    useGameDefault: t("settings.configuration.useGameDefault", undefined, "Use game default"),
    preserveNativeWhenUnset: t("settings.configuration.preserveNativeWhenUnset", undefined,
      "Keep native setting (current value not read)"),
    enabled: t("common.enabled", undefined, "Enabled"),
    disabled: t("common.disabled", undefined, "Disabled"),
    specializedUnavailable: t(
      "settings.configuration.specialized.unavailable",
      undefined,
      "A specialized editor is unavailable."
    )
  };
}

export function GuidedSettingsForm(props: GuidedSettingsFormProps) {
  const { locale, t } = useI18n();
  const moduleId = props.moduleDetails?.summary.id ?? null;
  const isDstModule = moduleId === "dontstarve";
  const moduleDefinition = resolveSettingsModuleDefinition(moduleId);
  const copy = buildConfigurationFieldCopy(t);
  const sections = props.schema.sections
    .map((section) => ({
      ...section,
      fields: props.schema.fields.filter((field) => {
        const renderer = field.presentation.rendererId
          ? moduleDefinition?.specializedRenderers?.[field.presentation.rendererId]
          : undefined;
        return field.sectionId === section.id &&
          (props.readOnly || !(field.presentation.state === "specialized" && renderer?.kind === "module-addon"));
      })
    }))
    .filter((section) => {
      if (!section.showWhenEmpty && section.fields.length === 0) return false;
      return !props.selectedSectionId || section.id === props.selectedSectionId;
    });

  function applyPatch(field: GuidedSettingsField, patch: SettingsObject) {
    if (props.readOnly) return;
    const changes = Object.entries(patch).flatMap(([key, value]) => {
      const target = props.schema.fields.find((candidate) => candidate.key === key);
      return target ? [{ field: target, value }] : [];
    });
    if (changes.length === 1 && changes[0].field.key === field.key) {
      props.onChange(field, changes[0].value);
    } else if (changes.length > 0) {
      props.onBatchChange(changes);
    }
  }

  function renderField(field: GuidedSettingsField) {
    const value = props.readOnly ? props.settings[field.key] : readGuidedFieldValue(field, props.settings, props.defaultContext);
    const schemaValidationMessage = props.validationIssues?.find((issue) => issue.fieldKey === field.key)?.message;
    const moduleMessage = props.readOnly ? undefined : moduleDefinition?.getFieldValidationMessage?.({
      field,
      value,
      settings: props.settings,
      locale,
      t
    });

    return (
      <ConfigurationField
        key={field.key}
        className={fieldClassName(field, shouldUseFullWidth(field, moduleId))}
        copy={copy}
        disabled={props.disabled || (!props.readOnly && moduleDefinition?.isFieldDisabled?.(field, props.settings))}
        readOnly={props.readOnly}
        field={field}
        idPrefix={props.idPrefix ?? `configuration-${moduleId ?? "module"}`}
        onPatch={(patch) => applyPatch(field, patch)}
        settings={props.settings}
        t={t}
        validationMessage={schemaValidationMessage ?? moduleMessage}
        value={value}
      />
    );
  }

  return (
    <div className="settings-guided-sections">
      {sections.map((section) => {
        const groups = moduleDefinition?.buildFieldGroups?.(section.id, section.fields, locale, t) ?? [];
        const sectionClassName = isDstModule
          ? `settings-schema-section settings-schema-section--dst settings-schema-section--${sanitizeClassSegment(section.id)}`
          : "settings-schema-section";
        return (
          <section key={section.id} className={sectionClassName}>
            {!props.selectedSectionId ? (
              <div className="settings-schema-section-head">
                <h4 className="settings-section-title">{section.title}</h4>
              </div>
            ) : null}

            {section.fields.length === 0 ? (
              <div className="form-note">{section.emptyHint ?? t("settings.guided.unavailable")}</div>
            ) : groups.length > 0 ? (
              <div className={isDstModule ? "dst-guided-groups" : "guided-field-groups"}>
                {groups.map((group) => {
                  const groupName = sanitizeClassSegment(group.layoutClass ?? group.id);
                  const groupPrefix = isDstModule ? "dst-guided-group" : "guided-field-group";
                  return (
                    <section key={`${section.id}:${group.id}`} className={`${groupPrefix} ${groupPrefix}--${groupName}`}>
                      {group.title && group.title.trim().toLocaleLowerCase(locale) !== section.title.trim().toLocaleLowerCase(locale) ? (
                        <div className={`${groupPrefix}-head`}>
                          <h5 className={`${groupPrefix}-title`}>{group.title}</h5>
                        </div>
                      ) : null}
                      <div className={`settings-schema-grid configuration-field-grid ${groupPrefix}-grid ${groupPrefix}-grid--${groupName}`}>
                        {group.fields.map(renderField)}
                      </div>
                    </section>
                  );
                })}
              </div>
            ) : (
              <div className="settings-schema-grid configuration-field-grid">{section.fields.map(renderField)}</div>
            )}
          </section>
        );
      })}
    </div>
  );
}
