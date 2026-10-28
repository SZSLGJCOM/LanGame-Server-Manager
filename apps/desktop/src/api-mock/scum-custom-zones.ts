import type { InstanceDetails } from "../types";
import {
  buildScumCustomZoneDamageInput, scumZoneKey,
  type ScumCustomZonesSnapshot, type WriteScumCustomZoneDamageInput
} from "../scum-custom-zones";

export class MockScumCustomZones {
  private readonly snapshots = new Map<string, ScumCustomZonesSnapshot>();
  private revision = 1;

  read(details: InstanceDetails): ScumCustomZonesSnapshot {
    const { id, module_id, status, active_process_count } = details.summary;
    if (module_id !== "scum") throw new Error("Custom zones belong only to SCUM instances.");
    if (status !== "Stopped" || active_process_count !== 0 || details.active_run !== null) {
      return { instance_id: id, status: "running", database_identity: null, revision: null,
        configurations: [], writable: false, message: null, backup_id: null };
    }
    const existing = this.snapshots.get(id);
    if (existing) return structuredClone(existing);
    const snapshot: ScumCustomZonesSnapshot = {
      instance_id: id, status: "ready", database_identity: `mock-scum-database:${id}`,
      revision: this.revision.toString(16).padStart(64, "0"), writable: true, message: null, backup_id: null,
      configurations: [
        { map_id: 1, configuration_id: 1000, name: "Global", global: true, regions: [],
          damage: { melee: "allow", melee_weapon: "allow", projectile: "allow" } },
        { map_id: 1, configuration_id: 1, name: "Workshop", global: false, regions: ["Workshop north", "Workshop south"],
          damage: { melee: "ignore", melee_weapon: "ignore", projectile: "ignore" } }
      ]
    };
    this.snapshots.set(id, snapshot);
    return structuredClone(snapshot);
  }

  write(details: InstanceDetails, input: WriteScumCustomZoneDamageInput): ScumCustomZonesSnapshot {
    const before = this.read(details);
    if (input.instance_id !== before.instance_id || input.database_identity !== before.database_identity ||
      input.expected_revision !== before.revision) throw new Error("Custom-zone data changed. Reload before saving.");
    const selected = before.configurations.find((zone) => scumZoneKey(zone) === scumZoneKey(input));
    if (!selected) throw new Error("The selected custom-zone configuration no longer exists.");
    const checked = buildScumCustomZoneDamageInput(before, selected, input.damage);
    const result: ScumCustomZonesSnapshot = {
      ...before, revision: (++this.revision).toString(16).padStart(64, "0"),
      backup_id: `mock-scum-zone-backup-${this.revision}`,
      configurations: before.configurations.map((zone) => scumZoneKey(zone) === scumZoneKey(selected)
        ? { ...zone, damage: checked.damage } : zone)
    };
    this.snapshots.set(before.instance_id, structuredClone(result));
    return structuredClone(result);
  }
}
