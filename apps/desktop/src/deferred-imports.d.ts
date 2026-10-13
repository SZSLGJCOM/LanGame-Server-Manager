declare module "*AssistantPanel?module-retry" {
  export const AssistantPanel: typeof import("./components/AssistantPanel").AssistantPanel;
}

declare module "*LibraryView?module-retry" {
  export const LibraryView: typeof import("./views/LibraryView").LibraryView;
}

declare module "*ServerWorkspaceView?module-retry" {
  export const ServerWorkspaceView: typeof import("./views/ServerWorkspaceView").ServerWorkspaceView;
}

declare module "*SystemView?module-retry" {
  export const SystemView: typeof import("./views/SystemView").SystemView;
}

declare module "*ConfigurationWorkspace?module-retry" {
  export const ConfigurationWorkspace: typeof import("./views/settings/ConfigurationWorkspace").ConfigurationWorkspace;
}

declare module "*ModWorkbench?module-retry" {
  export const ModWorkbench: typeof import("./views/servers/ModWorkbench").ModWorkbench;
}

declare module "*PlayerCenterWorkbench?module-retry" {
  export const PlayerCenterWorkbench: typeof import("./views/servers/PlayerCenterWorkbench").PlayerCenterWorkbench;
}

declare module "*GMToolsWorkbench?module-retry" {
  export const GMToolsWorkbench: typeof import("./views/servers/GMToolsWorkbench").GMToolsWorkbench;
}

declare module "*ServerMaintenanceWorkspace?module-retry" {
  export const ServerMaintenanceWorkspace: typeof import("./views/servers/ServerMaintenanceWorkspace").ServerMaintenanceWorkspace;
}
