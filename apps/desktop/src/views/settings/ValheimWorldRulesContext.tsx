import { createContext, useCallback, useContext, useEffect, useRef, useState } from "react";
import { readValheimWorldRules, type ValheimWorldRules } from "../../valheim-world";
import type { ConfigurationWorkspaceProviderProps } from "./module-types";

type RuleState = { status: "loading" } | { status: "ready"; world: ValheimWorldRules } | { status: "error"; error: string };
const Context = createContext<{ state: RuleState; refresh(): Promise<void> } | null>(null);

export function ValheimWorldRulesProvider(props: ConfigurationWorkspaceProviderProps) {
  const instanceId = props.details.summary.id;
  let worldName = props.details.summary.name;
  try {
    const settings: unknown = JSON.parse(props.details.settings_json);
    if (settings && typeof settings === "object" && "world_name" in settings && typeof settings.world_name === "string") {
      worldName = settings.world_name;
    }
  } catch { /* The configuration workspace reports malformed instance settings. */ }
  const identity = `${instanceId}\u0000${worldName}`;
  const generation = useRef(0);
  const [snapshot, setSnapshot] = useState<{ identity: string; state: RuleState }>({ identity, state: { status: "loading" } });
  const refresh = useCallback(async () => {
    const request = ++generation.current;
    setSnapshot({ identity, state: { status: "loading" } });
    try {
      const world = await readValheimWorldRules(instanceId, worldName);
      if (generation.current === request) setSnapshot({ identity, state: { status: "ready", world } });
    } catch (error) {
      if (generation.current === request) setSnapshot({ identity, state: { status: "error", error: String(error) } });
    }
  }, [identity, instanceId, worldName]);
  useEffect(() => { void refresh(); return () => { generation.current++; }; }, [refresh]);
  return <Context.Provider value={{ state: snapshot.identity === identity ? snapshot.state : { status: "loading" }, refresh }}>
    {props.children}
  </Context.Provider>;
}

export function useValheimWorldRules() { return useContext(Context); }
