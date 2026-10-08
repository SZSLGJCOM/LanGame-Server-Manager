'use strict';
const test = require('node:test');
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const fs = require('node:fs/promises');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const fixture = require('./fixtures/desktop_update_signature.json');
const { parsePackets, verify, hashInstaller, validateTrustedComment, proveTamperingIsRejected } = require('../verify_desktop_update_signature.cjs');
const config = { plugins: { updater: { pubkey: fixture.pubkey } } };
const data = Buffer.from(fixture.dataUnit.repeat(fixture.repeats));
const digest = crypto.createHash('blake2b512').update(data).digest();

test('public fixture verifies content and trusted comment (also used by Rust transport tests)', () => {
  assert.equal(crypto.createHash('sha256').update(data).digest('hex'), fixture.sha256);
  const packet = parsePackets(config, Buffer.from(fixture.signature));
  assert.deepEqual(verify(digest, packet), { content: true, global: true });
  validateTrustedComment(packet.trustedComment, '99.0.0');
  proveTamperingIsRejected(digest, packet);
});

test('independent minisign-verify 0.2.5 public ED test vector interoperates', () => {
  // Public protocol vector from jedisct1/rust-minisign-verify (ISC), lib.rs
  // verify_prehashed: the payload is the four ASCII bytes "test".
  const pub = 'untrusted comment: minisign public key E7620F1842B4E81F\nRWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3';
  const sig = 'untrusted comment: signature from minisign secret key\nRUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=\ntrusted comment: timestamp:1556193335\tfile:test\ny/rUw2y8/hOUYjZU71eHp/Wo1KZ40fGy2VJEDl34XMJM+TX48Ss/17u3IvIfbVR1FkZZSNCisQbuQY+bHwhEBg==';
  const packet = parsePackets({ plugins: { updater: { pubkey: Buffer.from(pub).toString('base64') } } }, Buffer.from(Buffer.from(sig).toString('base64')));
  assert.deepEqual(verify(crypto.createHash('blake2b512').update('test').digest(), packet), { content: true, global: true });
  assert.throws(() => validateTrustedComment(packet.trustedComment, '99.0.0'), /version/);
});

test('valid signatures for missing or wrong versions cannot relabel a release', () => {
  for (const name of ['missingVersionSignature', 'wrongVersionSignature']) {
    const packet = parsePackets(config, Buffer.from(fixture[name]));
    assert.deepEqual(verify(digest, packet), { content: true, global: true });
    assert.throws(() => validateTrustedComment(packet.trustedComment, '99.0.0'), /version/);
  }
});

test('trusted filename and duplicate field boundaries are checked', () => {
  for (const value of ['version:99.0.0\tfile:other.exe', 'version:99.0.0\tversion:99.0.0\tfile:LanGame Server Manager_99.0.0_x64-setup.exe']) {
    assert.throws(() => validateTrustedComment(Buffer.from(value), '99.0.0'));
  }
});

test('wrong key ID and malformed wrappers fail closed', () => {
  const decoded = Buffer.from(config.plugins.updater.pubkey, 'base64').toString().split('\n');
  const packet = Buffer.from(decoded[1], 'base64'); packet[2] ^= 1; decoded[1] = packet.toString('base64');
  const wrong = { plugins: { updater: { pubkey: Buffer.from(decoded.join('\n')).toString('base64') } } };
  assert.throws(() => parsePackets(wrong, Buffer.from(fixture.signature)), /key ID/);
  assert.throws(() => parsePackets(config, Buffer.from(fixture.signature + '!')), /base64/);
});

test('streamed file digest matches the independent public fixture', async () => {
  const directory = await fs.mkdtemp(path.join(os.tmpdir(), 'lgsm-signature-test-'));
  try {
    const file = path.join(directory, 'payload'); await fs.writeFile(file, data);
    const hashed = await hashInstaller(file);
    assert.equal(hashed.sha256, fixture.sha256);
    assert.ok(hashed.digest.equals(digest));
  } finally { await fs.rm(directory, { recursive: true, force: true }); }
});

test('CLI checks signed release identity and rejects changed bytes before claiming verification', async () => {
  const directory = await fs.mkdtemp(path.join(os.tmpdir(), 'lgsm-signature-cli-'));
  try {
    const installer = path.join(directory, 'LanGame.Server.Manager_99.0.0_x64-setup.exe');
    const configPath = path.join(directory, 'config.json');
    const signature = installer + '.sig';
    await fs.writeFile(configPath, JSON.stringify(config));
    await fs.writeFile(signature, fixture.signature);
    await fs.writeFile(installer, data);
    const run = version => spawnSync(process.execPath, [path.resolve(__dirname, '../verify_desktop_update_signature.cjs'),
      '--config', configPath, '--installer', installer, '--signature', signature, '--version', version],
      { encoding: 'utf8', timeout: 10000, windowsHide: true });
    assert.equal(run('99.0.0').status, 0);
    assert.notEqual(run('98.0.0').status, 0);
    const changed = Buffer.from(data); changed[0] ^= 1;
    await fs.writeFile(installer, changed);
    assert.notEqual(run('99.0.0').status, 0);
    await fs.writeFile(installer, data);
    await fs.writeFile(signature, fixture.wrongVersionSignature);
    assert.notEqual(run('99.0.0').status, 0);
  } finally { await fs.rm(directory, { recursive: true, force: true }); }
});
