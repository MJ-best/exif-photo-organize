# LOUPE 판매 사이트 (Firebase)

앱을 소개하고, 라이선스를 팔고, 산 사람이 키와 컴퓨터를 관리하는 웹사이트입니다. 앱과 같은 **크림 · 코코아** 색, 마스코트, 사진 타일로 만들었습니다.

| 경로 | 내용 |
|---|---|
| `/` | 소개 — 셀렉 과정 데모, 사용 흐름, 셀렉 도우미, 취향 학습, 기기 안 처리, 속도 · 호환, 가격, 자주 묻는 질문 |
| `/checkout` | 구매 — 로그인(Google · 이메일 링크) → 토스페이먼츠 결제위젯 → 결제 |
| `/account` | 내 라이선스 — 키 복사, 등록한 컴퓨터 빼기, 주문 내역 · 영수증, 내려받기 |
| `/terms` `/privacy` `/refund` | 이용약관 · 개인정보처리방침 · 환불 규정 (초안) |

## 구성

```
site/
├── web/                 페이지 · 스타일 · 스크립트 원본 (partials/ 머리말 · 꼬리말)
├── public/              npm run build 결과 (Firebase Hosting 이 내보냄, 저장소에 넣지 않음)
├── functions/           Cloud Functions `api` (서울 리전)
│   └── src/
│       ├── catalog.js   가격 · 구성 (여기가 가격의 유일한 기준)
│       ├── router.js    /api/* 주소
│       ├── store.js     Firestore — 주문 · 라이선스 · 등록한 컴퓨터
│       ├── toss.js      토스페이먼츠 결제 승인 · 조회
│       └── tokens.js    앱 활성화 토큰 (Ed25519 서명)
├── firestore.rules      브라우저에서 Firestore 직접 접근 차단 (모든 데이터는 /api 를 거침)
├── site.config.json     사업자 정보 · 다운로드 링크 · 환불 기간
└── scripts/             build.mjs (빌드), api-test.mjs (에뮬레이터 API 테스트)
```

- 글꼴(Pretendard, 고운바탕 — SIL OFL)과 Firebase SDK 까지 사이트 안에 담아, 결제창을 열기 전에는 다른 사이트로 요청이 가지 않습니다.
- 앱 스크린샷(`docs/images`)과 아이콘(`crates/app/assets`)은 빌드할 때 저장소에서 가져옵니다.
- 결제 금액은 서버(`catalog.js`)가 정하고, 결제 승인 때 다시 확인합니다. 브라우저에서 금액을 바꿔도 통하지 않습니다.

## 로컬에서 해 보기

```bash
cd site
npm install && npm --prefix functions install
npm run dev        # 빌드 + 에뮬레이터 → http://127.0.0.1:5050
npm test           # 함수 단위 테스트 + 에뮬레이터 API 테스트
```

에뮬레이터에서는 토스 결제창 대신 **테스트 결제** 버튼으로 구매 → 라이선스 발급까지 끝까지 확인합니다 (돈은 나가지 않음). 이메일 링크 로그인은 에뮬레이터가 메일 대신 링크를 터미널과 http://127.0.0.1:4000/auth 에 보여 줍니다. Java 11 이상이 필요합니다 (Firestore 에뮬레이터).

## 처음 배포하기

1. **Firebase 프로젝트** 를 만들고 **Blaze 요금제**로 바꿉니다 (Cloud Functions · Secret Manager 에 필요. 작은 사이트는 무료 사용량 안에서 거의 끝납니다). `.firebaserc` 의 `photo-organize` 를 프로젝트 ID 로 바꿉니다.
2. 프로젝트 설정 → **웹 앱 추가** (Hosting 이 그 설정을 `/__/firebase/init.json` 으로 내보내므로 코드에 키를 넣지 않습니다).
3. **Authentication → 로그인 방법**: *Google*, *이메일/비밀번호 → 이메일 링크(비밀번호 없는 로그인)* 를 켭니다. 맞춤 도메인을 쓰면 *승인된 도메인* 에 추가합니다.
4. **Firestore** 데이터베이스를 만듭니다 (위치 `asia-northeast3` 서울, 프로덕션 모드).
5. **토스페이먼츠** 개발자센터에서 *결제위젯* 키를 받습니다 (가입하면 바로 테스트 키).
   ```bash
   echo 'TOSS_CLIENT_KEY=test_gck_…' > functions/.env      # 공개 키
   npx firebase functions:secrets:set TOSS_SECRET_KEY        # test_gsk_… (비밀 키)
   ```
6. **라이선스 서명 키** 를 만듭니다. 비밀 키는 Secret Manager 에, 공개 키는 나중에 앱에 넣습니다.
   ```bash
   npm --prefix functions run keygen
   npx firebase functions:secrets:set LICENSE_SIGNING_KEY    # 출력된 PRIVATE KEY 블록 전체
   ```
7. 배포합니다 (빌드는 자동): `npx firebase deploy`
8. 토스페이먼츠 개발자센터 → **웹훅** 에 `https://<도메인>/api/toss/webhook` 을 등록합니다 (결제 상태 변경, 가상계좌 입금). 상점관리자에서 결제를 취소하면 라이선스가 자동으로 해지됩니다.
9. 필요하면 Hosting → **맞춤 도메인** 을 연결합니다.

## 실제로 팔기 전에

- [ ] **통신판매업 신고** → `site.config.json` 의 `mailOrderNumber`
- [ ] **문의 이메일 · 전화** → `email`, `phone` (꼬리말 · 환불 규정 · 개인정보처리방침에 쓰임)
- [ ] 약관 · 개인정보처리방침 **시행일** → `effectiveDate`
- [ ] 이용약관 · 개인정보처리방침 · 환불 규정은 **초안**입니다. 내용을 검토하세요 (가격 · 기기 수 · 업데이트 기간을 바꾸면 `web/terms.html` 제5조도 함께).
- [ ] **세금**: 사업자등록증에는 *부가가치세 면세사업자 (업태 정보통신업 · 종목 출판업)* 로 되어 있습니다. 소프트웨어 라이선스 판매가 면세로 처리되는지, 업종 추가나 과세사업자 전환이 필요한지 세무사에게 확인하세요. 지금은 결제 금액 전체를 면세로 보냅니다 — 과세로 바뀌면 `functions/src/catalog.js` 의 `TAX_FREE` 를 `false` 로.
- [ ] **토스페이먼츠 계약** (사업자등록증 · 통신판매업 신고증 · 정산 계좌, 사이트 심사) 후 라이브 키로 바꿉니다: `TOSS_CLIENT_KEY=live_gck_…`, `TOSS_SECRET_KEY=live_gsk_…`
- [ ] **앱과 연결** — 아래 [앱과 연결하기](#앱과-연결하기) 의 세 단계 (앱에 사이트 주소 · 공개 키 넣고 새 버전 릴리스 → `appActivation` 켜기).

`npm run build` 는 비어 있는 사업자 정보를 경고하고, 페이지에는 노란 **［입력 필요］** 로 표시합니다.

## 가격 · 구성 바꾸기

`functions/src/catalog.js` 의 `price` · `maxDevices` · `updatesMonths` · `features` 를 고치고 `npx firebase deploy --only functions`. 페이지는 `/api/catalog` 를 읽어 보여 주므로 HTML 의 숫자는 처음 그려질 때의 기본값입니다.

## 앱과 연결하기

앱의 **설정 → 라이선스** 에서 키를 넣으면 이 컴퓨터가 등록되고, 서버가 서명한 토큰을 저장해 **시작할 때마다 인터넷 없이** 확인합니다 (`crates/app/src/license.rs`). 일주일에 한 번쯤 서버에 다시 확인해, 환불 · 해지된 라이선스는 그 컴퓨터에서 빠집니다. 연결이 안 되면 그대로 씁니다.

1. 사이트 주소와 서명 공개 키를 `crates/app/assets/license.json` 에 넣습니다 (keygen 이 출력한 PEM 또는 `raw` 줄).
   ```json
   { "server": "https://<사이트 도메인>", "publicKey": "<keygen 의 raw 공개 키>" }
   ```
   두 값이 비어 있으면 앱의 라이선스 칸은 보이지 않습니다 (지금 상태).
2. 앱 새 버전을 릴리스합니다.
3. `site.config.json` 의 `appActivation` 을 `true` 로 바꾸고 사이트를 다시 배포합니다 — ‘내 라이선스’ 에 앱 등록 방법이 나옵니다.

개발할 때는 디버그 빌드에 환경 변수로 로컬 에뮬레이터를 연결할 수 있습니다 (릴리스 빌드는 무시):

```bash
PHOTO_ORGANIZE_LICENSE_SERVER=http://127.0.0.1:5050 \
PHOTO_ORGANIZE_LICENSE_PUBLIC_KEY="$(curl -s http://127.0.0.1:5050/api/licenses/public-key | jq -r .pem)" \
cargo run --bin photo-organize
```

### API

```
POST /api/licenses/activate   { key, machineId, machineName?, os?, appVersion? }
  200 { token, license: { product, email, updatesUntil, maxDevices, devices } }
  400 INVALID_KEY | BAD_MACHINE   403 REVOKED   404 INVALID_KEY   409 DEVICE_LIMIT
POST /api/licenses/deactivate { key, machineId }          → 200 { ok }
GET  /api/licenses/public-key                              → { alg: "Ed25519", pem }
```

- `machineId`: 컴퓨터마다 고정된 값의 해시 (영문 · 숫자 · `-` `_`, 8–128자). 같은 컴퓨터는 몇 번 등록해도 한 대로 셉니다.
- `token` = `base64url(JSON)` + `.` + `base64url(서명)`. 서명은 첫 부분의 ASCII 바이트에 대한 Ed25519 서명이고, keygen 이 출력한 공개 키(또는 32바이트 raw 키, 예: Rust `ed25519-dalek`)로 **인터넷 없이** 확인합니다.
- 토큰 내용: `v` `lid` `key` `product` `email` `mid`(machineId) `max` `upd`(새 버전 무료 기한, `YYYY-MM-DD`) `iat`.
- 키는 대소문자 · 하이픈 · 공백 · `O`/`0` · `I`/`1` 을 가려 받습니다.

## 데이터 (Firestore)

| 컬렉션 | 내용 |
|---|---|
| `orders/{orderId}` | 주문자 · 상품 · 금액 · 상태 (`created` → `paid` / `waiting` / `canceled` / `failed`) · 결제 키 · 영수증 |
| `licenses/{id}` | 키 · 주인 · 상태 (`active` / `revoked`) · 기기 수 · 새 버전 무료 기한 |
| `licenses/{id}/activations/{machineId}` | 등록한 컴퓨터 이름 · OS · 앱 버전 · 처음/최근 확인 |

사진이나 사진 분석 결과는 어디에도 보내지 않습니다.
