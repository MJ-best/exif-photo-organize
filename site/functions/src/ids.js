// Order ids and license keys.

import { randomBytes } from 'node:crypto';

// Crockford base32: no I, L, O, U — easy to read aloud and to type.
const ALPHABET = '0123456789ABCDEFGHJKMNPQRSTVWXYZ';

export function base32(bytes) {
  let bits = 0;
  let value = 0;
  let out = '';
  for (const b of bytes) {
    value = (value << 8) | b;
    bits += 8;
    while (bits >= 5) {
      out += ALPHABET[(value >>> (bits - 5)) & 31];
      bits -= 5;
    }
  }
  if (bits > 0) out += ALPHABET[(value << (5 - bits)) & 31];
  return out;
}

/** `PO20261105-7K2MXQ9D4TZB` — allowed by Toss Payments (letters, digits, `-`; 6–64 chars). */
export function newOrderId(now = new Date()) {
  const day = now.toISOString().slice(0, 10).replaceAll('-', '');
  return `PO${day}-${base32(randomBytes(8)).slice(0, 12)}`;
}

/** `PO-XXXXX-XXXXX-XXXXX-XXXXX`: 100 random bits. */
export function newLicenseKey() {
  const s = base32(randomBytes(13)).slice(0, 20);
  return `PO-${s.slice(0, 5)}-${s.slice(5, 10)}-${s.slice(10, 15)}-${s.slice(15, 20)}`;
}

/**
 * A key as typed or pasted (spaces, lower case, I/L for 1, O for 0, missing dashes) in its
 * canonical form, or null when it cannot be a key.
 */
export function normalizeLicenseKey(input) {
  if (typeof input !== 'string') return null;
  let s = input.toUpperCase().replace(/[\s-]/g, '');
  if (s.startsWith('PO')) s = s.slice(2);
  s = s.replace(/[IL]/g, '1').replace(/O/g, '0');
  if (!/^[0-9A-HJKMNP-TV-Z]{20}$/.test(s)) return null;
  return `PO-${s.slice(0, 5)}-${s.slice(5, 10)}-${s.slice(10, 15)}-${s.slice(15, 20)}`;
}

/** A machine id from the app: an opaque hash, used as a document id. */
export function validMachineId(id) {
  return typeof id === 'string' && /^[A-Za-z0-9_-]{8,128}$/.test(id);
}

export function validOrderId(id) {
  return typeof id === 'string' && /^[A-Za-z0-9_=-]{6,64}$/.test(id);
}
