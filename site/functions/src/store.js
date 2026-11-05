// Firestore data (only the server touches it — see firestore.rules):
//
//   orders/{orderId}                     uid, email, productId, amount, status, paymentKey, …
//   licenses/{licenseId}                 key, uid, email, productId, orderId, status, maxDevices, updatesUntil
//   licenses/{licenseId}/activations/{machineId}   machineName, os, appVersion, firstAt, lastAt

import { FieldValue, Timestamp, getFirestore } from 'firebase-admin/firestore';
import { newLicenseKey, newOrderId } from './ids.js';

const db = () => getFirestore();
const now = () => FieldValue.serverTimestamp();

export class AppError extends Error {
  constructor(status, code, message) {
    super(message);
    this.status = status;
    this.code = code;
  }
}

export async function createOrder({ uid, email, name, product, mode }) {
  const id = newOrderId();
  const order = {
    uid,
    email,
    name: name ?? null,
    productId: product.id,
    orderName: product.orderName,
    amount: product.price,
    currency: 'KRW',
    status: 'created',
    mode,
    createdAt: now(),
  };
  await db().collection('orders').doc(id).create(order);
  return { id, ...order };
}

export async function getOrder(id) {
  const snap = await db().collection('orders').doc(id).get();
  return snap.exists ? { id: snap.id, ...snap.data() } : null;
}

function addMonths(date, months) {
  const d = new Date(date);
  d.setUTCMonth(d.getUTCMonth() + months);
  return d.toISOString().slice(0, 10);
}

const ORDER_STATUS = {
  DONE: 'paid',
  WAITING_FOR_DEPOSIT: 'waiting',
  CANCELED: 'canceled',
  PARTIAL_CANCELED: 'partially_canceled',
  ABORTED: 'failed',
  EXPIRED: 'failed',
};

/**
 * Bring an order in line with its payment (as Toss Payments reports it): issue the license when the
 * payment is complete (once, however often this runs), revoke it when the payment is cancelled.
 */
export async function applyPayment(orderId, payment, product) {
  const orderRef = db().collection('orders').doc(orderId);
  return db().runTransaction(async (tx) => {
    const snap = await tx.get(orderRef);
    if (!snap.exists) throw new AppError(404, 'ORDER_NOT_FOUND', '주문을 찾을 수 없습니다.');
    const order = snap.data();
    const licRef = order.licenseId ? db().collection('licenses').doc(order.licenseId) : null;
    const lic = licRef ? await tx.get(licRef) : null;

    const status = ORDER_STATUS[payment.status] ?? order.status;
    const update = {
      status,
      paymentKey: payment.paymentKey ?? order.paymentKey ?? null,
      method: payment.method ?? order.method ?? null,
      receiptUrl: payment.receipt?.url ?? order.receiptUrl ?? null,
      tossStatus: payment.status ?? null,
      updatedAt: now(),
    };

    if (payment.status === 'DONE' && !licRef) {
      const approved = payment.approvedAt ? new Date(payment.approvedAt) : new Date();
      const ref = db().collection('licenses').doc();
      tx.set(ref, {
        key: newLicenseKey(),
        uid: order.uid,
        email: order.email,
        productId: order.productId,
        orderId,
        status: 'active',
        maxDevices: product?.maxDevices ?? 3,
        updatesUntil: addMonths(approved, product?.updatesMonths ?? 12),
        createdAt: now(),
      });
      tx.update(orderRef, { ...update, licenseId: ref.id, paidAt: Timestamp.fromDate(approved) });
      return { status, licenseId: ref.id, issued: true };
    }
    if (payment.status === 'CANCELED' && lic?.exists && lic.get('status') === 'active') {
      tx.update(licRef, { status: 'revoked', revokedAt: now(), revokedReason: 'payment_canceled' });
    }
    tx.update(orderRef, update);
    return { status, licenseId: order.licenseId ?? null, issued: false };
  });
}

/** Licenses and orders of one account, newest first. */
export async function listForUser(uid) {
  const [lics, orders] = await Promise.all([
    db().collection('licenses').where('uid', '==', uid).get(),
    db().collection('orders').where('uid', '==', uid).get(),
  ]);
  const licenses = await Promise.all(
    lics.docs.map(async (d) => {
      const acts = await d.ref.collection('activations').get();
      const x = d.data();
      return {
        id: d.id,
        key: x.key,
        productId: x.productId,
        status: x.status,
        maxDevices: x.maxDevices,
        updatesUntil: x.updatesUntil,
        createdAt: x.createdAt?.toDate?.().toISOString() ?? null,
        devices: acts.docs
          .map((a) => ({
            id: a.id,
            name: a.get('machineName') ?? null,
            os: a.get('os') ?? null,
            appVersion: a.get('appVersion') ?? null,
            lastAt: a.get('lastAt')?.toDate?.().toISOString() ?? null,
          }))
          .sort((a, b) => String(b.lastAt).localeCompare(String(a.lastAt))),
      };
    }),
  );
  const ts = (v) => v?.toDate?.().toISOString() ?? null;
  return {
    licenses: licenses.sort((a, b) => String(b.createdAt).localeCompare(String(a.createdAt))),
    orders: orders.docs
      .map((d) => {
        const x = d.data();
        return {
          id: d.id,
          orderName: x.orderName,
          amount: x.amount,
          currency: x.currency,
          status: x.status,
          method: x.method ?? null,
          receiptUrl: x.receiptUrl ?? null,
          mode: x.mode,
          createdAt: ts(x.createdAt),
          paidAt: ts(x.paidAt),
        };
      })
      // Orders that never reached the payment window are noise.
      .filter((o) => o.status !== 'created')
      .sort((a, b) => String(b.createdAt).localeCompare(String(a.createdAt))),
  };
}

async function licenseByKey(key) {
  const q = await db().collection('licenses').where('key', '==', key).limit(1).get();
  if (q.empty) throw new AppError(404, 'INVALID_KEY', '라이선스 키를 찾을 수 없습니다. 키를 다시 확인해 주세요.');
  return q.docs[0].ref;
}

/** Register a computer for a license (again: just refreshed), within the device limit. */
export async function activate({ key, machineId, machineName, os, appVersion }) {
  const licRef = await licenseByKey(key);
  return db().runTransaction(async (tx) => {
    const lic = await tx.get(licRef);
    const data = lic.data();
    if (data.status !== 'active') throw new AppError(403, 'REVOKED', '사용할 수 없는 라이선스입니다 (환불 · 해지).');
    const actRef = licRef.collection('activations').doc(machineId);
    const [act, all] = await Promise.all([tx.get(actRef), tx.get(licRef.collection('activations'))]);
    if (!act.exists && all.size >= data.maxDevices) {
      throw new AppError(
        409,
        'DEVICE_LIMIT',
        `이 라이선스는 컴퓨터 ${data.maxDevices}대까지 쓸 수 있습니다. 웹사이트의 내 라이선스에서 쓰지 않는 컴퓨터를 빼 주세요.`,
      );
    }
    // What the app sent this time; a field it left out keeps its earlier value.
    const info = Object.fromEntries(Object.entries({ machineName, os, appVersion }).filter(([, v]) => v));
    tx.set(actRef, { ...info, lastAt: now(), ...(act.exists ? {} : { firstAt: now() }) }, { merge: true });
    return { license: { id: licRef.id, ...data }, devices: act.exists ? all.size : all.size + 1 };
  });
}

/** Free a computer's seat: from the app (by key) or from the account page (by owner). */
export async function deactivate({ key, licenseId, uid, machineId }) {
  const licRef = key ? await licenseByKey(key) : db().collection('licenses').doc(licenseId);
  if (uid) {
    const lic = await licRef.get();
    if (!lic.exists || lic.get('uid') !== uid) throw new AppError(404, 'NOT_FOUND', '라이선스를 찾을 수 없습니다.');
  }
  await licRef.collection('activations').doc(machineId).delete();
}
