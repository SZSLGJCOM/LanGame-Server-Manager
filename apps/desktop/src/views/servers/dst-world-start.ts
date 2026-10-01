import type { DstWorldStartPreview } from "../../types";

interface StartPreparation {
  flush(instanceId: string): Promise<void>;
  preview(instanceId: string): Promise<DstWorldStartPreview>;
}

export class DstWorldStartError extends Error {
  constructor(readonly code: "unrecognized" | "changed") {
    super(code);
  }
}

export async function prepareDstWorldStart(instanceId: string, operations: StartPreparation): Promise<DstWorldStartPreview> {
  await operations.flush(instanceId);
  const preview = await operations.preview(instanceId);
  if (preview.instance_id !== instanceId) throw new DstWorldStartError("changed");
  const enabled = preview.shards.filter((shard) => shard.enabled);
  if (enabled.some((shard) => shard.state === "unrecognized")) throw new DstWorldStartError("unrecognized");
  // The backend checks this complete snapshot under the instance lock before
  // materializing configuration or starting either shard.
  return preview;
}
