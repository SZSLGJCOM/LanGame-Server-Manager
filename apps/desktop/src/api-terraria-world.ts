import { invokeOrMock } from "./api-transport";
import type { DstWorldStartPreview } from "./types";

export interface TerrariaWorldConflict {
  instance_id: string;
  settings_sha256: string;
  world_file: string;
  world_version: number;
  max_world_version: number;
  suggested_world_file: string;
}

export type WorldStartConfirmation = DstWorldStartPreview | { create_terraria_world: TerrariaWorldConflict };

export const previewTerrariaModWorld = (instanceId: string) =>
  invokeOrMock<TerrariaWorldConflict | null>("preview_terraria_mod_world", { instanceId });
