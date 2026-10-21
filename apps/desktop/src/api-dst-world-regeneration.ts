import { invokeOrMock } from "./api-transport";
import type { StartInstanceResult } from "./types";

/** Saves and stops the server, deletes every shard's world save, then starts it to generate a new world. */
export const regenerateDstWorld = (instanceId: string) =>
  invokeOrMock<StartInstanceResult>("regenerate_dontstarve_world", { instanceId });
