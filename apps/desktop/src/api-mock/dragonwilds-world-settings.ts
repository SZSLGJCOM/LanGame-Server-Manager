import type { InstanceDetails } from "../types";
import {
  DRAGONWILDS_WORLD_SETTINGS, isDragonwildsWorldSettingEditable, isDragonwildsWorldSettingValueValid,
  type DragonwildsWorldMode,
  type DragonwildsWorldSettingsSnapshot, type WriteDragonwildsWorldSettingsInput
} from "../dragonwilds-world-settings";

const definitions = DRAGONWILDS_WORLD_SETTINGS;

interface MockWorld {
  mode: DragonwildsWorldMode;
  overrides: Record<string, number>;
  revision: number;
}

export class MockDragonwildsWorldSettings {
  private readonly worlds = new Map<string, MockWorld>();

  read(details: InstanceDetails, backupId: string | null = null): DragonwildsWorldSettingsSnapshot {
    if (details.summary.module_id !== "runescapedragonwilds") throw new Error("Not a Dragonwilds instance.");
    let world = this.worlds.get(details.summary.id);
    if (!world) {
      world = { mode: "Normal", overrides: {}, revision: 0 };
      this.worlds.set(details.summary.id, world);
    }
    const values = Object.fromEntries(definitions.map((definition) => [definition.tag,
      world.overrides[definition.tag] ?? definition.preset_defaults[world.mode === "Custom" ? "Normal" : world.mode]]));
    return structuredClone({
      instance_id: details.summary.id, status: "ready", world_file: "MockWorld.sav",
      world_name: details.summary.name, world_mode: world.mode, revision: `mock-world-${world.revision}`,
      values, overrides: world.overrides, definitions: [...definitions],
      writable: details.summary.status === "Stopped" && details.summary.active_process_count === 0 && !details.active_run,
      message: null, backup_id: backupId
    });
  }

  write(details: InstanceDetails, input: WriteDragonwildsWorldSettingsInput): DragonwildsWorldSettingsSnapshot {
    const snapshot = this.read(details);
    if (!snapshot.writable) throw new Error("Stop the server before changing world settings.");
    if (input.instance_id !== details.summary.id || input.world_file !== snapshot.world_file || input.expected_revision !== snapshot.revision) {
      throw new Error("World settings changed. Reload before saving.");
    }
    if (!["Normal", "Hard", "Creative", "Custom"].includes(input.world_mode)) throw new Error("Invalid world mode.");
    const world = this.worlds.get(details.summary.id);
    if (!world) throw new Error("World not found.");
    const overrides = { ...world.overrides };
    if (world.mode !== "Custom" && input.world_mode === "Custom") {
      for (const definition of definitions) {
        overrides[definition.tag] ??= snapshot.values[definition.tag];
      }
    }
    for (const [tag, value] of Object.entries(input.values)) {
      const definition = definitions.find((item) => item.tag === tag);
      if (!definition || !isDragonwildsWorldSettingEditable(definition, input.world_mode) ||
        !isDragonwildsWorldSettingValueValid(definition, String(value))) {
        throw new Error("Invalid world rule change.");
      }
      overrides[tag] = value;
    }
    world.mode = input.world_mode;
    world.overrides = overrides;
    world.revision += 1;
    return this.read(details, `mock-world-settings-backup-${world.revision}`);
  }
}
