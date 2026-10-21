const CODES = new Set(["BUSY", "SHARD_NOT_RUNNING", "SHARD_NOT_ENABLED", "SHARD_CHANGED",
  "CONSOLE_UNAVAILABLE", "WORLD_NOT_READY", "RESPONSE_TIMEOUT", "ARTWORK_WORKER_FAILED"]);

/** Only the host's explicit code is authoritative; arbitrary Mod text is not. */
export function dstMapErrorCode(message: string): string | null {
  const match = /^DST_MAP_([A-Z_]+): /.exec(message);
  return match && CODES.has(match[1]) ? match[1] : null;
}
