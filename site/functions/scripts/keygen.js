// Make the key pair for activation tokens.
//   npm run keygen
// Store the private key as the LICENSE_SIGNING_KEY secret; put the public key in the app.

import { newKeyPair } from '../src/tokens.js';

const k = newKeyPair();
console.log('# 1) 비밀 키 → firebase functions:secrets:set LICENSE_SIGNING_KEY  (아래 블록 전체를 붙여 넣기)\n');
console.log(k.privateKeyPem);
console.log('# 2) 공개 키 → 앱에 넣기 (PEM 또는 32바이트 base64url)\n');
console.log(k.publicKeyPem);
console.log(`raw (base64url, 32 bytes): ${k.publicKeyRaw}\n`);
console.log('비밀 키는 저장소에 올리거나 다른 사람에게 보여 주지 마세요.');
