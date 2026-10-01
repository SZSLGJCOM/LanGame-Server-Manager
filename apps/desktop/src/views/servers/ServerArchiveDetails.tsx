import { useI18n } from "../../i18n";
import type { PendingInstanceDeletion } from "../../storage-management-types";

interface Props {
  deletion: PendingInstanceDeletion;
}

export function ServerArchiveDetails({ deletion }: Props) {
  const { t } = useI18n();
  return <div className="server-archive-details" data-deletion-id={deletion.operation_id}>
    <header><h2>{deletion.instance_name}</h2><span className="server-archive-warning">{t("servers.archives.deleteFailed")}</span></header>
    <p>{t("storage.pendingDeletionNote")}</p>
    <dl><div><dt>{t("servers.archives.deletionPath")}</dt><dd>{deletion.deleted_instance_root}</dd></div></dl>
    <ul className="server-archive-errors">{deletion.issues.map((issue, index) => <li key={index}>{issue}</li>)}</ul>
  </div>;
}
