export const CANONICAL_GAME_CONFIG_ACCEPTANCE_MODULE_IDS = [
  "abioticfactor",
  "arksurvivalascended",
  "arksurvivalevolved",
  "astroneer",
  "barotrauma",
  "conanexiles",
  "corekeeper",
  "dontstarve",
  "enshrouded",
  "humanitz",
  "minecraft",
  "necesse",
  "nightingale",
  "palworld",
  "projectzomboid",
  "returntomoria",
  "rimworld",
  "romestead",
  "runescapedragonwilds",
  "rust",
  "satisfactory",
  "scum",
  "sevendaystodie",
  "sonsoftheforest",
  "soulmask",
  "squad",
  "terraria",
  "theforest",
  "unturned",
  "valheim",
  "vrising",
  "windrose"
] as const;

type AcceptanceEnvironment = Readonly<Record<string, string | undefined>>;

export type GameConfigAcceptanceIdentityBinding = {
  ownerId: string;
  declaredId: string;
};

export type GameConfigAcceptanceRegistryInput = {
  canonicalModuleIds: readonly string[];
  bundledModuleIds: readonly string[];
  manifestBindings: readonly GameConfigAcceptanceIdentityBinding[];
  schemaRegistryBindings: readonly GameConfigAcceptanceIdentityBinding[];
  presentationRegistryIds: readonly string[];
  fixtureBindings: readonly GameConfigAcceptanceIdentityBinding[];
  environment?: AcceptanceEnvironment;
};

type SelectionInput = {
  canonicalModuleIds: readonly string[];
  fixtureModuleIds: readonly string[];
  environment?: AcceptanceEnvironment;
};

function assertUnique(ids: readonly string[], label: string): void {
  const seen = new Set<string>();
  for (const id of ids) {
    if (seen.has(id)) {
      throw new Error(`${label} contains duplicate module ID: ${id}`);
    }
    seen.add(id);
  }
}

function assertExactIds(actualIds: readonly string[], expectedIds: readonly string[], label: string): void {
  assertUnique(actualIds, label);
  const actual = new Set(actualIds);
  const expected = new Set(expectedIds);
  const missing = expectedIds.filter((id) => !actual.has(id));
  const unexpected = actualIds.filter((id) => !expected.has(id));
  if (missing.length > 0 || unexpected.length > 0) {
    throw new Error(
      `${label} must exactly match the canonical module IDs (missing: ${missing.join(", ") || "none"}; unexpected: ${unexpected.join(", ") || "none"})`
    );
  }
}

function assertExactCoverage(actualIds: readonly string[], expectedIds: readonly string[], label: string): void {
  const actual = new Set(actualIds);
  const expected = new Set(expectedIds);
  const missing = expectedIds.filter((id) => !actual.has(id));
  const unexpected = actualIds.filter((id) => !expected.has(id));
  if (missing.length > 0 || unexpected.length > 0) {
    throw new Error(
      `${label} must cover exactly the canonical module IDs (missing: ${missing.join(", ") || "none"}; unexpected: ${unexpected.join(", ") || "none"})`
    );
  }
}

function assertExactBindings(
  bindings: readonly GameConfigAcceptanceIdentityBinding[],
  expectedIds: readonly string[],
  label: string
): void {
  assertExactIds(bindings.map((binding) => binding.ownerId), expectedIds, `${label} owners`);
  assertExactIds(bindings.map((binding) => binding.declaredId), expectedIds, `${label} declarations`);
  const mismatches = bindings.filter((binding) => binding.ownerId !== binding.declaredId);
  if (mismatches.length > 0) {
    throw new Error(
      `${label} must preserve owner identity: ${mismatches
        .map((binding) => `${binding.ownerId}->${binding.declaredId}`)
        .join(", ")}`
    );
  }
}

function resolveFixtureModuleIds(
  bindings: readonly GameConfigAcceptanceIdentityBinding[],
  canonicalModuleIds: readonly string[]
): string[] {
  const ownerIds = bindings.map((binding) => binding.ownerId);
  const declaredIds = bindings.map((binding) => binding.declaredId);
  assertKnownIds(ownerIds, canonicalModuleIds, "fixture owners");
  assertKnownIds(declaredIds, canonicalModuleIds, "fixture declarations");
  const mismatches = bindings.filter((binding) => binding.ownerId !== binding.declaredId);
  if (mismatches.length > 0) {
    throw new Error(
      `fixture module IDs must match their module directories: ${mismatches
        .map((binding) => `${binding.ownerId}->${binding.declaredId}`)
        .join(", ")}`
    );
  }
  return [...new Set(ownerIds)];
}

function assertKnownIds(ids: readonly string[], canonicalModuleIds: readonly string[], label: string): void {
  const canonical = new Set(canonicalModuleIds);
  const unknown = ids.filter((id) => !canonical.has(id));
  if (unknown.length > 0) {
    throw new Error(`${label} contains unknown module IDs: ${unknown.join(", ")}`);
  }
}

function parseRequestedModules(value: string): string[] {
  if (value.trim() === "") {
    throw new Error("module selection must not be empty");
  }
  const requestedModules = value.split(",").map((id) => id.trim()).filter(Boolean);
  if (requestedModules.length === 0) {
    throw new Error("module selection must not be empty");
  }
  return requestedModules;
}

export function resolveGameConfigAcceptanceSelection({
  canonicalModuleIds,
  fixtureModuleIds,
  environment = {}
}: SelectionInput): string[] {
  assertUnique(canonicalModuleIds, "canonical module IDs");
  assertKnownIds(fixtureModuleIds, canonicalModuleIds, "fixture module IDs");

  const requestedModules = environment.GAME_CONFIG_ACCEPTANCE_MODULES;
  const selectedModuleIds = requestedModules === undefined
    ? (() => {
        assertExactCoverage(fixtureModuleIds, canonicalModuleIds, "game config acceptance fixtures");
        return [...canonicalModuleIds];
      })()
    : parseRequestedModules(requestedModules);

  assertUnique(selectedModuleIds, "selected acceptance module IDs");
  assertKnownIds(selectedModuleIds, canonicalModuleIds, "selected acceptance module IDs");

  if (environment.REQUIRE_ALL_GAME_CONFIG_ACCEPTANCE === "1") {
    assertExactCoverage(fixtureModuleIds, canonicalModuleIds, "game config acceptance fixtures");
    assertExactIds(selectedModuleIds, canonicalModuleIds, "required game config acceptance selection");
  } else {
    const fixtureIds = new Set(fixtureModuleIds);
    const missingFixtures = selectedModuleIds.filter((id) => !fixtureIds.has(id));
    if (missingFixtures.length > 0) {
      throw new Error(`selected acceptance module IDs have no fixtures: ${missingFixtures.join(", ")}`);
    }
  }

  return [...selectedModuleIds];
}

export function verifyGameConfigAcceptanceRegistry(input: GameConfigAcceptanceRegistryInput): string[] {
  const { canonicalModuleIds } = input;
  assertUnique(canonicalModuleIds, "canonical module IDs");
  assertExactIds(input.bundledModuleIds, canonicalModuleIds, "bundled module directories");
  assertExactBindings(input.manifestBindings, canonicalModuleIds, "manifest bindings");
  assertExactBindings(input.schemaRegistryBindings, canonicalModuleIds, "schema registry bindings");
  assertExactIds(input.presentationRegistryIds, canonicalModuleIds, "presentation registry IDs");
  return resolveGameConfigAcceptanceSelection({
    canonicalModuleIds,
    fixtureModuleIds: resolveFixtureModuleIds(input.fixtureBindings, canonicalModuleIds),
    environment: input.environment
  });
}
