type InstanceSelectionUpdate = string | null | ((current: string | null) => string | null);

export interface PreparedInstanceSelection {
  commit(current: string | null): string | null;
}

export class InstanceSelectionCursor {
  private instanceId: string | null;
  private intentVersion = 0;

  constructor(instanceId: string | null) {
    this.instanceId = instanceId;
  }

  capture(instanceId: string | null) {
    this.intentVersion += 1;
    this.instanceId = instanceId;
  }

  prepare(next: InstanceSelectionUpdate): PreparedInstanceSelection {
    const intentVersion = ++this.intentVersion;
    if (typeof next !== "function") {
      this.instanceId = next;
    }

    return {
      commit: (current) => {
        const resolved = typeof next === "function" ? next(current) : next;
        if (this.intentVersion === intentVersion) {
          this.instanceId = resolved;
        }
        return resolved;
      }
    };
  }

  current(): string | null {
    return this.instanceId;
  }
}

export interface InstancePanelRefreshPorts<TPanel> {
  getCurrentInstanceId: () => string | null;
  cacheInstancePanel: (instanceId: string, panel: TPanel) => void;
  replaceSelectedInstancePanel: (panel: TPanel) => void;
  reloadBootstrap?: () => void | Promise<unknown>;
}

export type InstancePanelRefreshResult = "cached-only" | "selected";

export async function refreshInstancePanelForCurrentSelection<TPanel>(
  instanceId: string,
  loadPanel: () => Promise<TPanel>,
  ports: InstancePanelRefreshPorts<TPanel>
): Promise<InstancePanelRefreshResult> {
  const panel = await loadPanel();
  ports.cacheInstancePanel(instanceId, panel);
  await ports.reloadBootstrap?.();
  if (ports.getCurrentInstanceId() !== instanceId) {
    return "cached-only";
  }

  ports.replaceSelectedInstancePanel(panel);
  return "selected";
}
