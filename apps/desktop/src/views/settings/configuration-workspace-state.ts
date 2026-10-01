import { buildConfigurationFieldIds } from "./ConfigurationField";
import type {
  ConfigurationSectionNode,
  ConfigurationWorkspaceModel,
  SettingsObject
} from "./settings-schema";

export interface ConfigurationFocusRoot {
  getElementById(inputId: string): {
    focus(options?: FocusOptions): void;
    scrollIntoView?(options?: ScrollIntoViewOptions): void;
  } | null;
}

export interface ConfigurationFieldNavigation {
  sectionId: string;
  inputId: string;
}

export function mergeConfigurationPatch(
  current: SettingsObject,
  patch: Readonly<SettingsObject>
): SettingsObject {
  return { ...current, ...patch };
}

export function resolveConfigurationFieldNavigation(
  model: ConfigurationWorkspaceModel,
  fieldKey: string,
  idPrefix: string,
  exactKey = false
): ConfigurationFieldNavigation | null {
  const item = model.items.find((candidate) => candidate.fieldKey === fieldKey);
  if (
    !item ||
    item.owner !== "configuration" ||
    (item.state !== "editable" && item.state !== "specialized") ||
    !model.actionableSectionIds.includes(item.sectionId)
  ) {
    return null;
  }
  return {
    sectionId: item.sectionId,
    inputId: buildConfigurationFieldIds(item.fieldKey, idPrefix, exactKey).inputId
  };
}

export function focusConfigurationControl(
  inputId: string,
  root?: ConfigurationFocusRoot
): boolean {
  const resolvedRoot = root ?? (typeof document === "undefined" ? undefined : document);
  const control = resolvedRoot?.getElementById(inputId);
  if (!control) return false;
  // Focusing a textarea only guarantees that its caret is visible. Reveal the
  // editor itself so short workspaces do not leave it behind the save status.
  control.focus({ preventScroll: true });
  if (typeof HTMLElement !== "undefined" && control instanceof HTMLElement && control.hasAttribute("disabled")) {
    const field = control.closest<HTMLElement>(".configuration-field[tabindex]");
    field?.focus({ preventScroll: true });
  }
  control.scrollIntoView?.({ block: "nearest", inline: "nearest" });
  return true;
}

export function findConfigurationSectionNode(
  roots: readonly ConfigurationSectionNode[],
  sectionId: string | null
): ConfigurationSectionNode | null {
  if (!sectionId) return null;
  for (const node of roots) {
    if (node.id === sectionId) return node;
    const child = findConfigurationSectionNode(node.children, sectionId);
    if (child) return child;
  }
  return null;
}
