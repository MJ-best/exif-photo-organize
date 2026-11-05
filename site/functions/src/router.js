// The site's API, served at /api/* through Firebase Hosting (one function, one cold start).
//
//   GET  /api/catalog                      prices, features, how to pay (public, cached)
//   POST /api/orders                       start an order (signed in)
//   GET  /api/toss/success                 Toss Payments successUrl: confirm, issue the license
//   POST /api/toss/webhook                 Toss Payments webhook: refunds, deposits
//   GET  /api/me                           my licenses and orders (signed in)
//   POST /api/me/licenses/:id/devices/:machine/remove
//   POST /api/licenses/activate            from the app: register a computer, get a signed token
//   POST /api/licenses/deactivate          from the app: free the computer's seat
//   GET  /api/licenses/public-key          the key the app checks tokens with

import { getAuth } from 'firebase-admin/auth';
import { logger } from 'firebase-functions';
import { TAX_FREE, product, publicCatalog } from './catalog.js';
import { TOSS_CLIENT_KEY, isEmulator, paymentMode, signingKey, tossSecretKey } from './config.js';
import { normalizeLicenseKey, validMachineId, validOrderId } from './ids.js';
import {
  AppError,
  activate,
  applyPayment,
  createOrder,
  deactivate,
  getOrder,
  listForUser,
} from './store.js';
import { newKeyPair, publicKeyOf, signToken } from './tokens.js';
import { TossError, confirmPayment, getPayment, mockPayment } from './toss.js';

const text = (v, max) => (typeof v === 'string' ? v.trim().slice(0, max) : null);

async function requireUser(req) {
  const m = /^Bearer (.+)$/.exec(req.get('Authorization') ?? '');
  if (!m) throw new AppError(401, 'SIGN_IN', '로그인이 필요합니다.');
  try {
    return await getAuth().verifyIdToken(m[1]);
  } catch {
    throw new AppError(401, 'SIGN_IN', '로그인이 만료되었습니다. 다시 로그인해 주세요.');
  }
}

// Activation tokens are signed with LICENSE_SIGNING_KEY; the emulator makes up a key per run.
let devKey = null;
function signingPem() {
  const pem = signingKey();
  if (pem) return pem;
  if (!isEmulator()) throw new AppError(503, 'NOT_CONFIGURED', '라이선스 서명 키가 설정되지 않았습니다.');
  devKey ??= newKeyPair();
  return devKey.privateKeyPem;
}

function paymentsInfo() {
  const mode = paymentMode();
  return { provider: 'tosspayments', mode, clientKey: mode === 'mock' ? null : TOSS_CLIENT_KEY.value() };
}

// ---- routes ----

function catalog(req, res) {
  // Cached by the Hosting CDN: prices change rarely.
  res.set('Cache-Control', 'public, max-age=300, s-maxage=600');
  res.json(publicCatalog(paymentsInfo()));
}

async function startOrder(req, res) {
  const user = await requireUser(req);
  const p = product(req.body?.productId ?? 'license');
  if (!p) throw new AppError(400, 'NO_PRODUCT', '없는 상품입니다.');
  if (!user.email) throw new AppError(400, 'NO_EMAIL', '이메일이 있는 계정으로 로그인해 주세요.');
  const payments = paymentsInfo();
  if (payments.mode !== 'mock' && !tossSecretKey()) {
    throw new AppError(503, 'NOT_CONFIGURED', '결제 설정이 아직 끝나지 않았습니다. 잠시 뒤 다시 시도해 주세요.');
  }
  const order = await createOrder({ uid: user.uid, email: user.email, name: user.name, product: p, mode: payments.mode });
  res.status(201).json({
    orderId: order.id,
    orderName: order.orderName,
    amount: order.amount,
    currency: order.currency,
    taxFreeAmount: TAX_FREE ? order.amount : 0,
    customerEmail: user.email,
    customerName: user.name ?? null,
    payments,
  });
}

async function tossSuccess(req, res) {
  const { paymentKey, orderId } = req.query;
  const amount = Number(req.query.amount);
  const back = (code, message) =>
    res.redirect(303, `/checkout?error=${encodeURIComponent(code)}&message=${encodeURIComponent(message)}`);

  if (typeof paymentKey !== 'string' || !paymentKey || !validOrderId(orderId) || !Number.isInteger(amount)) {
    return back('BAD_REQUEST', '결제 정보가 올바르지 않습니다.');
  }
  const order = await getOrder(orderId);
  if (!order) return back('ORDER_NOT_FOUND', '주문을 찾을 수 없습니다.');
  if (order.status === 'paid') return res.redirect(303, `/account?paid=${orderId}`);
  // The browser can be tampered with: the amount must be the order's.
  if (amount !== order.amount) return back('AMOUNT_MISMATCH', '결제 금액이 주문 금액과 다릅니다.');

  let payment;
  if (order.mode === 'mock') {
    if (!isEmulator()) return back('BAD_REQUEST', '테스트 주문은 실제 사이트에서 결제할 수 없습니다.');
    payment = mockPayment({ paymentKey, orderId, amount: order.amount });
  } else {
    try {
      payment = await confirmPayment({ paymentKey, orderId, amount: order.amount }, tossSecretKey());
    } catch (e) {
      if (e instanceof TossError && e.code === 'ALREADY_PROCESSED_PAYMENT') {
        payment = await getPayment(paymentKey, tossSecretKey());
      } else {
        logger.warn('confirm failed', { orderId, code: e.code, message: e.message });
        return back(e.code ?? 'CONFIRM_FAILED', e.message ?? '결제를 승인하지 못했습니다.');
      }
    }
  }
  if (payment.orderId !== orderId || payment.totalAmount !== order.amount) {
    logger.error('payment does not match the order', { orderId, payment: payment.orderId, total: payment.totalAmount });
    return back('MISMATCH', '결제 정보가 주문과 맞지 않습니다. 고객센터로 문의해 주세요.');
  }
  const r = await applyPayment(orderId, payment, product(order.productId));
  logger.info('payment applied', { orderId, status: r.status, issued: r.issued });
  return res.redirect(303, r.status === 'paid' ? `/account?paid=${orderId}` : `/account?order=${orderId}`);
}

async function tossWebhook(req, res) {
  const body = req.body ?? {};
  const orderId = body.data?.orderId ?? body.orderId ?? null;
  let order = validOrderId(orderId) ? await getOrder(orderId) : null;
  const paymentKey = body.data?.paymentKey ?? order?.paymentKey ?? null;
  if (!paymentKey || order?.mode === 'mock') return res.json({ ok: true, ignored: true });
  // Never trust the body: ask Toss Payments for the payment as it is now.
  const payment = await getPayment(paymentKey, tossSecretKey());
  order ??= await getOrder(payment.orderId);
  if (!order) return res.json({ ok: true, ignored: true });
  const r = await applyPayment(order.id, payment, product(order.productId));
  logger.info('webhook applied', { orderId: order.id, status: r.status, eventType: body.eventType ?? null });
  res.json({ ok: true });
}

async function me(req, res) {
  const user = await requireUser(req);
  res.json({ email: user.email ?? null, ...(await listForUser(user.uid)) });
}

async function removeDevice(req, res, licenseId, machineId) {
  const user = await requireUser(req);
  await deactivate({ licenseId, uid: user.uid, machineId });
  res.json({ ok: true });
}

async function activateRoute(req, res) {
  const b = req.body ?? {};
  const key = normalizeLicenseKey(b.key);
  if (!key) throw new AppError(400, 'INVALID_KEY', '라이선스 키 형식이 아닙니다 (PO-XXXXX-XXXXX-XXXXX-XXXXX).');
  if (!validMachineId(b.machineId)) throw new AppError(400, 'BAD_MACHINE', '기기 정보가 올바르지 않습니다.');
  const { license, devices } = await activate({
    key,
    machineId: b.machineId,
    machineName: text(b.machineName, 80),
    os: text(b.os, 40),
    appVersion: text(b.appVersion, 20),
  });
  const claims = {
    v: 1,
    lid: license.id,
    key,
    product: license.productId,
    email: license.email,
    mid: b.machineId,
    max: license.maxDevices,
    upd: license.updatesUntil,
    iat: Math.floor(Date.now() / 1000),
  };
  res.json({
    token: signToken(claims, signingPem()),
    license: { product: license.productId, email: license.email, updatesUntil: license.updatesUntil, maxDevices: license.maxDevices, devices },
  });
}

async function deactivateRoute(req, res) {
  const b = req.body ?? {};
  const key = normalizeLicenseKey(b.key);
  if (!key || !validMachineId(b.machineId)) throw new AppError(400, 'BAD_REQUEST', '라이선스 키와 기기 정보가 필요합니다.');
  await deactivate({ key, machineId: b.machineId });
  res.json({ ok: true });
}

function publicKey(req, res) {
  res.set('Cache-Control', 'public, max-age=3600');
  res.json({ alg: 'Ed25519', pem: publicKeyOf(signingPem()) });
}

export async function handle(req, res) {
  res.set('Cache-Control', 'private, no-store');
  try {
    // Through Hosting the path keeps its /api prefix; called directly it does not.
    const path = req.path.replace(/^\/api(?=\/|$)/, '') || '/';
    const route = `${req.method} ${path}`;
    let m;
    if (route === 'GET /catalog') return catalog(req, res);
    if (route === 'POST /orders') return await startOrder(req, res);
    if (route === 'GET /toss/success') return await tossSuccess(req, res);
    if (route === 'POST /toss/webhook') return await tossWebhook(req, res);
    if (route === 'GET /me') return await me(req, res);
    if (req.method === 'POST' && (m = /^\/me\/licenses\/([A-Za-z0-9]{1,40})\/devices\/([A-Za-z0-9_-]{8,128})\/remove$/.exec(path))) {
      return await removeDevice(req, res, m[1], m[2]);
    }
    if (route === 'POST /licenses/activate') return await activateRoute(req, res);
    if (route === 'POST /licenses/deactivate') return await deactivateRoute(req, res);
    if (route === 'GET /licenses/public-key') return publicKey(req, res);
    throw new AppError(404, 'NOT_FOUND', '없는 주소입니다.');
  } catch (e) {
    const status = e instanceof AppError || e instanceof TossError ? e.status || 500 : 500;
    if (status >= 500) logger.error('api error', { path: req.path, message: e.message, stack: e.stack });
    res.set('Cache-Control', 'private, no-store');
    res.status(status).json({
      error: { code: e.code ?? 'INTERNAL', message: status >= 500 && !(e instanceof AppError) ? '잠시 문제가 생겼습니다. 다시 시도해 주세요.' : e.message },
    });
  }
}
