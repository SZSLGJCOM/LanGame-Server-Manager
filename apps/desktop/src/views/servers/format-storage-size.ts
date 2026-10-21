/** Shared byte formatter for save, backup and cluster snapshot sizes (binary units). */
export function formatStorageSize(totalBytes: number): string {
  if (!Number.isFinite(totalBytes) || totalBytes < 0) return "—";
  if (totalBytes >= 1024 * 1024 * 1024) return `${(totalBytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
  if (totalBytes >= 1024 * 1024) return `${(totalBytes / (1024 * 1024)).toFixed(1)} MB`;
  if (totalBytes >= 1024) return `${Math.round(totalBytes / 1024)} KB`;
  return `${totalBytes} B`;
}
