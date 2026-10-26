const assert = require('node:assert/strict');
const fs = require('node:fs');
const test = require('node:test');
const { transpileTypeScript } = require('../scripts/typescript_source_tools.cjs');
require.extensions['.ts'] = (module, filename) => module._compile(transpileTypeScript(fs.readFileSync(filename, 'utf8'), filename), filename);
const { parseArkMap, acceptArkMapSnapshot, arkPlayerPoint, arkMapTargets } = require('../src/ark-world-map.ts');
const scope = { instanceId: 'ark-a', processKey: 'main', runId: 7 };
const response = () => ({ ...scope, mapName: 'TheIsland', sampledAtUnixMs: 1000,
  projection: { minX: -400000, maxX: 400000, minY: -400000, maxY: 400000 }, artwork: null,
  players: [{ playerId: 123, name: '玩家', accountId: '76561190000000001', team: 1000, x: 0, y: 200000, z: 500 }] });
test('ARK map binds instance, process and run and retains independent player identities', () => {
  const map = parseArkMap(response(), scope);
  assert.equal(map.players[0].playerId, 123);
  assert.equal(map.players[0].accountId, '76561190000000001');
  assert.deepEqual(arkPlayerPoint(map.players[0], map.projection), { x: 50, y: 75 });
  for (const other of [{ instanceId: 'b' }, { processKey: 'map-other' }, { runId: 8 }])
    assert.throws(() => parseArkMap({ ...response(), ...other }, scope));
});
test('ARK map rejects duplicate IDs, invalid geometry and unsafe artwork without fabricating absent coordinates', () => {
  const value = response(); value.players[0] = { ...value.players[0], x: null, y: null, z: null };
  assert.equal(arkPlayerPoint(parseArkMap(value, scope).players[0], value.projection), null);
  const native = { ...response(), projection: null };
  assert.deepEqual(arkPlayerPoint(native.players[0], null), { x: 0, y: 200000 });
  for (const change of [
    { players: [...response().players, ...response().players] },
    { players: [{ ...response().players[0], x: null }] },
    { players: [{ ...response().players[0], x: Infinity }] },
    { players: [{ ...response().players[0], playerId: 0 }] },
    { players: [{ ...response().players[0], accountId: 'x\n' }] },
    { projection: { minX: 1, maxX: 1, minY: 0, maxY: 1 } },
    { artwork: { dataUrl: 'https://example.com/map.png', source: 'external' } },
    { sampledAtUnixMs: -1 }, { players: Array(513).fill(response().players[0]) }
  ]) assert.throws(() => parseArkMap({ ...response(), ...change }, scope));
});
test('older samples cannot replace current positions and foreign runs never merge', () => {
  const current = parseArkMap(response(), scope);
  assert.equal(acceptArkMapSnapshot(current, { ...current, sampledAtUnixMs: 999 }, scope), current);
  assert.throws(() => acceptArkMapSnapshot(current, { ...current, runId: 8 }, scope));
  assert.equal(acceptArkMapSnapshot(current, { ...current, sampledAtUnixMs: 1001 }, scope).sampledAtUnixMs, 1001);
});
test('map choices use canonical configured process keys and preserve stopped enabled maps', () => {
  const choices = arkMapTargets(JSON.stringify({ map_name: 'TheIsland', additional_maps: [
    { id: 'desert', map_name: 'ScorchedEarth_P', name: '沙漠', enabled: true },
    { id: 'off', map_name: 'Aberration_P', name: '关闭', enabled: false }
  ] }));
  assert.deepEqual(choices.map(v => v.processKey), ['main', 'map-desert']);
  assert.equal(choices[1].label, '沙漠');
});
