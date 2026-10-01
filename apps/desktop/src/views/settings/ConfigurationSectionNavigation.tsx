import { useEffect, useId, useRef, useState } from "react";
import { ShellIcon } from "../../components/ShellIcon";
import { useConfigurationFieldHelp } from "./ConfigurationFieldHelp";
import type {
  ConfigurationSectionNode,
  GuidedSectionId
} from "./settings-schema";

export interface ConfigurationSectionNavigationProps {
  roots: readonly ConfigurationSectionNode[];
  selectedSectionId: GuidedSectionId | null;
  ariaLabel: string;
  onSelectSection: (sectionId: GuidedSectionId) => void;
  emptyLabel?: string;
}

interface ConfigurationNavigationBranchProps {
  node: ConfigurationSectionNode;
  selectedSectionId: GuidedSectionId | null;
  expandedIds: ReadonlySet<GuidedSectionId>;
  onSelectSection: (sectionId: GuidedSectionId) => void;
  onToggleSection: (sectionId: GuidedSectionId) => void;
}

function findConfigurationNavigationAncestors(
  nodes: readonly ConfigurationSectionNode[],
  targetId: GuidedSectionId,
  ancestors: readonly GuidedSectionId[] = []
): readonly GuidedSectionId[] | null {
  for (const node of nodes) {
    if (node.id === targetId) return ancestors;
    const match = findConfigurationNavigationAncestors(
      node.children,
      targetId,
      [...ancestors, node.id]
    );
    if (match) return match;
  }
  return null;
}

function collectConfigurationNavigationBranchIds(
  nodes: readonly ConfigurationSectionNode[],
  result = new Set<GuidedSectionId>()
): Set<GuidedSectionId> {
  for (const node of nodes) {
    if (node.children.length > 0) {
      result.add(node.id);
      collectConfigurationNavigationBranchIds(node.children, result);
    }
  }
  return result;
}

export function collectConfigurationNavigationAncestors(
  roots: readonly ConfigurationSectionNode[],
  selectedSectionId: GuidedSectionId | null
): Set<GuidedSectionId> {
  if (!selectedSectionId) return new Set();
  return new Set(findConfigurationNavigationAncestors(roots, selectedSectionId) ?? []);
}

export function reconcileConfigurationNavigationExpansion(
  roots: readonly ConfigurationSectionNode[],
  current: ReadonlySet<GuidedSectionId>,
  selectedSectionId: GuidedSectionId | null
): Set<GuidedSectionId> {
  const branchIds = collectConfigurationNavigationBranchIds(roots);
  const next = new Set([...current].filter((id) => branchIds.has(id)));
  for (const id of collectConfigurationNavigationAncestors(roots, selectedSectionId)) {
    next.add(id);
  }
  return next;
}

export function toggleConfigurationNavigationExpansion(
  current: ReadonlySet<GuidedSectionId>,
  sectionId: GuidedSectionId
): Set<GuidedSectionId> {
  const next = new Set(current);
  if (next.has(sectionId)) next.delete(sectionId);
  else next.add(sectionId);
  return next;
}

function navigationChildListId(sectionId: GuidedSectionId): string {
  return `configuration-navigation-${encodeURIComponent(sectionId).replace(/%/g, "-")}-children`;
}

function ConfigurationNavigationBranch(props: ConfigurationNavigationBranchProps) {
  const { node } = props;
  const titleElement = useRef<HTMLSpanElement>(null);
  const hasDescription = Boolean(node.description && node.description !== node.title);
  const help = useConfigurationFieldHelp(useId(), hasDescription ? node.description : node.title,
    undefined, undefined, "instructions", () => hasDescription || Boolean(titleElement.current
      && titleElement.current.scrollWidth > titleElement.current.clientWidth));
  const active = node.id === props.selectedSectionId;
  const hasChildren = node.children.length > 0;
  const expanded = hasChildren && props.expandedIds.has(node.id);
  const activeAncestor = hasChildren && collectConfigurationNavigationAncestors(
    [node],
    props.selectedSectionId
  ).has(node.id);
  const childListId = hasChildren ? navigationChildListId(node.id) : undefined;
  const selectNode = () => props.onSelectSection(node.id);
  const toggleNode = () => props.onToggleSection(node.id);
  const labelContent = (
    <>
      {node.icon ? (
        <ShellIcon name={node.icon} className="configuration-section-navigation__icon" />
      ) : (
        <span className="configuration-section-navigation__marker" aria-hidden="true" />
      )}
      <span ref={titleElement} className="configuration-section-navigation__title">{node.title}</span>
      {hasChildren && !node.actionable ? (
        <ShellIcon
          name="chevron-right"
          className="configuration-section-navigation__branch-icon"
        />
      ) : null}
    </>
  );

  return (
    <li
      className="configuration-section-navigation__item"
      data-configuration-section-id={node.id}
    >
      {node.actionable ? (
        <button
          type="button"
          className={[
            "configuration-section-navigation__button",
            active ? "is-active" : "",
            activeAncestor ? "is-active-ancestor" : ""
          ].filter(Boolean).join(" ")}
          aria-current={active ? "page" : undefined}
          ref={help.anchorRef} {...help.interactionProps} aria-describedby={help.descriptionId}
          onClick={selectNode}
        >
          {labelContent}
        </button>
      ) : hasChildren ? (
        <button
          type="button"
          className={[
            "configuration-section-navigation__group-label",
            "configuration-section-navigation__group-label--disclosure",
            expanded ? "is-expanded" : "",
            activeAncestor ? "is-active-ancestor" : ""
          ].filter(Boolean).join(" ")}
          aria-expanded={expanded}
          aria-controls={childListId}
          ref={help.anchorRef} {...help.interactionProps} aria-describedby={help.descriptionId}
          onClick={toggleNode}
        >
          {labelContent}
        </button>
      ) : (
        <div
          className="configuration-section-navigation__group-label"
          ref={help.anchorRef} {...help.interactionProps} aria-describedby={help.descriptionId}
          tabIndex={help.descriptionId ? 0 : undefined}
        >
          {labelContent}
        </div>
      )}

      {help.helpNode}
      {node.actionable && hasChildren ? (
        <button type="button"
          className={[
            "configuration-section-navigation__disclosure",
            expanded ? "is-expanded" : "",
            activeAncestor ? "is-active-ancestor" : ""
          ].filter(Boolean).join(" ")}
          aria-label={node.title}
          aria-expanded={expanded}
          aria-controls={childListId}
          onClick={toggleNode}>
          <ShellIcon name="chevron-right" className="configuration-section-navigation__branch-icon" />
        </button>
      ) : null}

      {expanded ? (
        <ul id={childListId}
          className="configuration-section-navigation__list configuration-section-navigation__list--nested">
          {node.children.map((child) => (
            <ConfigurationNavigationBranch
              key={child.id}
              node={child}
              selectedSectionId={props.selectedSectionId}
              expandedIds={props.expandedIds}
              onSelectSection={props.onSelectSection}
              onToggleSection={props.onToggleSection}
            />
          ))}
        </ul>
      ) : null}
    </li>
  );
}

export function ConfigurationSectionNavigation(props: ConfigurationSectionNavigationProps) {
  const [expandedIds, setExpandedIds] = useState(() =>
    reconcileConfigurationNavigationExpansion(props.roots, new Set(), props.selectedSectionId)
  );

  useEffect(() => {
    setExpandedIds((current) =>
      reconcileConfigurationNavigationExpansion(props.roots, current, props.selectedSectionId)
    );
  }, [props.roots, props.selectedSectionId]);

  const toggleSection = (sectionId: GuidedSectionId) => {
    setExpandedIds((current) => toggleConfigurationNavigationExpansion(current, sectionId));
  };

  return (
    <nav className="configuration-section-navigation" aria-label={props.ariaLabel}>
      {props.roots.length > 0 ? (
        <ul className="configuration-section-navigation__list configuration-section-navigation__list--root">
          {props.roots.map((root) => (
            <ConfigurationNavigationBranch
              key={root.id}
              node={root}
              selectedSectionId={props.selectedSectionId}
              expandedIds={expandedIds}
              onSelectSection={props.onSelectSection}
              onToggleSection={toggleSection}
            />
          ))}
        </ul>
      ) : props.emptyLabel ? (
        <p className="configuration-section-navigation__empty" role="status">
          {props.emptyLabel}
        </p>
      ) : null}
    </nav>
  );
}
