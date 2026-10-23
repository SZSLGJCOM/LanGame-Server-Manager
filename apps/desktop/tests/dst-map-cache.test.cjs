const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
require.extensions[".ts"] = (module, filename) => module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
const { readDstMapCache, saveDstMapCache, dstMapCachedView } = require("../src/dst-map-cache.ts");

// Replace only IndexedDB I/O. Validation, byte limits, eviction and stale-write
// decisions run through the production module. Browser durability is separate.
class MemoryIDB {
  stores = new Map();
  metrics = { reads: 0, getAll: 0 };
  failWrites = false;
  blocked = false;
  open() {
    const request = {};
    if (this.blocked) return request;
    setImmediate(() => {
      request.result = this.database();
      if (this.stores.size === 0) request.onupgradeneeded?.();
      request.onsuccess?.();
    });
    return request;
  }
  database() {
    const owner = this;
    return {
      objectStoreNames: { contains: name => owner.stores.has(name) },
      createObjectStore(name) { owner.stores.set(name, new Map()); },
      close() {},
      transaction(names) { return owner.transaction(names); },
    };
  }
  transaction(names) {
    const owner = this;
    const working = new Map(names.map(name => [name, new Map(owner.stores.get(name))]));
    let pending = 0, aborted = false, ended = false;
    const tx = {
      abort() { if (ended) throw new Error("Transaction finished"); aborted = true; ended = true; tx.onabort?.(); },
      objectStore(name) {
        const values = working.get(name);
        function request(action, existing) {
          const req = existing ?? {};
          pending++;
          setImmediate(() => {
            if (!aborted) {
              try { req.result = action(); req.onsuccess?.(); }
              catch (error) { tx.error = error; req.error = error; req.onerror?.(); tx.abort(); }
            }
            pending--;
            setImmediate(() => {
              if (!pending && !ended) {
                ended = true;
                for (const [key, rows] of working) owner.stores.set(key, rows);
                tx.oncomplete?.();
              }
            });
          });
          return req;
        }
        function write(action) {
          return request(() => {
            if (owner.failWrites) throw new DOMException("Quota denied", "QuotaExceededError");
            return action();
          });
        }
        return {
          get(key) { if (name === "maps") owner.metrics.reads++; return request(() => structuredClone(values.get(key))); },
          getAll() { owner.metrics.getAll++; throw new Error("Large-payload getAll must never be used"); },
          put(row) { return write(() => values.set(row.key, structuredClone(row))); },
          delete(key) { return write(() => values.delete(key)); },
          clear() { return write(() => values.clear()); },
          openCursor() {
            const keys = [...values.keys()].sort(), req = {};
            let index = 0;
            const advance = () => request(() => {
              if (index >= keys.length) return null;
              const current = keys[index];
              return { primaryKey: current, value: structuredClone(values.get(current)),
                delete() { return write(() => values.delete(current)); },
                continue() { index++; advance(); } };
            }, req);
            advance(); return req;
          },
        };
      },
    };
    return tx;
  }
}

const nativeNow = Date.now;
let now, db;
test.beforeEach(() => {
  now = nativeNow(); Date.now = () => now;
  db = new MemoryIDB(); globalThis.indexedDB = db;
});
test.afterEach(() => { Date.now = nativeNow; delete globalThis.indexedDB; });

function entry(instanceId = "server-a", shard = "master") {
  return { terrainAt: now - 1000, view: { center: { x: 1, y: 1 }, zoom: 2 }, snapshot: {
    instanceId, shard, runId: 7, capturedAtUnixMs: now, session: "world-a", geometryRevision: "15:0:30:40:1", width: 2, height: 2, tileScale: 4,
    day: 4, season: "winter", phase: "day", tiles: [1, 3, 3, 1], tileNames: { 1: "IMPASSABLE", 3: "MOD_GROUND" },
    tileKinds: { 1: "void", 3: "land" }, players: [], tileTextures: {}, tileBaseTextures: {}, tileColors: {},
    terrainLayers: [], landmarks: [], roads: [], landmarksTruncated: false,
    art: { textures: {}, icons: {}, iconFallbacks: [], issues: [], terrainBaseColor: null, terrainBaseColors: {}, terrainEdges: [] },
  } };
}
const key = (id, shard = "master") => JSON.stringify([id, shard]);

test("snapshots survive independent reads, retain distinct terrain/player timestamps and never gain live state", async () => {
  const value = entry();
  assert.equal(await saveDstMapCache(value), true);
  const restored = await readDstMapCache("server-a", "master");
  assert.deepEqual(restored, value);
  assert.equal("live" in restored, false);
  value.snapshot.tiles[0] = 3;
  assert.equal((await readDstMapCache("server-a", "master")).snapshot.tiles[0], 1);
  assert.equal(db.metrics.getAll, 0);
});

test("terrain refresh completion can be later than native player capture; malformed cameras are discarded", async () => {
  const value = entry(); value.terrainAt = now; value.snapshot.capturedAtUnixMs = now - 10;
  value.view = { center: { x: -1, y: 1 }, zoom: 2 };
  assert.equal(await saveDstMapCache(value), true);
  const restored = await readDstMapCache("server-a", "master");
  assert.equal(restored.terrainAt, now);
  assert.equal(restored.snapshot.capturedAtUnixMs, now - 10);
  assert.equal(restored.view, null);
});

test("close-up cameras above the old tenfold preset remain valid through the shared hard ceiling", async () => {
  const value = entry(); value.view.zoom = 48;
  assert.equal(await saveDstMapCache(value), true);
  assert.equal((await readDstMapCache("server-a", "master")).view.zoom, 48);
  value.view.zoom = 513;
  assert.equal(await saveDstMapCache(value), true);
  assert.equal((await readDstMapCache("server-a", "master")).view, null);
});

test("identity isolates instances/shards and cameras require the same native session and geometry", async () => {
  const value = entry();
  assert.equal(await saveDstMapCache(value), true);
  assert.equal(await readDstMapCache("server-b", "master"), null);
  assert.equal(await readDstMapCache("server-a", "caves"), null);
  for (const changed of [{ instanceId: "b" }, { shard: "caves" }, { session: "other" }, { width: 3 }, { height: 3 }, { tileScale: 8 }, { geometryRevision: "15:1:30:40:2" }]) {
    assert.equal(dstMapCachedView(value, { ...value.snapshot, ...changed }), null);
  }
  assert.deepEqual(dstMapCachedView(value, { ...value.snapshot, runId: 8 }), value.view);
});

test("persisted data is revalidated instead of trusting metadata or an old DTO", async () => {
  assert.equal(await saveDstMapCache(entry()), true);
  const stored = db.stores.get("maps").get(key("server-a"));
  stored.snapshot.instanceId = "different-instance";
  assert.equal(await readDstMapCache("server-a", "master"), null);
  stored.snapshot.instanceId = "server-a";
  stored.snapshot.tiles = null;
  assert.equal(await readDstMapCache("server-a", "master"), null);
  assert.equal(await saveDstMapCache({ ...entry(), terrainAt: now + 1 }), false);
  const bad = entry(); bad.snapshot.players = [{ id: "bad", x: Infinity }];
  assert.equal(await saveDstMapCache(bad), false);
});

test("snapshots without virtual room geometry evidence are discarded", async () => {
  assert.equal(await saveDstMapCache(entry()), true);
  delete db.stores.get("maps").get(key("server-a")).snapshot.geometryRevision;
  assert.equal(await readDstMapCache("server-a", "master"), null);
});

test("older collection cannot replace newer data but equal collection can update the camera", async () => {
  const newer = entry();
  assert.equal(await saveDstMapCache(newer), true);
  const older = entry(); older.snapshot.capturedAtUnixMs -= 500; older.view.zoom = 4;
  assert.equal(await saveDstMapCache(older), false);
  assert.equal((await readDstMapCache("server-a", "master")).view.zoom, 2);
  newer.view.zoom = 4;
  assert.equal(await saveDstMapCache(newer), true);
  assert.equal((await readDstMapCache("server-a", "master")).view.zoom, 4);
  const collision = entry(); collision.snapshot.session = "other";
  assert.equal(await saveDstMapCache(collision), false);
});

test("immediate stop/read joins a save still measuring terrain instead of restoring the prior player set", async () => {
  const old = entry();
  old.snapshot.width = 200; old.snapshot.height = 200; old.snapshot.tiles = Array(40000).fill(3);
  assert.equal(await saveDstMapCache(old), true);
  now++;
  const newer = structuredClone(old); newer.snapshot.capturedAtUnixMs = now;
  newer.snapshot.players = [{ id: "KU_NEW", name: "New", character: "wilson", icon: null, x: 0, z: 0, mapX: 1, mapZ: 1 }];
  const write = saveDstMapCache(newer);
  const restored = await readDstMapCache("server-a", "master");
  assert.equal(await write, true);
  assert.equal(restored.snapshot.players[0].id, "KU_NEW");
});

test("pending same-key saves coalesce to the latest camera and reject older captures", async () => {
  const first = entry();
  first.snapshot.width = 200; first.snapshot.height = 200; first.snapshot.tiles = Array(40000).fill(3);
  const writes = [saveDstMapCache(first)];
  for (let zoom = 3; zoom <= 12; zoom++) writes.push(saveDstMapCache({ ...first, view: { center: { x: 1, y: 1 }, zoom } }));
  const older = structuredClone(first); older.snapshot.capturedAtUnixMs--;
  assert.equal(await saveDstMapCache(older), false);
  const restored = await readDstMapCache("server-a", "master");
  assert.ok((await Promise.all(writes)).every(Boolean));
  assert.equal(restored.view.zoom, 12);
});

test("at most eight pending keys are owned and failure releases every worker slot", async () => {
  db.blocked = true;
  const writes = Array.from({ length: 8 }, (_, index) => saveDstMapCache(entry(`pending-${index}`)));
  assert.equal(await saveDstMapCache(entry("overflow")), false);
  assert.ok((await Promise.all(writes)).every(value => value === false));
  db.blocked = false;
  assert.equal(await saveDstMapCache(entry("after-timeout")), true);
});

test("metadata cannot impersonate a different world or understate the selected payload budget", async () => {
  assert.equal(await saveDstMapCache(entry()), true);
  const meta = db.stores.get("metadata").get(key("server-a"));
  meta.session = "different-world";
  assert.equal(await readDstMapCache("server-a", "master"), null);
  meta.session = "world-a"; meta.bytes = 1;
  assert.equal(await readDstMapCache("server-a", "master"), null);
});

test("expired data is removed and reads touch only the selected large map, with an eight-map LRU", async () => {
  for (let i = 0; i < 8; i++) { now++; assert.equal(await saveDstMapCache(entry(`s${i}`)), true); }
  const before = db.metrics.reads;
  now++; assert.ok(await readDstMapCache("s0", "master"));
  assert.equal(db.metrics.reads, before + 1);
  now++; assert.equal(await saveDstMapCache(entry("s8")), true);
  assert.equal(db.stores.get("maps").size, 8);
  assert.equal(await readDstMapCache("s1", "master"), null);
  assert.ok(await readDstMapCache("s0", "master"));
  now += 30 * 24 * 60 * 60 * 1000 + 1;
  assert.equal(await readDstMapCache("s0", "master"), null);
  assert.equal(db.stores.get("maps").size, 0);
  assert.equal(db.stores.get("metadata").size, 0);
});

test("byte budget evicts by metadata without loading other large maps", async () => {
  for (let i = 0; i < 4; i++) { now++; assert.equal(await saveDstMapCache(entry(`s${i}`)), true); }
  // Metadata represents otherwise existing large maps; no artificial huge DTO is parsed.
  for (const row of db.stores.get("metadata").values()) row.bytes = 20 * 1024 * 1024;
  const before = db.metrics.reads;
  now++; assert.equal(await saveDstMapCache(entry("new")), true);
  const rows = [...db.stores.get("metadata").values()];
  assert.ok(rows.reduce((total, row) => total + row.bytes, 0) <= 64 * 1024 * 1024);
  assert.equal(db.metrics.reads, before);
  assert.equal(db.metrics.getAll, 0);
});

test("quota abort rolls back payload, metadata and eviction together; denied storage remains optional", async () => {
  assert.equal(await saveDstMapCache(entry()), true);
  const before = structuredClone(db.stores);
  db.failWrites = true;
  assert.equal(await saveDstMapCache(entry("new")), false);
  assert.deepEqual(db.stores, before);
  delete globalThis.indexedDB;
  assert.equal(await readDstMapCache("server-a", "master"), null);
  assert.equal(await saveDstMapCache(entry()), false);
});

test("a protocol-valid payload beyond20MiB is rejected before opening storage", async () => {
  const value = entry(), name = "x".repeat(128);
  value.snapshot.landmarks = Array.from({ length: 32768 }, (_, index) => ({
    id: String(index), prefab: name, icon: name, priority: 0, mapX: 1, mapZ: 1,
  }));
  const png = "data:image/png;base64," + "A".repeat(450000);
  value.snapshot.art.textures = Object.fromEntries(Array.from({ length: 24 }, (_, index) => [`noise-${index}`, png]));
  assert.equal(await saveDstMapCache(value), false);
  assert.equal(db.stores.size, 0);
});

test("a blocked database settles within the bounded deadline instead of blocking live collection", async () => {
  db.blocked = true;
  const start = performance.now();
  assert.equal(await readDstMapCache("server-a", "master"), null);
  const elapsed = performance.now() - start;
  assert.ok(elapsed >= 1400 && elapsed < 3000, `Unexpected deadline: ${elapsed}`);
});
