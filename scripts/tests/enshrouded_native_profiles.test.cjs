'use strict';
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { validateProfiles } = require('../enshrouded_control_package.cjs');

const ROOT = path.resolve(__dirname, '../..');
const CONTROL = 'modules/enshrouded/control';
const RUST_IO = 'apps/desktop/src-tauri/src/live_players/enshrouded_native_io.rs';

function read(relative) {
  return fs.readFileSync(path.join(ROOT, relative), 'utf8');
}
function inventory() {
  const profiles = JSON.parse(read(`${CONTROL}/supported-builds.json`));
  validateProfiles(profiles);
  return profiles;
}
function initializer(source, declaration, label) {
  const match = source.match(declaration);
  assert.ok(match, `Cannot locate ${label}`);
  return match[1].replace(/\/\*[\s\S]*?\*\//g, '').replace(/\/\/[^\r\n]*/g, '');
}
function uniqueHashes(hashes, label) {
  assert.ok(hashes.length > 0, `${label} must register at least one executable`);
  for (const hash of hashes) assert.match(hash, /^[a-f0-9]{64}$/, `${label} has an invalid SHA256`);
  assert.equal(new Set(hashes).size, hashes.length, `${label} has duplicate executable hashes`);
  return [...hashes].sort();
}

test('supported profile inventory has valid, unique build and executable identities', () => {
  assert.doesNotThrow(inventory);
});

test('native admission covers exactly the declared executable profiles without duplicates', () => {
  const body = initializer(read(`${CONTROL}/native_profile.cpp`),
    /\bconstexpr\s+Build\s+kBuilds\s*\[\s*\]\s*=\s*\{([\s\S]*?)^\s*\};/m, 'C++ kBuilds');
  const hashes = [...body.matchAll(/\{\s*"([^"]+)"\s*,/g)].map((match) => match[1]);
  assert.deepEqual(uniqueHashes(hashes, 'C++ kBuilds'),
    uniqueHashes(inventory().map((profile) => profile.executable_sha256), 'supported inventory'));
});

test('bundled Rust installation admits exactly the native profiles including its baseline constant', () => {
  const source = read(RUST_IO);
  const baseline = source.match(/\bconst\s+EXE_SHA256\s*:\s*&str\s*=\s*"([^"]+)"\s*;/);
  assert.ok(baseline, 'Cannot locate Rust EXE_SHA256');
  const body = initializer(source,
    /\bconst\s+BUNDLED_EXE_SHA256\s*:\s*&\[\s*&str\s*\]\s*=\s*&\[([\s\S]*?)\]\s*;/,
    'Rust BUNDLED_EXE_SHA256');
  const hashes = body.split(',').map((entry) => entry.trim()).filter(Boolean).map((entry) => {
    if (entry === 'EXE_SHA256') return baseline[1];
    const literal = entry.match(/^"([^"]+)"$/);
    assert.ok(literal, `Unrecognized bundled executable entry: ${entry}`);
    return literal[1];
  });
  assert.deepEqual(uniqueHashes(hashes, 'Rust BUNDLED_EXE_SHA256'),
    uniqueHashes(inventory().map((profile) => profile.executable_sha256), 'supported inventory'));
});

test('inert PE tests cover every declared build and executable identity without duplicates', () => {
  const body = initializer(read(`${CONTROL}/tests/native_profile_tests.cpp`),
    /\bconstexpr\s+Expected\s+profiles\s*\[\s*\]\s*=\s*\{([\s\S]*?)^\s*\};/m,
    'native profile test expectations');
  const profiles = [...body.matchAll(/\{\s*"([^"]+)"\s*,\s*"([^"]+)"\s*,/g)]
    .map((match) => ({ game_build_id: match[2], executable_sha256: match[1] }));
  validateProfiles(profiles);
  const ordered = (items) => [...items].sort((a, b) => a.game_build_id.localeCompare(b.game_build_id));
  assert.deepEqual(ordered(profiles), ordered(inventory()));
});
