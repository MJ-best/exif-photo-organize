import assert from 'node:assert/strict';
import { test } from 'node:test';
import { PRODUCTS, product, publicCatalog } from '../src/catalog.js';
import { base32, newLicenseKey, newOrderId, normalizeLicenseKey, validMachineId, validOrderId } from '../src/ids.js';
import { newKeyPair, publicKeyOf, signToken, verifyToken } from '../src/tokens.js';

test('order ids are what Toss Payments accepts', () => {
  const seen = new Set();
  for (let i = 0; i < 1000; i++) {
    const id = newOrderId(new Date('2026-11-05T03:00:00Z'));
    assert.match(id, /^PO20261105-[0-9A-Z]{12}$/);
    assert.ok(validOrderId(id));
    seen.add(id);
  }
  assert.equal(seen.size, 1000);
  assert.ok(!validOrderId('abc'));
  assert.ok(!validOrderId('x'.repeat(65)));
  assert.ok(!validOrderId('PO2026/../x'));
});

test('license keys: format, randomness, forgiving input', () => {
  const k = newLicenseKey();
  assert.match(k, /^PO-[0-9A-HJKMNP-TV-Z]{5}(-[0-9A-HJKMNP-TV-Z]{5}){3}$/);
  assert.notEqual(newLicenseKey(), newLicenseKey());
  assert.equal(normalizeLicenseKey(k), k);
  // Typed in lower case, without dashes, with O for 0 and l for 1.
  const raw = k.slice(3).replaceAll('-', '').toLowerCase().replaceAll('0', 'o').replaceAll('1', 'l');
  assert.equal(normalizeLicenseKey(`  ${raw} `), k);
  assert.equal(normalizeLicenseKey(`po ${raw}`), k);
  assert.equal(normalizeLicenseKey('PO-1234'), null);
  assert.equal(normalizeLicenseKey('PO-UUUUU-UUUUU-UUUUU-UUUUU'), null); // U is not in the alphabet
  assert.equal(normalizeLicenseKey(null), null);
});

test('base32 packs bits', () => {
  assert.equal(base32(Buffer.from([0, 0, 0, 0, 0])), '00000000');
  assert.equal(base32(Buffer.from([0xff, 0xff, 0xff, 0xff, 0xff])), 'ZZZZZZZZ');
});

test('machine ids', () => {
  assert.ok(validMachineId('a1b2c3d4e5f6'));
  assert.ok(!validMachineId('short'));
  assert.ok(!validMachineId('../../etc/passwd'));
  assert.ok(!validMachineId(42));
});

test('activation tokens verify, and tampering is caught', () => {
  const { privateKeyPem, publicKeyPem, publicKeyRaw } = newKeyPair();
  assert.equal(publicKeyOf(privateKeyPem), publicKeyPem);
  assert.equal(Buffer.from(publicKeyRaw, 'base64url').length, 32);
  const claims = { v: 1, lid: 'L1', key: 'PO-AAAAA-BBBBB-CCCCC-DDDDD', mid: 'machine-01', upd: '2027-11-05' };
  const token = signToken(claims, privateKeyPem);
  assert.deepEqual(verifyToken(token, publicKeyPem), claims);

  const [body, sig] = token.split('.');
  const forged = Buffer.from(JSON.stringify({ ...claims, upd: '2099-01-01' })).toString('base64url');
  assert.equal(verifyToken(`${forged}.${sig}`, publicKeyPem), null);
  assert.equal(verifyToken(`${body}.${sig.slice(0, -2)}AA`, publicKeyPem), null);
  assert.equal(verifyToken(token, newKeyPair().publicKeyPem), null);
  assert.equal(verifyToken('nonsense', publicKeyPem), null);
});

test('catalog: integer KRW prices, the default product exists', () => {
  assert.ok(product('license'));
  assert.equal(product('nope'), null);
  for (const p of PRODUCTS) {
    assert.ok(Number.isInteger(p.price) && p.price > 0);
    assert.ok(p.orderName.length <= 100);
    assert.ok(p.maxDevices >= 1);
  }
  const c = publicCatalog({ provider: 'tosspayments', mode: 'test', clientKey: 'test_gck_x' });
  assert.equal(c.currency, 'KRW');
  assert.equal(c.payments.mode, 'test');
});
