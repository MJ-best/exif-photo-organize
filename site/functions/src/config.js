// Deployment settings (Firebase params). Secrets live in Cloud Secret Manager:
//   firebase functions:secrets:set TOSS_SECRET_KEY
//   firebase functions:secrets:set LICENSE_SIGNING_KEY   (npm run keygen)
// The client key is public and goes in functions/.env (TOSS_CLIENT_KEY=...).

import { defineSecret, defineString } from 'firebase-functions/params';

export const REGION = 'asia-northeast3'; // Seoul

export const TOSS_SECRET_KEY = defineSecret('TOSS_SECRET_KEY');
export const LICENSE_SIGNING_KEY = defineSecret('LICENSE_SIGNING_KEY');
export const TOSS_CLIENT_KEY = defineString('TOSS_CLIENT_KEY', {
  // The test key from Toss Payments' public documentation: try the flow before signing a contract.
  default: 'test_gck_docs_Ovk5rk1EwkEbP0W43n07xlzm',
  description: '토스페이먼츠 결제위젯 클라이언트 키 (공개 키, test_gck_… 또는 live_gck_…)',
});

export const isEmulator = () => process.env.FUNCTIONS_EMULATOR === 'true';

function secretValue(param) {
  try {
    return param.value() || '';
  } catch {
    return '';
  }
}

export const tossSecretKey = () => secretValue(TOSS_SECRET_KEY);
export const signingKey = () => secretValue(LICENSE_SIGNING_KEY);

/**
 * How payments are taken:
 * - `mock`: only in the local emulator without a Toss secret key — a test button stands in for
 *   the payment window, nothing is charged.
 * - `test`: Toss Payments test keys — the real payment window, no money moves.
 * - `live`: real payments.
 */
export function paymentMode() {
  if (isEmulator() && !tossSecretKey()) return 'mock';
  return TOSS_CLIENT_KEY.value().startsWith('live_') ? 'live' : 'test';
}
