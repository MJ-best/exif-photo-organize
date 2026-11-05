import { initializeApp } from 'firebase-admin/app';
import { onRequest } from 'firebase-functions/v2/https';
import { LICENSE_SIGNING_KEY, REGION, TOSS_SECRET_KEY } from './src/config.js';
import { handle } from './src/router.js';

initializeApp();

export const api = onRequest(
  {
    region: REGION,
    secrets: [TOSS_SECRET_KEY, LICENSE_SIGNING_KEY],
    invoker: 'public',
    memory: '256MiB',
    timeoutSeconds: 30,
    maxInstances: 10,
  },
  handle,
);
