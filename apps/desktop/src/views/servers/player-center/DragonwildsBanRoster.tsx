import type { ComponentProps } from "react";
import { NativeBanRosterActions, NativeBanRosterList } from "./NativeBanRoster";

export const DragonwildsBanRosterList = NativeBanRosterList;

export function DragonwildsBanRosterActions(props: Omit<ComponentProps<typeof NativeBanRosterActions>, "moduleId">) {
  return <NativeBanRosterActions {...props} moduleId="runescapedragonwilds" />;
}
