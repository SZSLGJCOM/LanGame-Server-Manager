import { buildConfigurationFieldIds } from "./ConfigurationField";
import type {
  ConfigurationSectionNode,
  ConfigurationWorkspaceModel,
  GuidedSettingsValidationIssue,
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

export function groupConfigurationValidationIssues(issues: readonly GuidedSettingsValidationIssue[]) {
  const groups = new Map<string, { message: string; fieldKeys: string[] }>();
  for (const issue of issues) {
    let group = groups.get(issue.message);
    if (!group) {
      group = { message: issue.message, fieldKeys: [] };
      groups.set(issue.message, group);
    }
    if (!group.fieldKeys.includes(issue.fieldKey)) group.fieldKeys.push(issue.fieldKey);
  }
  return [...groups.values()];
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
  let focusTarget = control;
  if (typeof HTMLElement !== "undefined" && control instanceof HTMLElement) {
    // A failed draft can remain in a collapsed native/CSV editor.
    for (let ancestor: HTMLElement | null = control; ancestor; ancestor = ancestor.parentElement) {
      if (ancestor instanceof HTMLDetailsElement) ancestor.open = true;
    }
    // Runtime locks may disable an invalid draft after its notice was published.
    // Focus its visible field/editor so the activity panel can close normally.
    if (control.hasAttribute("disabled")) {
      const field = control.closest<HTMLElement>(".configuration-field, .settings-schema-field, .configuration-workspace__addon, .ark-editor");
      if (!field) return false;
      if (!field.hasAttribute("tabindex")) field.tabIndex = -1;
      focusTarget = field;
    }
  }
  // Focusing a textarea only guarantees that its caret is visible. Reveal the
  // editor itself so short workspaces do not leave it behind the save status.
  focusTarget.focus({ preventScroll: true });
  control.scrollIntoView?.({ block: "nearest", inline: "nearest" });
  return typeof HTMLElement === "undefined" || !(focusTarget instanceof HTMLElement)
    || focusTarget.ownerDocument.activeElement === focusTarget;
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
