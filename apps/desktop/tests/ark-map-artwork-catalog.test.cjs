const assert = require('node:assert/strict');
const fs = require('node:fs');
const test = require('node:test');
const { transpileTypeScript } = require('../scripts/typescript_source_tools.cjs');
require.extensions['.ts'] = (module, filename) => module._compile(transpileTypeScript(fs.readFileSync(filename, 'utf8'), filename), filename);
const { findArkMapArtwork, arkArtworkRect } = require('../src/ark-map-artwork-catalog.ts');
const catalog = require('../src/data/ark-map-artwork.json');
const { isOfficialMediaUrl, preferredMediaSources } = require('../src/official-media-sources.ts');
const policy = require('../../../crates/app-network/official-sources.json');
const { loadSource } = require('./media-cache-test-support.cjs');

test('native identifiers pick edition-specific artwork and leave unknown or multi-realm maps to the grid', () => {
  const ase = findArkMapArtwork('arksurvivalevolved', 'TheIsland');
  const asa = findArkMapArtwork('arksurvivalascended', 'TheIsland_WP');
  assert.ok(ase.url.endsWith('/The_Island_Topographic_Map.jpg'));
  assert.ok(asa.url.endsWith('/The_Island_map_ASA.jpg'));
  assert.equal(findArkMapArtwork('arksurvivalascended', 'theisland_wp'), asa);
  for (const [edition, map] of [['arksurvivalevolved', 'TheIsland_WP'], ['arksurvivalascended', 'TheIsland'],
    ['arksurvivalascended', 'CustomMap_WP'], ['arksurvivalevolved', 'Genesis'], ['arksurvivalevolved', 'Fjordur']]) {
    assert.equal(findArkMapArtwork(edition, map), null);
  }
});
test('full image GPS rectangles retain margins and non-square calibration without changing player coordinates', () => {
  assert.deepEqual(arkArtworkRect(findArkMapArtwork('arksurvivalascended', 'TheIsland_WP').bounds), { x: 0, y: 0, width: 1000, height: 1000 });
  assert.deepEqual(arkArtworkRect(findArkMapArtwork('arksurvivalevolved', 'TheIsland').bounds), { x: 72, y: 72, width: 856, height: 856 });
  const center = arkArtworkRect(findArkMapArtwork('arksurvivalevolved', 'TheCenter').bounds);
  assert.deepEqual(center, { x: 10, y: -25, width: 1035, height: 1035 });
});
test('every reviewed catalog image has a matching narrow cache permission and bounded source metadata', () => {
  const keys = new Set(), paths = new Set();
  for (const entry of catalog.maps) {
    const key = `${entry.edition}:${entry.mapName.toLowerCase()}`;
    assert.ok(!keys.has(key)); keys.add(key);
    const art = findArkMapArtwork(entry.edition, entry.mapName);
    assert.match(entry.image, /^[A-Za-z0-9_() -]+\.jpg$/);
    assert.ok(isOfficialMediaUrl(art.url, 'image'));
    assert.equal(isOfficialMediaUrl(art.url, 'video'), false);
    assert.ok(art.width >= 128 && art.width <= 4096 && art.height >= 128 && art.height <= 4096);
    assert.ok(Object.values(art.bounds).every(value => Number.isFinite(value) && Math.abs(value) <= 150));
    assert.ok(art.bounds.minX < art.bounds.maxX && art.bounds.minY < art.bounds.maxY);
    assert.match(entry.coordinateSource, /^https:\/\/ark\.wiki\.gg\/wiki\/Data:Maps\/.+\?oldid=\d+$/);
    assert.equal(art.sourceUrl, `https://ark.wiki.gg/wiki/File:${entry.image}`);
    paths.add(new URL(art.url).pathname);
  }
  const group = policy.groups.find(entry => entry.id === 'ark-wiki-map-artwork');
  assert.deepEqual(new Set(group.exactPaths), paths, 'The cache grants only actual catalog resources');
  assert.deepEqual(group.pathPrefixes, []); assert.deepEqual(group.allowedQueryKeys, []);
});

test('verified mirror pairs precede the separate Wiki fallback and never attach to another map', () => {
  const github = 'https://raw.githubusercontent.com/fixture/maps/master/The_Island_map_ASA.jpg';
  const gitcode = 'https://api.gitcode.com/api/v5/repos/fixture/maps/raw/The_Island_map_ASA.jpg?ref=main';
  const candidatePolicy = { ...policy, exactResources: [...policy.exactResources.filter(resource => !resource.id.startsWith('ark-map-')), {
    id: 'ark-map-asa-theisland_wp', urls: [github, gitcode], purposes: ['image']
  }] };
  const catalogWithMirrors = loadSource('ark-map-artwork-catalog.ts', {
    '../../../crates/app-network/official-sources.json': candidatePolicy
  });
  const art = catalogWithMirrors.findArkMapArtwork('arksurvivalascended', 'TheIsland_WP');
  assert.deepEqual(Array.from(art.sources), [github, gitcode, art.url]);
  assert.deepEqual(Array.from(catalogWithMirrors.findArkMapArtwork('arksurvivalevolved', 'TheIsland').sources),
    ['https://ark.wiki.gg/images/The_Island_Topographic_Map.jpg']);
  const mediaWithMirrors = loadSource('official-media-sources.ts', {
    '../../../crates/app-network/official-sources.json': { ...candidatePolicy,
      chinaOrigins: [...candidatePolicy.chinaOrigins, 'https://api.gitcode.com'] }
  });
  assert.deepEqual(Array.from(mediaWithMirrors.preferredMediaSources(art.sources, Date.now(), 'zh-CN')), [gitcode, github, art.url]);
  assert.deepEqual(Array.from(mediaWithMirrors.preferredMediaSources(art.sources, Date.now(), 'en-US')), [github, gitcode, art.url]);
  assert.equal(mediaWithMirrors.isOfficialMediaUrl(`${gitcode}&access_token=fixture`, 'image'), false);
  assert.equal(mediaWithMirrors.isOfficialMediaUrl(github, 'video'), false);
});

test('published map mirrors pin exact commits and byte identities with locale-specific ordering', () => {
  const mirrors = policy.exactResources.filter(resource => resource.id.startsWith('ark-map-'));
  assert.equal(mirrors.length, catalog.maps.length);
  const used = new Set();
  for (const entry of catalog.maps) {
    const art = findArkMapArtwork(entry.edition, entry.mapName);
    const resource = mirrors.find(value => value.urls.includes(art.sources[0]));
    assert.ok(resource, entry.mapName);
    assert.ok(!used.has(resource.id), 'Each edition/map has its own mirror identity');
    used.add(resource.id);
    assert.match(resource.sha256, /^[0-9a-f]{64}$/);
    assert.equal(resource.cacheImmutable, true, 'Frozen maps are explicitly retained as downloaded resources');
    assert.deepEqual(resource.purposes, ['image']);
    assert.equal(resource.urls.length, 2);
    const [github, gitcode] = resource.urls;
    assert.match(github, /^https:\/\/raw\.githubusercontent\.com\/SZSLGJCOM\/LanGame-Server-Manager\/[0-9a-f]{40}\/assets\/ark-map-artwork\/\d{4}-\d{2}-\d{2}\//);
    assert.match(gitcode, /^https:\/\/api\.gitcode\.com\/api\/v5\/repos\/SZSLGJCOM\/LanGame-Server-Manager-Releases\/raw\/assets\/ark-map-artwork\/\d{4}-\d{2}-\d{2}\/.*\?ref=[0-9a-f]{40}$/);
    for (const url of resource.urls) {
      assert.equal(decodeURIComponent(new URL(url).pathname.split('/').at(-1)), entry.image);
      assert.equal(isOfficialMediaUrl(url, 'image'), true);
      assert.equal(isOfficialMediaUrl(url, 'video'), false);
      assert.equal(isOfficialMediaUrl(`${url}${url.includes('?') ? '&' : '?'}access_token=fixture`, 'image'), false);
      assert.equal(isOfficialMediaUrl(url.replace(entry.image, 'Unreviewed_Map.jpg'), 'image'), false);
    }
    assert.deepEqual(art.sources, [github, gitcode, art.url]);
    assert.deepEqual(preferredMediaSources(art.sources, 0, 'zh-CN'), [gitcode, github, art.url]);
    assert.deepEqual(preferredMediaSources(art.sources, 0, 'en-US'), [github, gitcode, art.url]);
  }
});
