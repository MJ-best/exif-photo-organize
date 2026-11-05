// Firebase Auth only (the site reads and writes data through /api).
// The configuration comes from Firebase Hosting (/__/firebase/init.json), so nothing is hard-coded;
// on localhost the Auth emulator is used.

import { initializeApp } from 'firebase/app';
import { connectAuthEmulator, getAuth } from 'firebase/auth';

const local = ['localhost', '127.0.0.1'].includes(location.hostname);
let ready;

export function getFirebaseAuth() {
  ready ??= (async () => {
    let config = null;
    try {
      const res = await fetch('/__/firebase/init.json');
      if (res.ok) config = await res.json();
    } catch {
      /* not served by Firebase Hosting */
    }
    if (!config?.apiKey) {
      if (!local) throw new Error('로그인 설정을 불러오지 못했습니다.');
      config = { apiKey: 'demo-key', authDomain: location.host, projectId: config?.projectId ?? 'demo-photo-organize' };
    }
    const auth = getAuth(initializeApp(config));
    auth.languageCode = 'ko';
    if (local) connectAuthEmulator(auth, `http://${location.hostname}:9099`, { disableWarnings: true });
    return auth;
  })();
  return ready;
}
