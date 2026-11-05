// Toss Payments REST API (https://docs.tosspayments.com/reference): confirm a payment after the
// buyer approved it in the payment window, and look one up (webhooks are re-checked this way,
// never trusted as sent).

const API = 'https://api.tosspayments.com/v1';

export class TossError extends Error {
  constructor(code, message, status) {
    super(message);
    this.code = code;
    this.status = status;
  }
}

function headers(secretKey, extra = {}) {
  return {
    Authorization: 'Basic ' + Buffer.from(`${secretKey}:`).toString('base64'),
    'Content-Type': 'application/json',
    ...extra,
  };
}

async function call(url, init) {
  let res;
  try {
    res = await fetch(url, { ...init, signal: AbortSignal.timeout(20_000) });
  } catch (e) {
    throw new TossError('NETWORK', `토스페이먼츠에 연결하지 못했습니다: ${e.message}`, 502);
  }
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new TossError(data.code || 'TOSS_ERROR', data.message || `HTTP ${res.status}`, res.status);
  return data;
}

export function confirmPayment({ paymentKey, orderId, amount }, secretKey) {
  return call(`${API}/payments/confirm`, {
    method: 'POST',
    // Retrying the same confirmation is safe.
    headers: headers(secretKey, { 'Idempotency-Key': `confirm-${orderId}` }),
    body: JSON.stringify({ paymentKey, orderId, amount }),
  });
}

export function getPayment(paymentKey, secretKey) {
  return call(`${API}/payments/${encodeURIComponent(paymentKey)}`, { method: 'GET', headers: headers(secretKey) });
}

/** A stand-in for the local emulator (payment mode `mock`): approves anything. */
export function mockPayment({ paymentKey, orderId, amount }) {
  return {
    paymentKey,
    orderId,
    status: 'DONE',
    totalAmount: amount,
    method: '테스트 결제',
    approvedAt: new Date().toISOString(),
    receipt: null,
  };
}
