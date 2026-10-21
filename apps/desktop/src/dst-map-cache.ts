// Rebuildable offline snapshots only. Cache failures never prevent native reads.
import { parseDstWorldMap, type DstWorldMap } from "./dst-world-map";
import { DST_MAP_MAX_VIEW_ZOOM, DST_MAP_MIN_ZOOM } from "./dst-map-view";
import { DONTSTARVE_SHARDS, type DontStarveShard } from "./views/settings/modules/dontstarve-shards";

const DATABASE = "langame-dst-map-cache";
const MAPS = "maps", METADATA = "metadata";
const MAX_ENTRIES = 8, MAX_ENTRY_BYTES = 20 * 1024 * 1024, MAX_TOTAL_BYTES = 64 * 1024 * 1024;
const MAX_AGE_MS = 30 * 24 * 60 * 60 * 1000, STORAGE_BUDGET_MS = 1500;
const METADATA_BYTE_ALLOWANCE = 4096;

export interface DstMapCachedView { center: { x: number; y: number }; zoom: number }
export interface DstMapCacheEntry { snapshot: DstWorldMap; terrainAt: number; view: DstMapCachedView | null }
interface Metadata {
  key: string; bytes: number; terrainAt: number; touchedAt: number; capturedAt: number;
  session: string; width: number; height: number; tileScale: number; runId: number;
}
interface StoredEntry extends DstMapCacheEntry { key: string }
interface PendingWrite {
  promise: Promise<boolean>; latest: DstMapCacheEntry | null; newest: DstWorldMap; deadline: number;
}
// Each key owns one worker and at most one coalesced successor. A read joins
// that worker even while it is sizing data, before its IDB transaction exists.
const pendingWrites = new Map<string, PendingWrite>();

function cacheKey(instanceId: string, shard: DontStarveShard): string | null {
  return typeof instanceId === "string" && instanceId.length > 0 && instanceId.length <= 256
    && !/[\u0000-\u001f\u007f]/.test(instanceId) && DONTSTARVE_SHARDS.includes(shard)
    ? JSON.stringify([instanceId, shard]) : null;
}
function timestamp(value: unknown, now: number): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value > 0 && value <= now;
}
function validView(value: unknown, snapshot: DstWorldMap): DstMapCachedView | null {
  if (!value || typeof value !== "object") return null;
  const view = value as Partial<DstMapCachedView>, center = view.center;
  return center && Number.isFinite(center.x) && Number.isFinite(center.y)
    && center.x >= 0 && center.x <= snapshot.width && center.y >= 0 && center.y <= snapshot.height
    && typeof view.zoom === "number" && Number.isFinite(view.zoom)
    && view.zoom >= DST_MAP_MIN_ZOOM && view.zoom <= DST_MAP_MAX_VIEW_ZOOM
    ? { center: { x: center.x, y: center.y }, zoom: view.zoom } : null;
}

/** A restored camera belongs to the same native world geometry, never just a shard name. */
export function dstMapCachedView(entry: DstMapCacheEntry, snapshot: DstWorldMap): DstMapCachedView | null {
  const previous = entry.snapshot;
  return previous.instanceId === snapshot.instanceId && previous.shard === snapshot.shard
    && previous.session === snapshot.session && previous.width === snapshot.width && previous.height === snapshot.height
    && previous.tileScale === snapshot.tileScale ? validView(entry.view, snapshot) : null;
}

function validate(value: unknown, instanceId: string, shard: DontStarveShard, now: number): DstMapCacheEntry | null {
  try {
    if (!value || typeof value !== "object") return null;
    const entry = value as Partial<DstMapCacheEntry>;
    if (!timestamp(entry.terrainAt, now) || now - entry.terrainAt > MAX_AGE_MS) return null;
    const snapshot = parseDstWorldMap(entry.snapshot, instanceId, shard, true);
    if (!timestamp(snapshot.capturedAtUnixMs, now)) return null;
    // Persist only the map DTO: unknown fields and caller-owned objects are not cache authority.
    const { runId, capturedAtUnixMs, session, width, height, tileScale, day, season, phase,
      tiles, tileNames, tileKinds, players, tileTextures, tileBaseTextures, landmarks, tileColors,
      terrainLayers, roads, landmarksTruncated, art } = snapshot;
    return { snapshot: { instanceId, shard, runId, capturedAtUnixMs, session, width, height, tileScale, day, season, phase,
      tiles, tileNames, tileKinds, players, tileTextures, tileBaseTextures, landmarks, tileColors, terrainLayers, roads, landmarksTruncated, art },
      terrainAt: entry.terrainAt, view: validView(entry.view, snapshot) };
  } catch { return null; }
}

// A conservative UTF-8 JSON size, without allocating/stringifying an entire tile grid.
// Each slice visits at most4096 values; cancellation also bounds pre-storage work.
async function boundedBytes(value: unknown, deadline: number): Promise<number | null> {
  const encoder = new TextEncoder();
  function* parts(item: unknown, depth = 0): Generator<number> {
    if (depth > 12) throw new Error("Map cache nesting exceeds its data contract.");
    if (item === null) { yield 4; return; }
    if (typeof item === "string") {
      if (item.length > 1024 * 1024) throw new Error("Map cache string exceeds the artwork contract.");
      yield encoder.encode(JSON.stringify(item)).length; return;
    }
    if (typeof item === "number" || typeof item === "boolean") { yield String(item).length; return; }
    if (Array.isArray(item)) {
      yield 2 + Math.max(0, item.length - 1);
      for (const child of item) yield* parts(child, depth + 1);
    } else if (item && typeof item === "object") {
      const entries = Object.entries(item).filter(([, child]) => child !== undefined);
      yield 2 + Math.max(0, entries.length - 1);
      for (const [key, child] of entries) { yield encoder.encode(JSON.stringify(key)).length + 1; yield* parts(child, depth + 1); }
    } else throw new Error("Unsupported map cache data.");
  }
  // Timer nesting adds a minimum delay to every slice in browsers. On native
  // tile grids those delays consumed the storage budget before a coalesced
  // camera write could run. A task channel yields without changing the budget.
  const channel = new MessageChannel();
  try {
    let bytes = 0, visited = 0;
    for (const size of parts(value)) {
      bytes += size;
      if (bytes > MAX_ENTRY_BYTES || Date.now() >= deadline) return null;
      if (++visited % 4096 === 0) await new Promise<void>(resolve => {
        channel.port1.onmessage = () => resolve();
        channel.port2.postMessage(null);
      });
    }
    return bytes;
  } finally { channel.port1.close(); channel.port2.close(); }
}

function validMetadata(value: unknown, now: number): value is Metadata {
  if (!value || typeof value !== "object") return false;
  const item = value as Metadata;
  return typeof item.key === "string" && item.key.length <= 300 && Number.isSafeInteger(item.bytes)
    && item.bytes > 0 && item.bytes <= MAX_ENTRY_BYTES && timestamp(item.terrainAt, now)
    && now - item.terrainAt <= MAX_AGE_MS && timestamp(item.capturedAt, now) && timestamp(item.touchedAt, now)
    && typeof item.session === "string" && item.session.length <= 128
    && Number.isSafeInteger(item.width) && item.width > 0 && item.width <= 2048
    && Number.isSafeInteger(item.height) && item.height > 0 && item.height <= 2048
    && Number.isFinite(item.tileScale) && item.tileScale > 0 && Number.isSafeInteger(item.runId) && item.runId > 0;
}

function inStores<T>(fallback: T, deadline: number,
  action: (maps: IDBObjectStore, metadata: IDBObjectStore, result: (value: T) => void) => void): Promise<T> {
  if (Date.now() >= deadline) return Promise.resolve(fallback);
  return new Promise(resolve => {
    let settled = false, database: IDBDatabase | undefined, transaction: IDBTransaction | undefined, result = fallback;
    const finish = (value: T) => { if (settled) return; settled = true; clearTimeout(timer); database?.close(); resolve(value); };
    const timer = setTimeout(() => {
      finish(fallback);
      try { transaction?.abort(); } catch { /* A completed transaction cannot be aborted. */ }
    }, Math.max(0, deadline - Date.now()));
    try {
      const request = indexedDB.open(DATABASE, 1);
      request.onupgradeneeded = () => {
        if (!request.result.objectStoreNames.contains(MAPS)) request.result.createObjectStore(MAPS, { keyPath: "key" });
        if (!request.result.objectStoreNames.contains(METADATA)) request.result.createObjectStore(METADATA, { keyPath: "key" });
      };
      request.onerror = request.onblocked = () => finish(fallback);
      request.onsuccess = () => {
        database = request.result;
        if (settled) { database.close(); return; }
        database.onversionchange = () => { try { transaction?.abort(); } catch { /* Already complete. */ } finish(fallback); };
        try {
          transaction = database.transaction([MAPS, METADATA], "readwrite");
          transaction.oncomplete = () => finish(result);
          transaction.onabort = transaction.onerror = () => finish(fallback);
          action(transaction.objectStore(MAPS), transaction.objectStore(METADATA), value => { result = value; });
        } catch { finish(fallback); }
      };
    } catch { finish(fallback); }
  });
}

function metadataRows(maps: IDBObjectStore, metadata: IDBObjectStore, done: (rows: Metadata[]) => void): void {
  const rows: Metadata[] = [], now = Date.now();
  let visited = 0;
  const request = metadata.openCursor();
  request.onsuccess = () => {
    const cursor = request.result;
    if (!cursor) { done(rows); return; }
    // Corruption cannot cause an unbounded metadata scan or retain orphaned large payloads.
    if (++visited > 64) { maps.clear(); metadata.clear(); done([]); return; }
    if (validMetadata(cursor.value, now) && cursor.value.key === cursor.primaryKey) rows.push(cursor.value);
    else { maps.delete(cursor.primaryKey); cursor.delete(); }
    cursor.continue();
  };
}

export async function readDstMapCache(instanceId: string, shard: DontStarveShard): Promise<DstMapCacheEntry | null> {
  const key = cacheKey(instanceId, shard), now = Date.now(), deadline = now + STORAGE_BUDGET_MS;
  if (!key) return null;
  try {
    const pending = pendingWrites.get(key);
    if (pending) await pending.promise;
    const cached = await inStores<{ stored: StoredEntry; metadata: Metadata } | null>(null, deadline, (maps, metadata, result) => {
      metadataRows(maps, metadata, rows => {
        const entry = rows.find(item => item.key === key);
        if (!entry) return;
        const request = maps.get(key);
        request.onsuccess = () => {
          if (!request.result || request.result.key !== key) { maps.delete(key); metadata.delete(key); return; }
          result({ stored: request.result as StoredEntry, metadata: entry });
          metadata.put({ ...entry, touchedAt: now });
        };
      });
    });
    if (!cached) return null;
    const entry = validate(cached.stored, instanceId, shard, Date.now());
    if (!entry) return null;
    const meta = cached.metadata, snapshot = entry.snapshot;
    if (meta.session !== snapshot.session || meta.width !== snapshot.width || meta.height !== snapshot.height
      || meta.tileScale !== snapshot.tileScale || meta.runId !== snapshot.runId
      || meta.capturedAt !== snapshot.capturedAtUnixMs || meta.terrainAt !== entry.terrainAt) return null;
    const bytes = await boundedBytes({ ...entry, key }, deadline);
    if (bytes === null || bytes + METADATA_BYTE_ALLOWANCE > Math.min(MAX_ENTRY_BYTES, meta.bytes)) return null;
    return entry;
  } catch { return null; }
}

async function writeEntry(entry: DstMapCacheEntry, key: string, deadline: number): Promise<boolean> {
  const now = Date.now();
  try {
    const stored: StoredEntry = { ...entry, key }, measured = await boundedBytes(stored, deadline);
    if (measured === null || measured + METADATA_BYTE_ALLOWANCE > MAX_ENTRY_BYTES) return false;
    const bytes = measured + METADATA_BYTE_ALLOWANCE;
    return await inStores(false, deadline, (maps, metadata, result) => {
      metadataRows(maps, metadata, rows => {
        const old = rows.find(item => item.key === key), next = entry.snapshot;
        if (old && (old.capturedAt > next.capturedAtUnixMs || old.capturedAt === next.capturedAtUnixMs
          && (old.session !== next.session || old.runId !== next.runId || old.width !== next.width || old.height !== next.height || old.tileScale !== next.tileScale))) return;
        const updated: Metadata = { key, bytes, terrainAt: entry.terrainAt, touchedAt: now, capturedAt: next.capturedAtUnixMs,
          session: next.session, width: next.width, height: next.height, tileScale: next.tileScale, runId: next.runId };
        const retained = rows.filter(item => item.key !== key).sort((a, b) => b.touchedAt - a.touchedAt || a.key.localeCompare(b.key));
        let total = bytes, count = 1;
        for (const item of retained) {
          if (count >= MAX_ENTRIES || total + item.bytes > MAX_TOTAL_BYTES) { maps.delete(item.key); metadata.delete(item.key); }
          else { total += item.bytes; count++; }
        }
        maps.put(stored); metadata.put(updated); result(true);
      });
    });
  } catch { return false; }
}

export async function saveDstMapCache(value: DstMapCacheEntry): Promise<boolean> {
  const now = Date.now();
  try {
    const snapshot = value?.snapshot;
    if (!snapshot) return false;
    const key = cacheKey(snapshot.instanceId, snapshot.shard);
    if (!key) return false;
    const existing = pendingWrites.get(key);
    if (!existing && pendingWrites.size >= MAX_ENTRIES) return false;
    // Detach before yielding so later caller mutation cannot change the write.
    const entry = validate(structuredClone(value), snapshot.instanceId, snapshot.shard, now);
    if (!entry) return false;
    if (existing) {
      const old = existing.newest, next = entry.snapshot;
      if (old.capturedAtUnixMs > next.capturedAtUnixMs || old.capturedAtUnixMs === next.capturedAtUnixMs
        && (old.session !== next.session || old.runId !== next.runId || old.width !== next.width
          || old.height !== next.height || old.tileScale !== next.tileScale)) return false;
      existing.latest = entry; existing.newest = next;
      return existing.promise;
    }
    const owner: PendingWrite = { promise: Promise.resolve(false), latest: entry, newest: entry.snapshot,
      deadline: now + STORAGE_BUDGET_MS };
    pendingWrites.set(key, owner);
    owner.promise = (async () => {
      let ok = false;
      try {
        while (owner.latest && Date.now() < owner.deadline) {
          const next = owner.latest;
          owner.latest = null;
          ok = await writeEntry(next, key, owner.deadline);
          if (!ok) return false;
        }
        return ok && owner.latest === null;
      } finally {
        owner.latest = null;
        if (pendingWrites.get(key) === owner) pendingWrites.delete(key);
      }
    })();
    return owner.promise;
  } catch { return false; }
}

/** A regenerated world invalidates every shard's offline map of the instance, including writes still in flight. */
export async function forgetDstMapCache(instanceId: string): Promise<void> {
  const keys = DONTSTARVE_SHARDS.map(shard => cacheKey(instanceId, shard)).filter((key): key is string => key !== null);
  await Promise.all(keys.map(key => pendingWrites.get(key)?.promise));
  await inStores(undefined, Date.now() + STORAGE_BUDGET_MS, (maps, metadata) => {
    for (const key of keys) { maps.delete(key); metadata.delete(key); }
  });
}
