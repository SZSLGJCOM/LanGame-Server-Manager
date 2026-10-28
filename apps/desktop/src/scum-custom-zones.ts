import { invokeOrMock } from "./api-transport";

export type ScumZoneHandling = "ignore" | "allow" | "block";
export interface ScumNpcVehicleDamage {
  melee: ScumZoneHandling;
  melee_weapon: ScumZoneHandling;
  projectile: ScumZoneHandling;
}
export interface ScumCustomZoneConfiguration {
  map_id: number;
  configuration_id: number;
  name: string;
  global: boolean;
  regions: string[];
  damage: ScumNpcVehicleDamage;
}
export interface ScumCustomZonesSnapshot {
  instance_id: string;
  status: "ready" | "empty" | "running" | "unsupported";
  database_identity: string | null;
  revision: string | null;
  configurations: ScumCustomZoneConfiguration[];
  writable: boolean;
  message: string | null;
  backup_id: string | null;
}
export interface WriteScumCustomZoneDamageInput {
  instance_id: string;
  database_identity: string;
  expected_revision: string;
  map_id: number;
  configuration_id: number;
  damage: ScumNpcVehicleDamage;
}

export const SCUM_NPC_VEHICLE_DAMAGE_FIELDS = ["melee", "melee_weapon", "projectile"] as const;
export const SCUM_ZONE_HANDLING_METHODS: readonly ScumZoneHandling[] = ["ignore", "allow", "block"];

export function isScumZoneHandling(value: unknown): value is ScumZoneHandling {
  return value === "ignore" || value === "allow" || value === "block";
}

export function scumZoneKey(zone: Pick<ScumCustomZoneConfiguration, "map_id" | "configuration_id">): string {
  return `${zone.map_id}:${zone.configuration_id}`;
}

export function sameScumZoneDamage(left: ScumNpcVehicleDamage, right: ScumNpcVehicleDamage): boolean {
  return SCUM_NPC_VEHICLE_DAMAGE_FIELDS.every((field) => left[field] === right[field]);
}

export function buildScumCustomZoneDamageInput(
  snapshot: ScumCustomZonesSnapshot, zone: ScumCustomZoneConfiguration, damage: ScumNpcVehicleDamage
): WriteScumCustomZoneDamageInput {
  if (snapshot.status !== "ready" || !snapshot.writable || !snapshot.database_identity || !snapshot.revision ||
    !/^[a-f0-9]{64}$/u.test(snapshot.revision) ||
    !snapshot.configurations.some((entry) => scumZoneKey(entry) === scumZoneKey(zone)) ||
    !SCUM_NPC_VEHICLE_DAMAGE_FIELDS.every((field) => isScumZoneHandling(damage[field]))) {
    throw new Error("Reload the stopped server's custom-zone data before saving.");
  }
  return {
    instance_id: snapshot.instance_id, database_identity: snapshot.database_identity,
    expected_revision: snapshot.revision, map_id: zone.map_id, configuration_id: zone.configuration_id,
    damage: { melee: damage.melee, melee_weapon: damage.melee_weapon, projectile: damage.projectile }
  };
}

export const readScumCustomZones = (instanceId: string) =>
  invokeOrMock<ScumCustomZonesSnapshot>("read_scum_custom_zones", { instanceId });

export const writeScumCustomZoneDamage = (input: WriteScumCustomZoneDamageInput) =>
  invokeOrMock<ScumCustomZonesSnapshot>("write_scum_custom_zone_damage", { input });
