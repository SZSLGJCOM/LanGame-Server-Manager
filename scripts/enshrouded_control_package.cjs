'use strict';

// Independent extension trust root. Output signatures use the standard minisign
// ED (Ed25519 over Blake2b-512) format, including its signed trusted comment.
const fs = require('node:fs/promises');
const path = require('node:path');
const crypto = require('node:crypto');
const { parsePackets, verify } = require('./verify_desktop_update_signature.cjs');

const ROOT = path.resolve(__dirname, '..');
const PUBLIC_KEY = path.join(ROOT, 'modules/enshrouded/control/update-public-key.pub');
const MAX_DLL = 1024 * 1024;
const VERSION = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;
const SHA = /^[a-f0-9]{64}$/;
const BUILD = /^[1-9]\d{0,19}$/;

function requireValue(condition, message) { if (!condition) throw new Error(message); }
function sha256(bytes) { return crypto.createHash('sha256').update(bytes).digest('hex'); }
function fields(object, expected, label) {
  requireValue(object && typeof object === 'object' && !Array.isArray(object)
    && Object.keys(object).sort().join(',') === [...expected].sort().join(','), `${label}: unexpected fields`);
}
function publicPacket(text) {
  const lines = text.trimEnd().split(/\r?\n/);
  requireValue(lines.length === 2 && lines[0].startsWith('untrusted comment: '), 'Invalid public key');
  const packet = Buffer.from(lines[1], 'base64');
  requireValue(packet.length === 42 && packet.subarray(0, 2).equals(Buffer.from('Ed'))
    && packet.toString('base64') === lines[1], 'Invalid public key packet');
  return packet;
}
function sign(bytes, key, packet, comment) {
  requireValue(/^[\x20-\x7e]+$/.test(comment), 'Trusted comment must be printable ASCII');
  const signature = crypto.sign(null, crypto.createHash('blake2b512').update(bytes).digest(), key);
  const signaturePacket = Buffer.concat([Buffer.from('ED'), packet.subarray(2, 10), signature]);
  const global = crypto.sign(null, Buffer.concat([signature, Buffer.from(comment)]), key);
  return `untrusted comment: LanGame Enshrouded control signature\n${signaturePacket.toString('base64')}\ntrusted comment: ${comment}\n${global.toString('base64')}\n`;
}
function verifySignature(bytes, signature, publicText, comment) {
  const config = { plugins: { updater: { pubkey: Buffer.from(publicText).toString('base64') } } };
  const parsed = parsePackets(config, Buffer.from(Buffer.from(signature).toString('base64')));
  requireValue(parsed.trustedComment.toString('ascii') === comment, 'Unexpected signed trusted comment');
  const result = verify(crypto.createHash('blake2b512').update(bytes).digest(), parsed);
  requireValue(result.content && result.global, 'Extension signature verification failed');
}
function validateProfiles(profiles) {
  requireValue(Array.isArray(profiles) && profiles.length > 0 && profiles.length <= 64, 'Expected 1..64 verified profiles');
  const builds = new Set(); const hashes = new Set();
  for (const profile of profiles) {
    fields(profile, ['game_build_id', 'executable_sha256'], 'profile');
    requireValue(typeof profile.game_build_id === 'string' && BUILD.test(profile.game_build_id)
      && typeof profile.executable_sha256 === 'string' && SHA.test(profile.executable_sha256), 'Invalid verified build identity');
    requireValue(!builds.has(profile.game_build_id) && !hashes.has(profile.executable_sha256), 'Duplicate profile identity');
    builds.add(profile.game_build_id); hashes.add(profile.executable_sha256);
  }
}
function validateManifest(manifest) {
  fields(manifest, ['schema', 'module_id', 'protocol', 'target', 'version', 'revision', 'profiles', 'dll'], 'manifest');
  requireValue(manifest.schema === 1 && manifest.module_id === 'enshrouded' && manifest.protocol === 1
    && manifest.target === 'windows-x86_64', 'Unexpected extension contract');
  requireValue(typeof manifest.version === 'string' && manifest.version.length <= 48 && VERSION.test(manifest.version)
    && manifest.version.split('.').every((part) => Number(part) <= 0xffffffff), 'Invalid extension version');
  requireValue(Number.isSafeInteger(manifest.revision) && manifest.revision > 0, 'Revision must be a positive safe integer');
  validateProfiles(manifest.profiles);
  fields(manifest.dll, ['asset_name', 'sha256', 'size'], 'DLL');
  requireValue(manifest.dll.asset_name === `Lgsm.Enshrouded.Control_${manifest.version}_windows-x86_64.dll`
    && SHA.test(manifest.dll.sha256) && Number.isInteger(manifest.dll.size)
    && manifest.dll.size > 0 && manifest.dll.size <= MAX_DLL, 'Invalid DLL contract');
  return manifest;
}
function validateDll(bytes) {
  requireValue(bytes.length >= 128 && bytes.length <= MAX_DLL && bytes.subarray(0, 2).toString() === 'MZ', 'Expected a bounded Windows DLL');
  const pe = bytes.readUInt32LE(0x3c);
  requireValue(pe >= 64 && pe + 26 <= bytes.length && bytes.subarray(pe, pe + 4).equals(Buffer.from('PE\0\0'))
    && bytes.readUInt16LE(pe + 4) === 0x8664 && (bytes.readUInt16LE(pe + 22) & 0x2000) !== 0
    && bytes.readUInt16LE(pe + 24) === 0x20b, 'Expected a Windows x64 PE32+ DLL');
}
async function readRegular(file, limit) {
  const stat = await fs.lstat(file);
  requireValue(stat.isFile() && !stat.isSymbolicLink() && stat.size > 0 && stat.size <= limit, 'Expected a bounded ordinary input file');
  const result = await fs.readFile(file);
  requireValue(result.length === stat.size, 'Input changed during read');
  return result;
}
async function initialize(privatePath, publicPath) {
  requireValue(path.isAbsolute(privatePath) && path.isAbsolute(publicPath), 'Key paths must be absolute');
  const relative = path.relative(ROOT, privatePath);
  requireValue(relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative), 'Private keys must stay outside the repository');
  const resolvedParent = await fs.realpath(path.dirname(privatePath));
  const resolvedRelative = path.relative(await fs.realpath(ROOT), resolvedParent);
  requireValue(resolvedRelative.startsWith(`..${path.sep}`) || path.isAbsolute(resolvedRelative), 'Private key directory resolves inside the repository');
  const { privateKey, publicKey } = crypto.generateKeyPairSync('ed25519');
  const keyId = crypto.randomBytes(8);
  const raw = publicKey.export({ format: 'der', type: 'spki' }).subarray(-32);
  const packet = Buffer.concat([Buffer.from('Ed'), keyId, raw]);
  const publicText = `untrusted comment: LanGame Enshrouded control independent public key\n${packet.toString('base64')}\n`;
  // The caller must create and ACL-protect the private directory before this call.
  await fs.writeFile(privatePath, privateKey.export({ format: 'pem', type: 'pkcs8' }), { flag: 'wx', mode: 0o600 });
  try { await fs.writeFile(publicPath, publicText, { flag: 'wx' }); }
  catch (error) { await fs.unlink(privatePath); throw error; }
  return { initialized: true, publicKeyPath: publicPath };
}
async function packageExtension(options) {
  const dllBytes = await readRegular(options['--dll'], MAX_DLL); validateDll(dllBytes);
  const profiles = JSON.parse((await readRegular(options['--profiles'], 32768)).toString('utf8')); validateProfiles(profiles);
  const receipt = JSON.parse((await readRegular(options['--acceptance'], 65536)).toString('utf8'));
  requireValue(receipt.schema === 1 && receipt.dll_sha256 === sha256(dllBytes)
    && JSON.stringify(receipt.profiles) === JSON.stringify(profiles)
    && Array.isArray(receipt.verified_actions) && ['kick', 'ban', 'unban'].every((action) => receipt.verified_actions.includes(action))
    && typeof receipt.evidence === 'string' && receipt.evidence.trim().length > 0,
  'Acceptance receipt must bind this DLL and profiles to real kick, ban and unban evidence');
  const manifest = validateManifest({ schema: 1, module_id: 'enshrouded', protocol: 1, target: 'windows-x86_64',
    version: options['--version'], revision: Number(options['--revision']), profiles,
    dll: { asset_name: `Lgsm.Enshrouded.Control_${options['--version']}_windows-x86_64.dll`, sha256: sha256(dllBytes), size: dllBytes.length } });
  const publicText = (await readRegular(options['--public-key'] || PUBLIC_KEY, 8192)).toString('ascii');
  const packet = publicPacket(publicText);
  const privateKey = crypto.createPrivateKey(await readRegular(options['--private-key'], 8192));
  const derived = crypto.createPublicKey(privateKey).export({ format: 'der', type: 'spki' }).subarray(-32);
  requireValue(derived.equals(packet.subarray(10)), 'Private key does not match the pinned extension root');
  const manifestBytes = Buffer.from(`${JSON.stringify(manifest, null, 2)}\n`);
  const files = new Map([
    [manifest.dll.asset_name, dllBytes],
    [`${manifest.dll.asset_name}.minisig`, Buffer.from(sign(dllBytes, privateKey, packet, `lgsm-enshrouded-control-dll:${manifest.version}`))],
    ['manifest.json', manifestBytes],
    ['manifest.json.minisig', Buffer.from(sign(manifestBytes, privateKey, packet, 'lgsm-enshrouded-control-manifest:1'))],
    ['LICENSE.txt', await readRegular(path.join(ROOT, 'LICENSE'), 65536)],
    ['LICENSE.minhook.txt', await readRegular(path.join(ROOT, 'modules/enshrouded/control/vendor/minhook/LICENSE.txt'), 65536)],
  ]);
  verifySignature(dllBytes, files.get(`${manifest.dll.asset_name}.minisig`), publicText, `lgsm-enshrouded-control-dll:${manifest.version}`);
  verifySignature(manifestBytes, files.get('manifest.json.minisig'), publicText, 'lgsm-enshrouded-control-manifest:1');
  await fs.mkdir(options['--out']); // Never overwrite or empty an existing staging directory.
  for (const [name, bytes] of files) await fs.writeFile(path.join(options['--out'], name), bytes, { flag: 'wx' });
  const sums = [...files].map(([name, bytes]) => `${sha256(bytes)}  ${name}`).join('\n') + '\n';
  await fs.writeFile(path.join(options['--out'], 'SHA256SUMS'), sums, { flag: 'wx' });
  return { packaged: true, version: manifest.version, revision: manifest.revision, dllSHA256: manifest.dll.sha256 };
}
async function verifyPackage(directory, publicPath = PUBLIC_KEY) {
  const publicText = (await readRegular(publicPath, 8192)).toString('ascii');
  const manifestBytes = await readRegular(path.join(directory, 'manifest.json'), 65536);
  const signature = await readRegular(path.join(directory, 'manifest.json.minisig'), 8192);
  verifySignature(manifestBytes, signature, publicText, 'lgsm-enshrouded-control-manifest:1');
  const manifest = validateManifest(JSON.parse(manifestBytes.toString('utf8')));
  const dll = await readRegular(path.join(directory, manifest.dll.asset_name), MAX_DLL); validateDll(dll);
  requireValue(dll.length === manifest.dll.size && sha256(dll) === manifest.dll.sha256, 'DLL differs from signed manifest');
  verifySignature(dll, await readRegular(path.join(directory, `${manifest.dll.asset_name}.minisig`), 8192),
    publicText, `lgsm-enshrouded-control-dll:${manifest.version}`);
  return { verified: true, ...manifest };
}
function argumentsFrom(argv) {
  const command = argv[0]; const options = {};
  requireValue(['init', 'package', 'verify', 'verify-manifest'].includes(command), 'Expected init, package, verify or verify-manifest');
  const required = { init: ['--private-key', '--public-key'],
    package: ['--dll', '--profiles', '--acceptance', '--version', '--revision', '--private-key', '--out'],
    verify: ['--directory'], 'verify-manifest': ['--manifest', '--signature'] }[command];
  for (let i = 1; i < argv.length; i += 2) {
    requireValue([...required, '--public-key'].includes(argv[i]) && argv[i + 1] && !Object.hasOwn(options, argv[i]), 'Unknown, missing or duplicate argument');
    options[argv[i]] = argv[i + 1];
  }
  requireValue(required.every((key) => options[key]), `Required: ${required.join(' ')}`);
  for (const [name, value] of Object.entries(options)) if (!['--version', '--revision'].includes(name)) requireValue(path.isAbsolute(value), 'File paths must be absolute');
  return { command, options };
}
async function main() {
  const { command, options } = argumentsFrom(process.argv.slice(2));
  if (command === 'init') return initialize(options['--private-key'], options['--public-key']);
  if (command === 'package') return packageExtension(options);
  if (command === 'verify') return verifyPackage(options['--directory'], options['--public-key']);
  const bytes = await readRegular(options['--manifest'], 65536);
  verifySignature(bytes, await readRegular(options['--signature'], 8192),
    (await readRegular(options['--public-key'] || PUBLIC_KEY, 8192)).toString('ascii'), 'lgsm-enshrouded-control-manifest:1');
  return { verified: true, ...validateManifest(JSON.parse(bytes.toString('utf8'))) };
}
module.exports = { sign, verifySignature, validateManifest, validateProfiles, validateDll, publicPacket, packageExtension, verifyPackage, sha256 };
if (require.main === module) main().then((result) => console.log(JSON.stringify(result))).catch((error) => {
  // OpenSSL or filesystem errors can include input details. Do not print keys.
  console.error(`Enshrouded extension operation failed: ${error.code || error.message}`); process.exitCode = 1;
});
