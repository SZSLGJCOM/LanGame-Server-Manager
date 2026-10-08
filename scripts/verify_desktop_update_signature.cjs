'use strict';

const fs = require('node:fs/promises');
const path = require('node:path');
const crypto = require('node:crypto');

// minisign-verify 0.2.5, src/lib.rs:234-369: ED signs Blake2b-512;
// the global signature signs the 64-byte signature and trusted comment body.
const SPKI_PREFIX = Buffer.from('302a300506032b6570032100', 'hex');
const TRUSTED_PREFIX = 'trusted comment: ';

function requireCondition(condition, message) {
  if (!condition) throw new Error(message);
}

function argumentsFrom(argv) {
  const options = {};
  for (let index = 0; index < argv.length; index += 2) {
    const key = argv[index];
    requireCondition(['--config', '--installer', '--signature', '--version'].includes(key), 'Unknown argument');
    requireCondition(argv[index + 1] && !Object.hasOwn(options, key), 'Missing or duplicate argument');
    if (key !== '--version') requireCondition(path.isAbsolute(argv[index + 1]), 'Input paths must be absolute');
    options[key] = argv[index + 1];
  }
  requireCondition(Object.keys(options).length === 4, 'Required: --config PATH --installer PATH --signature PATH --version VERSION');
  requireCondition(/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(options['--version']), 'Expected stable release version');
  return options;
}

function base64(text, label) {
  requireCondition(typeof text === 'string' && text.length > 0, `${label}: missing base64`);
  requireCondition(/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(text), `${label}: invalid base64`);
  const decoded = Buffer.from(text, 'base64');
  requireCondition(decoded.toString('base64') === text, `${label}: noncanonical base64`);
  return decoded;
}

function ascii(buffer, label) {
  requireCondition(buffer.every((byte) => byte === 9 || byte === 10 || byte === 13 || (byte >= 32 && byte <= 126)), `${label}: expected ASCII text`);
  return buffer.toString('ascii');
}

function lines(buffer, count, label) {
  let text = ascii(buffer, label).replace(/\r\n/g, '\n');
  requireCondition(!text.includes('\r'), `${label}: invalid line ending`);
  if (text.endsWith('\n')) text = text.slice(0, -1);
  const result = text.split('\n');
  requireCondition(result.length === count && result.every((line) => line.length > 0), `${label}: unexpected line count`);
  requireCondition(result[0].startsWith('untrusted comment: '), `${label}: missing untrusted comment`);
  return result;
}

async function readSmall(filePath, limit) {
  const file = await fs.open(filePath, 'r');
  try {
    const stat = await file.stat();
    requireCondition(stat.isFile() && stat.size > 0 && stat.size <= limit, 'Invalid metadata file size or type');
    const buffer = Buffer.alloc(limit + 1);
    let length = 0;
    while (length < buffer.length) {
      const { bytesRead } = await file.read(buffer, length, buffer.length - length, null);
      if (bytesRead === 0) break;
      length += bytesRead;
    }
    requireCondition(length <= limit, 'Metadata file exceeds its size limit');
    return buffer.subarray(0, length);
  } finally {
    await file.close();
  }
}

function parsePackets(config, signatureFile) {
  const publicLines = lines(base64(config.plugins?.updater?.pubkey, 'updater public key'), 2, 'public key');
  const publicPacket = base64(publicLines[1], 'public key packet');
  requireCondition(publicPacket.length === 42, 'Public key packet must contain 42 bytes');
  requireCondition(['Ed', 'ED'].includes(publicPacket.subarray(0, 2).toString('ascii')), 'Unsupported public key algorithm');
  const signatureLines = lines(base64(ascii(signatureFile, 'signature wrapper').trim(), 'signature wrapper'), 4, 'signature');
  const signaturePacket = base64(signatureLines[1], 'signature packet');
  const globalSignature = base64(signatureLines[3], 'global signature');
  requireCondition(signaturePacket.length === 74, 'Signature packet must contain 74 bytes');
  requireCondition(globalSignature.length === 64, 'Global signature must contain 64 bytes');
  requireCondition(signaturePacket.subarray(0, 2).equals(Buffer.from('ED', 'ascii')), 'Only ED prehashed signatures are accepted');
  requireCondition(crypto.timingSafeEqual(publicPacket.subarray(2, 10), signaturePacket.subarray(2, 10)), 'Signature key ID does not match the configured public key');
  requireCondition(signatureLines[2].startsWith(TRUSTED_PREFIX), 'Missing trusted comment prefix');
  return {
    key: crypto.createPublicKey({ key: Buffer.concat([SPKI_PREFIX, publicPacket.subarray(10)]), format: 'der', type: 'spki' }),
    keyId: Buffer.from(publicPacket.subarray(2, 10)).reverse().toString('hex').toUpperCase(),
    signature: signaturePacket.subarray(10),
    trustedComment: Buffer.from(signatureLines[2].slice(TRUSTED_PREFIX.length), 'ascii'),
    globalSignature,
  };
}

async function hashInstaller(installer) {
  const file = await fs.open(installer, 'r');
  try {
    const before = await file.stat({ bigint: true });
    requireCondition(before.isFile() && before.size > 0n, 'Installer must be a nonempty regular file');
    const blake = crypto.createHash('blake2b512');
    const sha = crypto.createHash('sha256');
    let bytes = 0n;
    const stream = file.createReadStream({ autoClose: false, highWaterMark: 1024 * 1024, signal: AbortSignal.timeout(120000) });
    for await (const chunk of stream) {
      bytes += BigInt(chunk.length);
      blake.update(chunk);
      sha.update(chunk);
    }
    const after = await file.stat({ bigint: true });
    requireCondition(bytes === before.size && ['dev', 'ino', 'size', 'mtimeNs', 'ctimeNs'].every((name) => before[name] === after[name]), 'Installer changed during verification');
    return { digest: blake.digest(), sha256: sha.digest('hex') };
  } finally {
    await file.close();
  }
}

function verify(digest, packet) {
  const content = crypto.verify(null, digest, packet.key, packet.signature);
  const global = crypto.verify(null, Buffer.concat([packet.signature, packet.trustedComment]), packet.key, packet.globalSignature);
  return { content, global };
}

function proveTamperingIsRejected(digest, packet) {
  const changedDigest = Buffer.from(digest);
  changedDigest[0] ^= 1;
  const digestResult = verify(changedDigest, packet);
  requireCondition(!digestResult.content && digestResult.global, 'Altered digest was not rejected at the content signature');
  const changedSignature = Buffer.from(packet.signature);
  changedSignature[0] ^= 1;
  const signatureResult = verify(digest, { ...packet, signature: changedSignature });
  requireCondition(!signatureResult.content && !signatureResult.global, 'Altered signature was not rejected');
  const commentResult = verify(digest, { ...packet, trustedComment: Buffer.concat([packet.trustedComment, Buffer.from('X')]) });
  requireCondition(commentResult.content && !commentResult.global, 'Altered trusted comment was not rejected at the global signature');
}

async function main() {
  const options = argumentsFrom(process.argv.slice(2));
  const config = JSON.parse((await readSmall(options['--config'], 256 * 1024)).toString('utf8'));
  const packet = parsePackets(config, await readSmall(options['--signature'], 8192));
  // Public names normalize spaces and optionally add the offline suffix. Both
  // bundles are signed by Tauri before that export rename.
  const match = /^LanGame[ .]Server[ .]Manager_(\d+\.\d+\.\d+)_x64(?:-offline)?-setup\.exe$/.exec(path.basename(options['--installer']));
  requireCondition(match, 'Unexpected installer artifact name');
  requireCondition(match[1] === options['--version'], 'Artifact filename version differs from release');
  const hashes = await hashInstaller(options['--installer']);
  const result = verify(hashes.digest, packet);
  requireCondition(result.content, 'Installer content signature is invalid');
  requireCondition(result.global, 'Trusted comment global signature is invalid');
  validateTrustedComment(packet.trustedComment, options['--version']);
  console.log(JSON.stringify({ version: match[1], installerSHA256: hashes.sha256, keyId: packet.keyId, verified: true, signedVersionVerified: true }));
}

function validateTrustedComment(comment, version) {
  const fields = new Map();
  for (const field of comment.toString('ascii').split('\t')) {
    const index = field.indexOf(':');
    requireCondition(index > 0 && !fields.has(field.slice(0, index)), 'Duplicate or malformed trusted comment field');
    fields.set(field.slice(0, index), field.slice(index + 1));
  }
  requireCondition(fields.get('version') === version, 'Signed version differs from release or is missing');
  requireCondition(fields.get('file') === `LanGame Server Manager_${version}_x64-setup.exe`, 'Signed artifact filename differs from release');
}

module.exports = { parsePackets, verify, hashInstaller, validateTrustedComment, proveTamperingIsRejected };
if (require.main === module) main().catch((error) => {
  console.error(`Update signature verification failed: ${error instanceof Error ? error.message : String(error)}`);
  process.exitCode = 1;
});
