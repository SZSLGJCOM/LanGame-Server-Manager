import { useEffect, useRef, useState } from "react";
import { previewInstanceLaunch, readModuleDetails } from "../api";
import {
  ServerModuleInstallationReader,
  type ServerModuleInstallationInput,
  type ServerModuleInstallationSnapshot
} from "../server-module-installations";

export function useServerModuleInstallations(options: ServerModuleInstallationInput & {
  onError: (error: unknown) => void;
}): ServerModuleInstallationSnapshot {
  const latest = useRef(options);
  latest.current = options;
  const [snapshot, setSnapshot] = useState<ServerModuleInstallationSnapshot>({
    moduleInstallations: {}, instanceLaunchPlans: {}, instanceLaunchFailures: {}
  });
  const readerRef = useRef<ServerModuleInstallationReader | null>(null);
  if (!readerRef.current) {
    readerRef.current = new ServerModuleInstallationReader({
      readModuleDetails: (moduleId) => readModuleDetails(moduleId, { includePreservedProgramCounts: false }),
      previewInstanceLaunch,
      onChange: setSnapshot,
      onError: (error) => latest.current.onError(error)
    });
  }
  const reader = readerRef.current;
  useEffect(() => {
    reader.update(options);
  }, [reader, options.enabled, options.modules, options.instances, options.selectedInstanceModuleDetails, options.selectedLaunchPlan]);
  useEffect(() => () => reader.pause(), [reader]);
  return snapshot;
}
