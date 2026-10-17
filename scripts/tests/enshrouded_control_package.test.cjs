'use strict';
const test = require('node:test');
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const fs = require('node:fs/promises');
const os = require('node:os');
const path = require('node:path');
const { sign, verifySignature, validateManifest, validateProfiles, validateDll, packageExtension, verifyPackage, sha256 } = require('../enshrouded_control_package.cjs');

function keyPair() {
  const keys = crypto.generateKeyPairSync('ed25519');
  const packet = Buffer.concat([Buffer.from('Ed'), crypto.randomBytes(8), keys.publicKey.export({ format: 'der', type: 'spki' }).subarray(-32)]);
  return { ...keys, packet, text: `untrusted comment: generated test key\n${packet.toString('base64')}\n` };
}
const profiles = [{ game_build_id: '25988414', executable_sha256: 'b627639ce4a3f8c940fe90afa1d36ed59db3ff9ced24e935890eb7c99b04d5bf' }];
function fixtureDll() {
  const bytes = Buffer.alloc(128);
  bytes.write('MZ'); bytes.writeUInt32LE(64, 0x3c); bytes.write('PE\0\0', 64);
  bytes.writeUInt16LE(0x8664, 68); bytes.writeUInt16LE(0x2000, 86); bytes.writeUInt16LE(0x20b, 88);
  return bytes;
}

test('standard minisign dual signatures reject content, key, trusted-purpose and global-signature tampering', () => {
  const key = keyPair(); const bytes = Buffer.from('non executable test payload');
  const signature = sign(bytes, key.privateKey, key.packet, 'lgsm-enshrouded-control-manifest:1');
  assert.doesNotThrow(() => verifySignature(bytes, signature, key.text, 'lgsm-enshrouded-control-manifest:1'));
  assert.throws(() => verifySignature(Buffer.concat([bytes, Buffer.from('x')]), signature, key.text, 'lgsm-enshrouded-control-manifest:1'));
  assert.throws(() => verifySignature(bytes, signature, keyPair().text, 'lgsm-enshrouded-control-manifest:1'));
  assert.throws(() => verifySignature(bytes, signature, key.text, 'lgsm-enshrouded-control-dll:1.0.0'));
  const lines = signature.trimEnd().split('\n');
  const global = Buffer.from(lines[3], 'base64'); global[0] ^= 1; lines[3] = global.toString('base64');
  assert.throws(() => verifySignature(bytes, lines.join('\n'), key.text, 'lgsm-enshrouded-control-manifest:1'));
});

test('profiles cannot introduce duplicate builds or digest aliases and DLL must be Windows x64', () => {
  assert.throws(() => validateProfiles([...profiles, { ...profiles[0] }]));
  assert.throws(() => validateProfiles([{ ...profiles[0], game_build_id: '025988414' }]));
  assert.doesNotThrow(() => validateDll(fixtureDll()));
  const x86 = fixtureDll(); x86.writeUInt16LE(0x14c, 68); assert.throws(() => validateDll(x86));
});

test('packaging requires real-action receipt, keeps immutable staging and verifies signed DLL after export', async () => {
  const directory = await fs.mkdtemp(path.join(os.tmpdir(), 'enshrouded-package-test-'));
  try {
    const key = keyPair(); const dll = fixtureDll();
    const files = { '--dll': 'bridge.dll', '--profiles': 'profiles.json', '--acceptance': 'acceptance.json', '--private-key': 'fixture.pem', '--public-key': 'fixture.pub', '--out': 'staging' };
    const options = Object.fromEntries(Object.entries(files).map(([name, leaf]) => [name, path.join(directory, leaf)]));
    options['--version'] = '1.0.0'; options['--revision'] = '1';
    await fs.writeFile(options['--dll'], dll);
    await fs.writeFile(options['--profiles'], JSON.stringify(profiles));
    await fs.writeFile(options['--private-key'], key.privateKey.export({ type: 'pkcs8', format: 'pem' }));
    await fs.writeFile(options['--public-key'], key.text);
    const receipt = { schema: 1, dll_sha256: sha256(dll), profiles, verified_actions: ['kick', 'ban'], evidence: 'Synthetic fixture only' };
    await fs.writeFile(options['--acceptance'], JSON.stringify(receipt));
    await assert.rejects(packageExtension(options), /kick, ban and unban/);
    receipt.verified_actions.push('unban'); await fs.writeFile(options['--acceptance'], JSON.stringify(receipt));
    const packaged = await packageExtension(options); assert.equal(packaged.packaged, true);
    const verified = await verifyPackage(options['--out'], options['--public-key']); assert.equal(verified.dll.sha256, sha256(dll));
    await assert.rejects(packageExtension(options), { code: 'EEXIST' });
    const manifestPath = path.join(options['--out'], 'manifest.json');
    const manifest = JSON.parse(await fs.readFile(manifestPath));
    assert.throws(() => validateManifest({ ...manifest, url: 'https://example.invalid/bridge.dll' }));
    await fs.appendFile(path.join(options['--out'], verified.dll.asset_name), 'tampered');
    await assert.rejects(verifyPackage(options['--out'], options['--public-key']), /DLL differs/);
  } finally { await fs.rm(directory, { recursive: true, force: true }); }
});

test('checked-in discarded-key vector verifies while production trust rejects it', async () => {
  const directory = path.resolve(__dirname, '../../apps/desktop/src-tauri/test-data/enshrouded-extension');
  const bytes = await fs.readFile(path.join(directory, 'manifest.json'));
  const signature = await fs.readFile(path.join(directory, 'manifest.json.minisig'));
  const fixtureKey = await fs.readFile(path.join(directory, 'public-key.pub'), 'ascii');
  verifySignature(bytes, signature, fixtureKey, 'lgsm-enshrouded-control-manifest:1');
  const production = await fs.readFile(path.resolve(__dirname, '../../modules/enshrouded/control/update-public-key.pub'), 'ascii');
  assert.throws(() => verifySignature(bytes, signature, production, 'lgsm-enshrouded-control-manifest:1'));
});
