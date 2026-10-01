import type {
  SettingsModuleDefinition,
  SettingsModuleSpecializedRenderer
} from "./module-types";
import type {
  ConfigurationFieldBehavior,
  ConfigurationFieldPresentation,
  ConfigurationSectionDefinition,
  ConfigurationSpecializedRendererContract
} from "./settings-schema";

type SchemaProperty = Readonly<Record<string, unknown>>;

export interface ValidateConfigurationPresentationInput {
  definition: SettingsModuleDefinition;
  properties: Readonly<Record<string, unknown>>;
  sections: readonly ConfigurationSectionDefinition[];
}

const PLAYER_ACCESS_RENDERER_ID = "player-access-roster";
const BUILT_IN_SPECIALIZED_RENDERERS: Readonly<Record<string, ConfigurationSpecializedRendererContract>> = {
  [PLAYER_ACCESS_RENDERER_ID]: {
    kind: "workspace",
    workspace: "player_access"
  }
};

function isRecord(value: unknown): value is SchemaProperty {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function readNonEmptyString(value: unknown): string | undefined {
  return typeof value === "string" && value.trim().length > 0 ? value.trim() : undefined;
}

function isScalarProperty(property: SchemaProperty): boolean {
  const types = Array.isArray(property.type)
    ? property.type.filter((type) => type !== "null")
    : [property.type];
  return types.length === 1 &&
    (types[0] === "string" || types[0] === "integer" || types[0] === "number" || types[0] === "boolean");
}

function resolveBehavior(property: SchemaProperty): ConfigurationFieldBehavior | undefined {
  return property.format === "textarea" ? "multiline" : undefined;
}

function withBehavior(
  presentation: ConfigurationFieldPresentation,
  property: SchemaProperty
): ConfigurationFieldPresentation {
  const behavior = resolveBehavior(property);
  return behavior ? { ...presentation, behavior } : presentation;
}

function applyPresentationOverride(
  presentation: ConfigurationFieldPresentation,
  override: Partial<ConfigurationFieldPresentation>
): ConfigurationFieldPresentation {
  const transitioned = override.state && override.state !== presentation.state
    ? { ...presentation, reason: undefined, rendererId: undefined, rendererFieldKey: undefined }
    : presentation;
  const resolved = { ...transitioned, ...override };
  if (resolved.state === "specialized") {
    const { reason: _reason, ...specialized } = resolved;
    return specialized;
  }
  if (resolved.state === "editable") {
    const { reason: _reason, rendererId: _rendererId, rendererFieldKey: _rendererFieldKey, ...editable } = resolved;
    return editable;
  }
  const { rendererId: _rendererId, rendererFieldKey: _rendererFieldKey, ...nonEditable } = resolved;
  return nonEditable;
}

export function resolveConfigurationFieldPresentation(
  fieldKey: string,
  property: SchemaProperty,
  definition?: SettingsModuleDefinition | null
): ConfigurationFieldPresentation {
  const override = definition?.fieldPresentationOverrides?.[fieldKey];
  const sectionId = readNonEmptyString(property["x-lsgm-section"]) ?? "advanced";
  let presentation: ConfigurationFieldPresentation;
  if (fieldKey === "bind_ip") {
    presentation = {
      state: "derived",
      owner: "instance_network",
      sectionId,
      reason: "Listener bind address is managed by the instance network editor."
    };
  } else if (readNonEmptyString(property["x-lsgm-player-access-kind"])) {
    presentation = {
      state: "specialized",
      owner: "player_access",
      sectionId,
      rendererId: PLAYER_ACCESS_RENDERER_ID
    };
  } else if (!isScalarProperty(property)) {
    presentation = {
      state: "excluded",
      owner: "configuration",
      sectionId,
      reason: "No renderer is registered for this non-scalar schema field."
    };
  } else {
    presentation = withBehavior({
      state: "editable",
      owner: "configuration",
      sectionId
    }, property);
  }

  return override ? applyPresentationOverride(presentation, override) : presentation;
}

export function resolveConfigurationRendererContract(
  rendererId: string,
  definition?: SettingsModuleDefinition | null
): SettingsModuleSpecializedRenderer | undefined {
  return BUILT_IN_SPECIALIZED_RENDERERS[rendererId] ?? definition?.specializedRenderers?.[rendererId];
}

function validateSectionGraph(sections: readonly ConfigurationSectionDefinition[]): Set<string> {
  const sectionIds = new Set<string>();
  for (const section of sections) {
    if (!section.id.trim()) {
      throw new Error("configuration section id must not be empty");
    }
    if (sectionIds.has(section.id)) {
      throw new Error(`duplicate configuration section ${section.id}`);
    }
    if (!Number.isFinite(section.order)) {
      throw new Error(`configuration section ${section.id} requires a finite order`);
    }
    sectionIds.add(section.id);
  }

  for (const section of sections) {
    if (section.parentId && !sectionIds.has(section.parentId)) {
      throw new Error(`configuration section ${section.id} has unknown parent ${section.parentId}`);
    }
  }

  const parents = new Map(sections.map((section) => [section.id, section.parentId]));
  for (const section of sections) {
    const visited = new Set<string>();
    let cursor: string | undefined = section.id;
    while (cursor) {
      if (visited.has(cursor)) {
        throw new Error(`configuration section cycle includes ${cursor}`);
      }
      visited.add(cursor);
      cursor = parents.get(cursor);
    }
  }

  return sectionIds;
}

function validatePresentation(
  moduleId: string,
  fieldKey: string,
  presentation: ConfigurationFieldPresentation,
  sectionIds: ReadonlySet<string>,
  definition: SettingsModuleDefinition,
  property: SchemaProperty
): void {
  if (!sectionIds.has(presentation.sectionId)) {
    throw new Error(`${moduleId}.${fieldKey} references unknown section ${presentation.sectionId}`);
  }
  if (presentation.state === "editable" && !isScalarProperty(property)) {
    throw new Error(`${moduleId}.${fieldKey} non-scalar presentation requires a specialized renderer`);
  }
  if (presentation.state === "specialized") {
    if (!presentation.rendererId) {
      throw new Error(`${moduleId}.${fieldKey} specialized presentation requires rendererId`);
    }
    const renderer = resolveConfigurationRendererContract(presentation.rendererId, definition);
    if (!renderer) {
      throw new Error(`${moduleId}.${fieldKey} references unregistered renderer ${presentation.rendererId}`);
    }
    if (renderer.kind === "module-addon") {
      if (!("Renderer" in renderer) || typeof renderer.Renderer !== "function") {
        throw new Error(`${moduleId}.${fieldKey} renderer ${presentation.rendererId} requires a registered renderer`);
      }
      if (renderer.sectionId !== presentation.sectionId) {
        throw new Error(`${moduleId}.${fieldKey} renderer ${presentation.rendererId} must use section ${presentation.sectionId}`);
      }
      if (renderer.fieldKey && renderer.fieldKey !== fieldKey) {
        throw new Error(`${moduleId}.${fieldKey} renderer ${presentation.rendererId} must own field ${fieldKey}`);
      }
    }
    if (renderer.kind === "workspace" && renderer.workspace !== presentation.owner) {
      throw new Error(`${moduleId}.${fieldKey} renderer workspace must match owner ${presentation.owner}`);
    }
    if (renderer.kind !== "workspace" && presentation.owner !== "configuration") {
      throw new Error(`${moduleId}.${fieldKey} ${renderer.kind} renderer must be owned by Configuration`);
    }
  }
  if (
    (presentation.state === "derived" ||
      presentation.state === "generated" ||
      presentation.state === "excluded") &&
    !presentation.reason?.trim()
  ) {
    throw new Error(`${moduleId}.${fieldKey} ${presentation.state} presentation requires a reason`);
  }
}

export function validateConfigurationPresentationDefinition(
  input: ValidateConfigurationPresentationInput
): Record<string, ConfigurationFieldPresentation> {
  const sectionIds = validateSectionGraph(input.sections);
  const propertyKeys = new Set(Object.keys(input.properties));

  for (const overrideKey of Object.keys(input.definition.fieldPresentationOverrides ?? {})) {
    if (!propertyKeys.has(overrideKey)) {
      throw new Error(`${input.definition.id} presentation override references unknown property ${overrideKey}`);
    }
  }

  const presentations: Record<string, ConfigurationFieldPresentation> = {};
  for (const [fieldKey, property] of Object.entries(input.properties)) {
    if (!isRecord(property)) {
      throw new Error(`${input.definition.id}.${fieldKey} schema property must be an object`);
    }
    const presentation = resolveConfigurationFieldPresentation(fieldKey, property, input.definition);
    validatePresentation(input.definition.id, fieldKey, presentation, sectionIds, input.definition, property);
    presentations[fieldKey] = presentation;
  }

  for (const [rendererId, renderer] of Object.entries(input.definition.specializedRenderers ?? {})) {
    if (renderer.kind !== "module-addon") continue;
    if (!sectionIds.has(renderer.sectionId)) {
      throw new Error(`${input.definition.id} renderer ${rendererId} references unknown section ${renderer.sectionId}`);
    }
    if (!renderer.fieldKey) continue;
    const presentation = presentations[renderer.fieldKey];
    if (!presentation) {
      throw new Error(`${input.definition.id} renderer ${rendererId} references unknown property ${renderer.fieldKey}`);
    }
    if (presentation.state !== "specialized" || presentation.rendererId !== rendererId) {
      throw new Error(`${input.definition.id} renderer ${rendererId} does not own specialized field ${renderer.fieldKey}`);
    }
  }

  return presentations;
}
