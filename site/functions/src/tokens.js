// Activation tokens the app keeps and checks offline: `<payload>.<signature>`, both base64url;
// the signature is Ed25519 over the ASCII bytes of `<payload>`. The app embeds the public key
// (npm run keygen prints it), so a token cannot be forged, and no network is needed to start.

import { createPrivateKey, createPublicKey, generateKeyPairSync, sign, verify } from 'node:crypto';

const b64url = (buf) => Buffer.from(buf).toString('base64url');

export function signToken(payload, privateKeyPem) {
  const body = b64url(JSON.stringify(payload));
  const sig = sign(null, Buffer.from(body), createPrivateKey(privateKeyPem));
  return `${body}.${b64url(sig)}`;
}

/** The payload if the signature is valid, otherwise null. */
export function verifyToken(token, publicKeyPem) {
  const [body, sig] = String(token).split('.');
  if (!body || !sig) return null;
  const ok = verify(null, Buffer.from(body), createPublicKey(publicKeyPem), Buffer.from(sig, 'base64url'));
  if (!ok) return null;
  try {
    return JSON.parse(Buffer.from(body, 'base64url').toString('utf8'));
  } catch {
    return null;
  }
}

export function newKeyPair() {
  const { privateKey, publicKey } = generateKeyPairSync('ed25519');
  return {
    privateKeyPem: privateKey.export({ type: 'pkcs8', format: 'pem' }).toString(),
    publicKeyPem: publicKey.export({ type: 'spki', format: 'pem' }).toString(),
    // The raw 32-byte public key, for ed25519 libraries that take bytes (e.g. ed25519-dalek).
    publicKeyRaw: b64url(publicKey.export({ format: 'der', type: 'spki' }).subarray(-32)),
  };
}

export function publicKeyOf(privateKeyPem) {
  return createPublicKey(createPrivateKey(privateKeyPem)).export({ type: 'spki', format: 'pem' }).toString();
}
