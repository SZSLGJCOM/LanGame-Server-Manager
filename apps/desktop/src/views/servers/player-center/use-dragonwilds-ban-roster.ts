import { useNativeBanRoster } from "./use-native-ban-roster";

export function useDragonwildsBanRoster(instanceId: string | null) {
  return useNativeBanRoster(instanceId, "runescapedragonwilds");
}

export type DragonwildsBanRosterState = ReturnType<typeof useDragonwildsBanRoster>;
