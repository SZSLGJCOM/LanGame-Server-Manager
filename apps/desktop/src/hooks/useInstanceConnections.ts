import { useCallback, useEffect, useRef, useState } from "react";
import { readInstanceConnectionInfo } from "../api";
import type { InstanceConnectionInfo, InstanceDetails, InstanceSummary } from "../types";

interface ConnectionSnapshot {
  connections: Partial<Record<string, InstanceConnectionInfo>>;
  failed: boolean;
}

export function useInstanceConnections(
  instances: InstanceSummary[],
  selectedDetails: InstanceDetails | null,
  readConnections = readInstanceConnectionInfo
) {
  const [snapshot, setSnapshot] = useState<ConnectionSnapshot>({ connections: {}, failed: false });
  const latest = useRef(instances);
  latest.current = instances;
  const flight = useRef({ mounted: false, running: false, requested: 0, epoch: 0, fingerprint: "" });

  useEffect(() => {
    const owner = flight.current;
    owner.mounted = true;
    return () => { owner.mounted = false; owner.epoch++; };
  }, []);

  useEffect(() => {
    const owner = flight.current;
    owner.requested++;
    const fingerprint = JSON.stringify([
      instances.map(({ id, module_id, bind_ip, status, port_count }) => [id, module_id, bind_ip, status, port_count]),
      selectedDetails && [selectedDetails.summary.id, selectedDetails.summary.bind_ip, selectedDetails.ports,
        selectedDetails.summary.module_id === "corekeeper" ? selectedDetails.settings_json : null]
    ]);
    if (fingerprint !== owner.fingerprint) {
      owner.fingerprint = fingerprint;
      owner.epoch++;
    }

    async function refresh() {
      // IPC cannot be cancelled. Retain its slot until completion, and coalesce newer refreshes.
      if (owner.running) return;
      owner.running = true;
      let completedRequest = -1;
      try {
        while (owner.mounted && completedRequest !== owner.requested) {
          const epoch = owner.epoch;
          completedRequest = owner.requested;
          const ids = latest.current.map((instance) => instance.id);
          const connections: ConnectionSnapshot["connections"] = {};
          try {
            for (let index = 0; index < ids.length; index += 128) {
              if (!owner.mounted || owner.epoch !== epoch) break;
              const batch = ids.slice(index, index + 128);
              const rows = await readConnections(batch);
              const expected = new Set(batch);
              for (const row of rows) {
                if (!expected.delete(row.instance_id)) throw new Error("Unexpected instance connection response");
                connections[row.instance_id] = row;
              }
              if (expected.size) throw new Error("Incomplete instance connection response");
            }
            if (owner.mounted && owner.epoch === epoch) setSnapshot({ connections, failed: false });
          } catch {
            // Clear stale endpoints on failure; copying independently rereads the saved configuration.
            if (owner.mounted && owner.epoch === epoch) setSnapshot({ connections: {}, failed: true });
          }
        }
      } finally {
        owner.running = false;
      }
    }

    void refresh();
  }, [instances, selectedDetails, readConnections]);

  const acceptConnection = useCallback((connection: InstanceConnectionInfo) => {
    const owner = flight.current;
    if (!owner.mounted) return;
    owner.epoch++;
    owner.requested++;
    setSnapshot((current) => ({
      ...current,
      connections: { ...current.connections, [connection.instance_id]: connection }
    }));
  }, []);

  return { ...snapshot, acceptConnection };
}
