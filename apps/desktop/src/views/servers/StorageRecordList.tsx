import type { ReactNode } from "react";
import { ConfigurationHelp } from "../settings/ConfigurationFieldHelp";

/** Shared dense list for backup and save-slot rows, so both read as one table. */
export function StorageRecordList({ label, children }: { label: string; children: ReactNode }) {
  return <div className="server-file-backup-table" role="list" aria-label={label}>{children}</div>;
}

interface RowProps {
  name: string;
  /** Small marker after the name (kind tag, in-use dot). */
  badge?: ReactNode;
  /** One-line facts: size and relative time. */
  facts: ReactNode;
  /** Secondary facts (file count, absolute time, path, notes) shown in the help bubble. */
  details?: string;
  actions?: ReactNode;
  /** Full-width extension below the row (e.g. an expandable file list). */
  children?: ReactNode;
  current?: boolean;
}

export function StorageRecordRow({ name, badge, facts, details, actions, children, current }: RowProps) {
  return <article className="server-file-backup-row" role="listitem" data-current={current || undefined}>
    <ConfigurationHelp description={details ? `${name}\n${details}` : null}>{(help) =>
      <div className="server-file-backup-main" ref={help.anchorRef} {...help.interactionProps}
        tabIndex={help.descriptionId ? 0 : undefined} aria-describedby={help.descriptionId}>
        <span className="server-file-backup-name">{name}</span>
        {badge}
        <span className="server-file-backup-meta">{facts}</span>
      </div>}</ConfigurationHelp>
    {actions ? <div className="server-file-backup-actions">{actions}</div> : null}
    {children}
  </article>;
}
