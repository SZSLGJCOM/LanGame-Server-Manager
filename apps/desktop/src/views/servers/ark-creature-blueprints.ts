// These base-game assets are shared by ASE and ASA. Full paths allow spawning
// even when the current map has not loaded the corresponding creature class.
const BASE_CREATURE_BLUEPRINTS: Record<string, string> = {
  Rex_Character_BP_C: "/Game/PrimalEarth/Dinos/Rex/Rex_Character_BP.Rex_Character_BP_C",
  Dodo_Character_BP_C: "/Game/PrimalEarth/Dinos/Dodo/Dodo_Character_BP.Dodo_Character_BP_C"
};

const ASA_CREATURE_BLUEPRINTS: Record<string, string> = {
  // Official build 26477280 UFS asset manifest; native verification uses the
  // independent enigmasaur_readback receipt in commands_native_ark_creatures.
  Chameleon_Character_BP_C: "/Game/ASA/Dinos/Chameleon/Chameleon_Character_BP.Chameleon_Character_BP_C"
};

export function resolveArkCreatureBlueprint(moduleId: string, creature: string): string {
  const value = creature.trim();
  return (moduleId === "arksurvivalascended" ? ASA_CREATURE_BLUEPRINTS[value] : undefined)
    ?? BASE_CREATURE_BLUEPRINTS[value]
    ?? value;
}
