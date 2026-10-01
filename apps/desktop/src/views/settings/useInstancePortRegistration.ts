import { useCallback, useEffect, useMemo, useReducer } from "react";
import type { InstanceDetails, ModuleDetails, PortBinding } from "../../types";
import { buildArkMapPortGroups } from "./ark-cluster-maps";
import {
  createInstancePortRegistrationState,
  normalizeInstancePorts,
  reduceInstancePortRegistrationState
} from "./instance-port-presentation";

export function useInstancePortRegistration(details: InstanceDetails, moduleDetails: ModuleDetails | null) {
  const persistedPorts = useMemo(() => normalizeInstancePorts(details.ports), [details.ports]);
  const [registration, dispatch] = useReducer(
    reduceInstancePortRegistrationState,
    persistedPorts,
    createInstancePortRegistrationState
  );
  const defaultPorts = useMemo(
    () => normalizeInstancePorts(moduleDetails?.default_ports ?? []),
    [moduleDetails?.default_ports]
  );
  const portGroups = useMemo(
    () => buildArkMapPortGroups(details.summary.module_id, details.settings_json, moduleDetails?.runtime.port_groups ?? []),
    [details.summary.module_id, details.settings_json, moduleDetails?.runtime.port_groups]
  );

  useEffect(() => {
    dispatch({ type: "baseline-received", ports: persistedPorts });
  }, [persistedPorts]);

  const setPorts = useCallback((nextPorts: PortBinding[]) => {
    dispatch({
      type: "draft-edited",
      ports: nextPorts,
      defaultPorts,
      portGroups
    });
  }, [defaultPorts, portGroups]);

  return {
    ports: registration.draftPorts,
    defaultPorts,
    setPorts
  };
}
