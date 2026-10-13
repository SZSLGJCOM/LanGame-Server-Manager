import { useEffect, useState, type ComponentType } from "react";
import { createDeferredModule } from "../../deferred-module";
import { useDeferredModule } from "../../hooks/useDeferredModule";
import { ViewLoadingState } from "../ViewLoadingState";
import type { ServerMaintenanceWorkspaceProps } from "./ServerMaintenanceWorkspace";

function deferWorkspace<Props extends object>(
  load: () => Promise<ComponentType<Props>>,
  recover: () => Promise<ComponentType<Props>>
) {
  const source = createDeferredModule(load, recover);
  return function DeferredWorkspace(props: Props) {
    const state = useDeferredModule(source);
    const Workspace = state.value;
    return Workspace ? <Workspace {...props} />
      : <ViewLoadingState error={state.error} canRetry={state.canRetry} onRetry={state.retry} />;
  };
}

export const ConfigurationWorkspace = deferWorkspace(
  () => import("../settings/ConfigurationWorkspace").then((module) => module.ConfigurationWorkspace),
  () => import("../settings/ConfigurationWorkspace?module-retry").then((module) => module.ConfigurationWorkspace)
);
export const ModWorkbench = deferWorkspace(
  () => import("./ModWorkbench").then((module) => module.ModWorkbench),
  () => import("./ModWorkbench?module-retry").then((module) => module.ModWorkbench)
);
export const PlayerCenterWorkbench = deferWorkspace(
  () => import("./PlayerCenterWorkbench").then((module) => module.PlayerCenterWorkbench),
  () => import("./PlayerCenterWorkbench?module-retry").then((module) => module.PlayerCenterWorkbench)
);
export const GMToolsWorkbench = deferWorkspace(
  () => import("./GMToolsWorkbench").then((module) => module.GMToolsWorkbench),
  () => import("./GMToolsWorkbench?module-retry").then((module) => module.GMToolsWorkbench)
);

const maintenanceModule = createDeferredModule(
  () => import("./ServerMaintenanceWorkspace").then((module) => module.ServerMaintenanceWorkspace),
  () => import("./ServerMaintenanceWorkspace?module-retry").then((module) => module.ServerMaintenanceWorkspace)
);

export function ServerMaintenanceWorkspace(props: ServerMaintenanceWorkspaceProps) {
  const [visited, setVisited] = useState(props.active);
  useEffect(() => {
    if (props.active) setVisited(true);
  }, [props.active]);
  // Once opened, retain the maintenance owner and immediate-save results across tabs.
  // The owning instance key still resets this lifetime when the selection changes.
  const state = useDeferredModule(maintenanceModule, props.active || visited);
  const Workspace = state.value;
  return Workspace ? <Workspace {...props} /> : props.active
    ? <ViewLoadingState error={state.error} canRetry={state.canRetry} onRetry={state.retry} /> : null;
}
