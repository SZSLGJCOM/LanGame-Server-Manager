import type { ComponentType, ReactNode } from "react";
import type { TranslateFn } from "../../i18n";
import type { InstanceDetails, ModuleDetails } from "../../types";
import type {
  ConfigurationFieldPresentationOverride,
  ConfigurationSpecializedRendererContract,
  GuidedEditorVariant,
  GuidedFieldCopy,
  GuidedSettingsField,
  GuidedSettingsValidationIssue,
  GuidedSectionId,
  GuidedSettingsSection,
  ConfigurationPresentationField,
  SettingsObject
} from "./settings-schema";

export interface ConfigurationSpecializedRendererProps {
  sectionId: GuidedSectionId;
  active?: boolean;
  fieldKey?: string;
  details: InstanceDetails;
  moduleDetails: ModuleDetails;
  settings: SettingsObject;
  disabled: boolean;
  onPatch: (patch: Readonly<SettingsObject>) => void;
  onNavigateField?: (fieldKey: string) => void;
}

export interface ConfigurationSpecializedRendererRegistration {
  kind: "module-addon";
  placement?: "before-fields" | "after-fields";
  keepMounted?: boolean;
  saveMode?: "instance-settings" | "native-settings" | "explicit";
  sectionId: GuidedSectionId;
  fieldKey?: string;
  Renderer: ComponentType<ConfigurationSpecializedRendererProps>;
}

export type SettingsModuleSpecializedRenderer =
  | ConfigurationSpecializedRendererContract
  | ConfigurationSpecializedRendererRegistration;

export interface SettingsModuleFieldGroup {
  id: string;
  title?: string;
  description?: string;
  layoutClass?: string;
  fields: GuidedSettingsField[];
}

export interface SettingsModuleFieldValidationContext {
  field: GuidedSettingsField;
  value: unknown;
  settings: SettingsObject;
  locale: string;
  t: TranslateFn;
}

export interface SettingsModuleSettingsContext {
  locale: string;
  t: TranslateFn;
}

export interface ConfigurationWorkspaceToolsProps extends ConfigurationSpecializedRendererProps {
  schema: import("./settings-schema").GuidedSettingsSchema;
}

export interface ConfigurationWorkspaceProviderProps {
  children: ReactNode;
  details: InstanceDetails;
}

export interface SettingsModuleDefinition {
  id: string;
  workspaceProvider?: ComponentType<ConfigurationWorkspaceProviderProps>;
  workspaceToolbar?: ComponentType<ConfigurationWorkspaceToolsProps>;
  fieldPresentationOverrides?: Readonly<Record<string, ConfigurationFieldPresentationOverride>>;
  specializedRenderers?: Readonly<Record<string, SettingsModuleSpecializedRenderer>>;
  sections?: GuidedSettingsSection[];
  getSections?: (t: TranslateFn, locale?: string) => GuidedSettingsSection[];
  getAdditionalPresentationFields?: (
    context: SettingsModuleSettingsContext
  ) => readonly ConfigurationPresentationField[];
  buildFieldGroups?: (
    sectionId: GuidedSectionId,
    fields: GuidedSettingsField[],
    locale: string,
    t: TranslateFn
  ) => SettingsModuleFieldGroup[];
  resolveFieldSortWeight?: (key: string) => number;
  getFieldCopy?: (key: string, t: TranslateFn, locale?: string) => GuidedFieldCopy | undefined;
  getEnumOptionLabel?: (fieldKey: string, value: unknown, locale: string, t: TranslateFn) => string | undefined;
  resolveFieldEditorVariant?: (key: string) => GuidedEditorVariant | undefined;
  resolveFieldIcon?: (key: string) => string | undefined;
  getFieldValidationMessage?: (context: SettingsModuleFieldValidationContext) => string | undefined;
  isFieldDisabled?: (field: GuidedSettingsField, settings: Readonly<SettingsObject>) => boolean;
  initializeSettings?: (
    settings: Readonly<SettingsObject>,
    context: SettingsModuleSettingsContext
  ) => SettingsObject;
  applySettingsPatch?: (
    settings: Readonly<SettingsObject>,
    patch: Readonly<SettingsObject>,
    context: SettingsModuleSettingsContext
  ) => SettingsObject;
  getSettingsValidationIssues?: (
    settings: Readonly<SettingsObject>,
    context: SettingsModuleSettingsContext
  ) => GuidedSettingsValidationIssue[];
}
