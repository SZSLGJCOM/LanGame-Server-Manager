import { createContext, useContext, useState, type ReactNode } from "react";
import { InstanceSettingsSaveCoordinator } from "./instance-settings-save-coordinator";

const InstanceSettingsSaveContext = createContext<InstanceSettingsSaveCoordinator | null>(null);

export function InstanceSettingsSaveProvider({ children }: { children: ReactNode }) {
  const [coordinator] = useState(() => new InstanceSettingsSaveCoordinator());
  return <InstanceSettingsSaveContext.Provider value={coordinator}>{children}</InstanceSettingsSaveContext.Provider>;
}

export function useInstanceSettingsSaveCoordinator(): InstanceSettingsSaveCoordinator {
  const coordinator = useContext(InstanceSettingsSaveContext);
  if (!coordinator) throw new Error("Instance settings saves require InstanceSettingsSaveProvider.");
  return coordinator;
}
