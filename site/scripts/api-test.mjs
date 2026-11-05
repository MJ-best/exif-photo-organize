// The sales API end to end on the Firebase emulators (mock payments):
//   npm run test:api
// (runs under `firebase emulators:exec`, which starts and stops the emulators)

import assert from 'node:assert/strict';
import { createPublicKey, verify } from 'node:crypto';

const HOSTING = 'http://127.0.0.1:5050';
const AUTH = 'http://127.0.0.1:9099';

async function call(path, { method = 'GET', body, token } = {}) {
  const headers = { Accept: 'application/json' };
  if (body !== undefined) headers['Content-Type'] = 'application/json';
  if (token) headers.Authorization = `Bearer ${token}`;
  const res = await fetch(HOSTING + path, {
    method,
    headers,
    body: body === undefined ? undefined : JSON.stringify(body),
    redirect: 'manual',
  });
  const text = await res.text();
  let json = null;
  try {
    json = JSON.parse(text);
  } catch {
    /* a redirect */
  }
  return { status: res.status, location: res.headers.get('location'), json };
}

async function signUp(email) {
  const res = await fetch(`${AUTH}/identitytoolkit.googleapis.com/v1/accounts:signUp?key=fake-api-key`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ email, password: 'test-password-1', returnSecureToken: true }),
  });
  const data = await res.json();
  assert.ok(data.idToken, `sign up: ${JSON.stringify(data)}`);
  return data.idToken;
}

// The functions emulator may still be loading.
for (let i = 0; ; i++) {
  const r = await call('/api/catalog');
  if (r.status === 200) break;
  if (i > 30) throw new Error(`catalog: ${r.status}`);
  await new Promise((r) => setTimeout(r, 1000));
}

let step = 0;
const ok = (what) => console.log(`✔ ${++step}. ${what}`);

const catalog = (await call('/api/catalog')).json;
assert.equal(catalog.payments.mode, 'mock');
const product = catalog.products[0];
ok(`catalog: ${product.name} ${product.price}원`);

assert.equal((await call('/api/orders', { method: 'POST', body: {} })).status, 401);
ok('orders need sign-in');

const token = await signUp(`buyer-${Date.now()}@example.com`);
const order = (await call('/api/orders', { method: 'POST', body: { productId: product.id }, token })).json;
assert.equal(order.amount, product.price);
assert.match(order.orderId, /^PO\d{8}-[0-9A-Z]{12}$/);
ok(`order ${order.orderId}`);

const success = (amount) =>
  call(`/api/toss/success?paymentKey=mock_${order.orderId}&orderId=${order.orderId}&amount=${amount}`);
let r = await success(1);
assert.equal(r.status, 303);
assert.match(r.location, /^\/checkout\?error=AMOUNT_MISMATCH/);
ok('a tampered amount is refused');

r = await success(order.amount);
assert.equal(r.status, 303);
assert.equal(r.location, `/account?paid=${order.orderId}`);
r = await success(order.amount);
assert.equal(r.location, `/account?paid=${order.orderId}`);
ok('payment confirmed (and confirming again changes nothing)');

let me = (await call('/api/me', { token })).json;
assert.equal(me.licenses.length, 1);
assert.equal(me.orders.length, 1);
assert.equal(me.orders[0].status, 'paid');
const license = me.licenses[0];
assert.match(license.key, /^PO-[0-9A-Z]{5}(-[0-9A-Z]{5}){3}$/);
assert.equal(license.maxDevices, product.maxDevices);
ok(`one license issued: ${license.key}`);

const activate = (machineId, extra = {}) =>
  call('/api/licenses/activate', { method: 'POST', body: { key: license.key, machineId, ...extra } });
r = await activate('machine-0001', { machineName: '작업실 맥', os: 'macOS', appVersion: '0.4.0' });
assert.equal(r.status, 200);
const pem = (await call('/api/licenses/public-key')).json.pem;
const [body, sig] = r.json.token.split('.');
assert.ok(verify(null, Buffer.from(body), createPublicKey(pem), Buffer.from(sig, 'base64url')));
const claims = JSON.parse(Buffer.from(body, 'base64url').toString());
assert.equal(claims.mid, 'machine-0001');
assert.equal(claims.key, license.key);
ok('activation token is signed and verifiable');

r = await activate('machine-0001');
assert.equal(r.json.license.devices, 1);
me = (await call('/api/me', { token })).json;
assert.equal(me.licenses[0].devices[0].os, 'macOS');
ok('activating the same computer again keeps one seat and its details');

for (let i = 2; i <= product.maxDevices; i++) assert.equal((await activate(`machine-000${i}`)).status, 200);
r = await activate('machine-0099');
assert.equal(r.status, 409);
assert.equal(r.json.error.code, 'DEVICE_LIMIT');
ok(`device limit: ${product.maxDevices}`);

assert.equal(
  (await call(`/api/me/licenses/${license.id}/devices/machine-0002/remove`, { method: 'POST', token })).status,
  200,
);
assert.equal((await activate('machine-0099')).status, 200);
ok('freeing a seat from the account page lets another computer in');

const other = await signUp(`other-${Date.now()}@example.com`);
r = await call(`/api/me/licenses/${license.id}/devices/machine-0003/remove`, { method: 'POST', token: other });
assert.equal(r.status, 404);
ok("someone else cannot touch the license's computers");

r = await call('/api/licenses/activate', { method: 'POST', body: { key: 'PO-00000-00000-00000-00000', machineId: 'machine-0001' } });
assert.equal(r.status, 404);
r = await call('/api/licenses/activate', { method: 'POST', body: { key: 'hello', machineId: 'machine-0001' } });
assert.equal(r.status, 400);
ok('unknown and malformed keys are refused');

console.log('\n모든 API 테스트 통과');
