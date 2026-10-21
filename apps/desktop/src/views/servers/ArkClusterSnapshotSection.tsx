import { useCallback, useEffect, useRef, useState } from "react";
import { readArkCluster } from "../../api";
import { describeError } from "../../app-state";
import type { ArkClusterReport } from "../../ark-clusters";
import { ActivityNotice } from "../../components/ActivityNotice";
import { useI18n } from "../../i18n";
import { ArkClusterMaintenance } from "./ArkClusterMaintenance";

interface Props {
  instanceId: string;
  onChanged?: () => void;
}

/** Cluster-wide snapshots live with backups; the cluster report is read here so
 * snapshot safety checks (members stopped, no issues, interrupted restores) still apply. */
export function ArkClusterSnapshotSection({ instanceId, onChanged }: Props) {
  const { t } = useI18n();
  const [report, setReport] = useState<ArkClusterReport | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const request = useRef(0);

  const refresh = useCallback(async () => {
    const id = ++request.current;
    setLoading(true);
    setError(null);
    try {
      const next = await readArkCluster(instanceId);
      if (id === request.current) setReport(next);
    } catch (failure) {
      if (id === request.current) { setReport(null); setError(describeError(failure)); }
    } finally {
      if (id === request.current) setLoading(false);
    }
  }, [instanceId]);

  useEffect(() => {
    setReport(null);
    void refresh();
    return () => { request.current += 1; };
  }, [refresh]);

  return <>
    {error ? <ActivityNotice tone="error" action={<button type="button" className="secondary-button" disabled={loading}
      onClick={() => void refresh()}>{t("common.retry")}</button>}>{error}</ActivityNotice> : null}
    <ArkClusterMaintenance instanceId={instanceId} report={report} busy={loading}
      onChanged={async () => { await refresh(); onChanged?.(); }} />
  </>;
}
