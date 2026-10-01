import type { ShellIconName } from "../../components/ShellIcon";

export type SettingsObject = Record<string, unknown>;
export type GuidedSectionId = string;
export const GUIDED_CORE_SECTION_IDS = ["room", "network", "access", "runtime"] as const;
export type GuidedCoreSectionId = (typeof GUIDED_CORE_SECTION_IDS)[number];
export type GuidedFieldType = "string" | "integer" | "number" | "boolean";
export type GuidedControlType = "text" | "textarea" | "password" | "number" | "select" | "checkbox";
export type GuidedEditorVariant = "workshop-id-list" | "enum-check-list" | "string-list";
export type GuidedDefaultSource = "instance_id" | "instance_name";

export type ConfigurationSpecializedRendererContract =
  | {
      kind: "guided-field";
      editorVariant: GuidedEditorVariant;
    }
  | {
      kind: "workspace";
      workspace: "mods" | "player_access";
    };

export interface GuidedEnumOption {
  label: string;
  value: unknown;
}

export interface GuidedFieldCopy {
  title: string;
  description?: string;
}

export interface GuidedSettingsSection {
  id: GuidedSectionId;
  title: string;
  description?: string | null;
  emptyHint?: string | null;
  showWhenEmpty?: boolean;
  parentId?: GuidedSectionId;
  order?: number;
  icon?: ShellIconName;
}

export type ConfigurationPresentationState =
  | "editable"
  | "specialized"
  | "derived"
  | "generated"
  | "excluded";

export type ConfigurationOwner =
  | "configuration"
  | "maintenance"
  | "mods"
  | "player_access"
  | "instance_network";

export type ConfigurationFieldBehavior =
  | "plain"
  | "multiline"
  | "path"
  | "secret"
  | "raw";

export interface ConfigurationFieldPresentation {
  state: ConfigurationPresentationState;
  owner: ConfigurationOwner;
  sectionId: GuidedSectionId;
  behavior?: ConfigurationFieldBehavior;
  rendererId?: string;
  rendererFieldKey?: string;
  aliases?: readonly string[];
  restartScope?: "none" | "server" | "world" | "cluster";
  resourceUrl?: string;
  reason?: string;
}

export type ConfigurationFieldPresentationOverride = Partial<ConfigurationFieldPresentation>;

export interface ConfigurationSectionDefinition extends GuidedSettingsSection {
  order: number;
}

export interface ConfigurationPresentationField {
  key: string;
  title: string;
  description?: string | null;
  sectionId: GuidedSectionId;
  sortWeight?: number;
  sourceId?: string | null;
  sourceKey?: string | null;
  sourceSurface?: string | null;
  icon?: string | null;
  presentation: ConfigurationFieldPresentation;
}

export interface GuidedSettingsField extends ConfigurationPresentationField {
  type: GuidedFieldType;
  control: GuidedControlType;
  editorVariant?: GuidedEditorVariant;
  required: boolean;
  defaultValue?: unknown;
  defaultSource?: GuidedDefaultSource;
  preserveNativeWhenUnset?: boolean;
  enumOptions?: GuidedEnumOption[];
  suggestions?: GuidedEnumOption[];
  minLength?: number;
  maxLength?: number;
  pattern?: string;
  disallowedLinePrefixes?: readonly string[];
  minimum?: number;
  maximum?: number;
  step?: number;
}

export interface GuidedFieldDefaultContext {
  instanceId?: string | null;
  instanceName?: string | null;
}

export interface GuidedFieldChange {
  field: GuidedSettingsField;
  value: unknown;
}

export interface GuidedSettingsValidationIssue {
  fieldKey: string;
  reason: string;
  message: string;
}

export interface GuidedSettingsSchema {
  title: string;
  sections: GuidedSettingsSection[];
  fields: GuidedSettingsField[];
  presentationFields?: ConfigurationPresentationField[];
  parseError?: string | null;
}

export type ConfigurationBuiltInEditor = "instance-network";

export interface ConfigurationPresentationItem {
  sectionId: GuidedSectionId;
  fieldKey: string;
  breadcrumb: readonly string[];
  owner: ConfigurationOwner;
  state: ConfigurationPresentationState;
  field: ConfigurationPresentationField;
}

export interface ConfigurationSectionNode {
  id: GuidedSectionId;
  title: string;
  description?: string | null;
  parentId?: GuidedSectionId;
  order: number;
  icon?: ShellIconName;
  breadcrumb: readonly string[];
  items: ConfigurationPresentationItem[];
  children: ConfigurationSectionNode[];
  actionable: boolean;
  builtInEditor?: ConfigurationBuiltInEditor;
}

export interface ConfigurationWorkspaceModel {
  roots: ConfigurationSectionNode[];
  actionableSectionIds: GuidedSectionId[];
  items: ConfigurationPresentationItem[];
}

export interface ConfigurationSearchResult {
  sectionId: GuidedSectionId;
  fieldKey: string;
  title: string;
  description?: string | null;
  sourceKey?: string | null;
  aliases: readonly string[];
  reason?: string;
  breadcrumb: readonly string[];
  owner: ConfigurationOwner;
  state: ConfigurationPresentationState;
}
