import type {
  ConfigurationBuiltInEditor,
  ConfigurationPresentationField,
  ConfigurationPresentationItem,
  ConfigurationSearchResult,
  ConfigurationSectionNode,
  ConfigurationWorkspaceModel,
  GuidedSectionId,
  GuidedSettingsSchema,
  GuidedSettingsSection
} from "./settings-schema";

interface IndexedSection {
  definition: GuidedSettingsSection;
  index: number;
  order: number;
}

interface IndexedField {
  field: ConfigurationPresentationField;
  index: number;
}

const BUILT_IN_EDITORS: Readonly<Partial<Record<GuidedSectionId, ConfigurationBuiltInEditor>>> = {
  network: "instance-network"
};

function compareIndexedSections(left: IndexedSection, right: IndexedSection): number {
  return left.order - right.order || left.index - right.index;
}

function compareIndexedFields(left: IndexedField, right: IndexedField): number {
  const leftWeight = left.field.sortWeight ?? Number.MAX_SAFE_INTEGER;
  const rightWeight = right.field.sortWeight ?? Number.MAX_SAFE_INTEGER;
  return leftWeight - rightWeight || left.index - right.index;
}

function indexSections(sections: readonly GuidedSettingsSection[]): Map<GuidedSectionId, IndexedSection> {
  const indexed = new Map<GuidedSectionId, IndexedSection>();
  sections.forEach((definition, index) => {
    if (!definition.id.trim()) {
      throw new Error("configuration section id must not be empty");
    }
    if (indexed.has(definition.id)) {
      throw new Error(`duplicate configuration section ${definition.id}`);
    }
    if (definition.order !== undefined && !Number.isFinite(definition.order)) {
      throw new Error(`configuration section ${definition.id} requires a finite order`);
    }
    indexed.set(definition.id, {
      definition,
      index,
      order: definition.order ?? index
    });
  });

  for (const { definition } of indexed.values()) {
    if (definition.parentId && !indexed.has(definition.parentId)) {
      throw new Error(`configuration section ${definition.id} has unknown parent ${definition.parentId}`);
    }
  }
  return indexed;
}

function validateAcyclicSections(sections: ReadonlyMap<GuidedSectionId, IndexedSection>): void {
  for (const id of sections.keys()) {
    const visited = new Set<GuidedSectionId>();
    let cursor: GuidedSectionId | undefined = id;
    while (cursor) {
      if (visited.has(cursor)) {
        throw new Error(`configuration section cycle includes ${cursor}`);
      }
      visited.add(cursor);
      cursor = sections.get(cursor)?.definition.parentId;
    }
  }
}

function groupFieldsBySection(
  fields: readonly ConfigurationPresentationField[],
  sections: ReadonlyMap<GuidedSectionId, IndexedSection>
): Map<GuidedSectionId, IndexedField[]> {
  const grouped = new Map<GuidedSectionId, IndexedField[]>();
  const fieldKeys = new Set<string>();
  fields.forEach((field, index) => {
    if (fieldKeys.has(field.key)) {
      throw new Error(`duplicate configuration field ${field.key}`);
    }
    fieldKeys.add(field.key);
    if (field.sectionId !== field.presentation.sectionId) {
      throw new Error(`field ${field.key} has conflicting section ids`);
    }
    if (!sections.has(field.sectionId)) {
      throw new Error(`field ${field.key} references unknown section ${field.sectionId}`);
    }
    const sectionFields = grouped.get(field.sectionId) ?? [];
    sectionFields.push({ field, index });
    grouped.set(field.sectionId, sectionFields);
  });
  for (const sectionFields of grouped.values()) {
    sectionFields.sort(compareIndexedFields);
  }
  return grouped;
}

function isActionableField(field: ConfigurationPresentationField): boolean {
  return field.presentation.owner === "configuration" &&
    (field.presentation.state === "editable" || field.presentation.state === "specialized");
}

function buildSectionNode(
  indexed: IndexedSection,
  breadcrumb: readonly string[],
  childSections: ReadonlyMap<GuidedSectionId | null, IndexedSection[]>,
  sectionFields: ReadonlyMap<GuidedSectionId, IndexedField[]>
): ConfigurationSectionNode | null {
  const definition = indexed.definition;
  const nextBreadcrumb = [...breadcrumb, definition.title];
  const items = (sectionFields.get(definition.id) ?? []).map(({ field }) => ({
    sectionId: definition.id,
    fieldKey: field.key,
    breadcrumb: nextBreadcrumb,
    owner: field.presentation.owner,
    state: field.presentation.state,
    field
  }));
  const children = (childSections.get(definition.id) ?? [])
    .map((child) => buildSectionNode(child, nextBreadcrumb, childSections, sectionFields))
    .filter((child): child is ConfigurationSectionNode => child !== null);
  const builtInEditor = BUILT_IN_EDITORS[definition.id];
  if (items.length === 0 && children.length === 0 && !builtInEditor) {
    return null;
  }

  return {
    id: definition.id,
    title: definition.title,
    description: definition.description,
    parentId: definition.parentId,
    order: indexed.order,
    icon: definition.icon,
    breadcrumb: nextBreadcrumb,
    items,
    children,
    actionable: Boolean(builtInEditor) || items.some(({ field }) => isActionableField(field)),
    builtInEditor
  };
}

function flattenNodes(roots: readonly ConfigurationSectionNode[]): ConfigurationSectionNode[] {
  return roots.flatMap((node) => [node, ...flattenNodes(node.children)]);
}

export function buildConfigurationWorkspaceModel(
  schema: GuidedSettingsSchema
): ConfigurationWorkspaceModel {
  const indexedSections = indexSections(schema.sections);
  validateAcyclicSections(indexedSections);
  const sectionFields = groupFieldsBySection(schema.presentationFields ?? schema.fields, indexedSections);
  const childSections = new Map<GuidedSectionId | null, IndexedSection[]>();
  for (const indexed of indexedSections.values()) {
    const parentId = indexed.definition.parentId ?? null;
    const children = childSections.get(parentId) ?? [];
    children.push(indexed);
    childSections.set(parentId, children);
  }
  for (const children of childSections.values()) {
    children.sort(compareIndexedSections);
  }

  const roots = (childSections.get(null) ?? [])
    .map((root) => buildSectionNode(root, [], childSections, sectionFields))
    .filter((root): root is ConfigurationSectionNode => root !== null);
  const nodes = flattenNodes(roots);
  return {
    roots,
    actionableSectionIds: nodes.filter((node) => node.actionable).map((node) => node.id),
    items: nodes.flatMap((node) => node.items)
  };
}

export function resolveConfigurationSectionId(
  model: ConfigurationWorkspaceModel,
  requestedId?: GuidedSectionId | null
): GuidedSectionId | null {
  if (requestedId && model.actionableSectionIds.includes(requestedId)) {
    return requestedId;
  }
  return model.actionableSectionIds[0] ?? null;
}

export function configurationNavigationRoots(
  nodes: readonly ConfigurationSectionNode[]
): ConfigurationSectionNode[] {
  return nodes.flatMap((node) => {
    const children = configurationNavigationRoots(node.children);
    const hasConfigurationContent = node.actionable ||
      node.items.some((item) => item.owner === "configuration");
    return hasConfigurationContent || children.length > 0 ? [{ ...node, children }] : [];
  });
}

function normalizeSearchText(value: string, locale: string): string {
  return value
    .normalize("NFKC")
    .toLocaleLowerCase(locale)
    .replace(/\p{White_Space}+/gu, " ")
    .trim();
}

function searchableValues(item: ConfigurationPresentationItem): string[] {
  return [
    item.field.title,
    item.field.description ?? "",
    item.field.sourceKey ?? "",
    item.field.key,
    ...item.breadcrumb,
    ...(item.field.presentation.aliases ?? []),
    item.field.presentation.reason ?? ""
  ];
}

export function searchConfigurationItems(
  model: ConfigurationWorkspaceModel,
  query: string,
  locale: string
): ConfigurationSearchResult[] {
  const normalizedQuery = normalizeSearchText(query, locale);
  if (!normalizedQuery) {
    return [];
  }
  const terms = normalizedQuery.split(" ");
  return model.items.flatMap((item) => {
    const corpus = normalizeSearchText(searchableValues(item).join(" "), locale);
    if (!terms.every((term) => corpus.includes(term))) {
      return [];
    }
    return [{
      sectionId: item.sectionId,
      fieldKey: item.fieldKey,
      title: item.field.title,
      description: item.field.description,
      sourceKey: item.field.sourceKey,
      aliases: item.field.presentation.aliases ?? [],
      reason: item.field.presentation.reason,
      breadcrumb: item.breadcrumb,
      owner: item.owner,
      state: item.state
    }];
  });
}
