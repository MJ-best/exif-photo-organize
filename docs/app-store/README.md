# Mac App Store 에 올리기

웹사이트판(무료 체험 + 라이선스 키)은 그대로 두고, Mac App Store 에는 **유료 앱**으로 따로 냅니다. 코드는 같고, 빌드만 다릅니다.

| | 웹사이트판 | Mac App Store판 |
|---|---|---|
| 받는 곳 · 결제 | GitHub 릴리스 · 판매 사이트(토스페이먼츠) | App Store (애플 결제) |
| 라이선스 | 키 등록 (설정 → 라이선스) | 없음 — 산 사람은 그대로 씀 (App Review 2.4.5 · 3.1.1 은 키를 금지) |
| 네트워크 | 라이선스 확인에만 | 쓰지 않음 |
| 파일 접근 | 자유 | **샌드박스** — 사용자가 고른 폴더 · 카드만, 한 번 고르면 기억 |
| 업데이트 | 터미널 한 줄 / 직접 받기 | App Store 가 알아서 (매 버전 심사) |
| 수수료 | 결제 수수료(약 3%) | 애플 15% (소규모 사업자 프로그램) |
| 빌드 | `cargo build` (기본) | `scripts/macos-appstore.sh` (`--no-default-features`) |

## 1. 애플 개발자 프로그램 가입 (사용자가 할 일)

- https://developer.apple.com/programs/enroll 에서 가입. **연 99달러**(원화로 결제, 정확한 금액은 가입 화면).
- 애플 규정상 **개인사업자(1인 사업자)는 ‘개인’으로 가입**합니다. 앱스토어의 판매자 이름은 가입한 사람의 **실명**으로 나옵니다. ‘은총’ 같은 상호로 나오려면 법인과 D-U-N-S 번호가 필요합니다.
- 매출 · 정산 계좌 · 세금이 사업자와 맞도록 **사업자 대표(김경아) 명의의 Apple ID** 로 가입하는 것을 권합니다.
- 가입이 승인되면 **팀 ID**(10자)를 developer.apple.com → Membership 에서 확인합니다.

## 2. 계약 · 세금 · 정산 (App Store Connect → 비즈니스)

1. **유료 앱 계약**(Paid Applications Agreement)에 동의
2. **은행 계좌** 등록
3. **세금 양식** — 미국 밖 개인은 W-8BEN (한미 조세 조약으로 미국 판매분 원천징수가 줄어듦)
4. **소규모 사업자 프로그램** 신청 → 수수료 30% → 15% (연 매출 100만 달러 이하)

앱스토어 매출도 사업 소득입니다. 웹사이트 판매와 마찬가지로 면세사업자(출판업) 업종으로 처리되는지 세무사에게 함께 확인하세요.

## 3. 인증서 · 앱 ID · 프로필 (Mac, Xcode 필요)

1. **인증서 두 개** — Xcode → 설정 → Accounts → 팀 선택 → Manage Certificates → `+`
   - **Apple Distribution** (앱 서명)
   - **Mac Installer Distribution** (설치 패키지 서명 · 키체인에는 *3rd Party Mac Developer Installer* 로 보임)
2. **앱 ID** — developer.apple.com → Certificates, Identifiers & Profiles → Identifiers → `+` → App IDs → App
   - Bundle ID: `io.github.mj-best.photo-organize` (웹사이트판과 같음). 자기 도메인을 쓰려면 지금 정하세요 — 첫 업로드 뒤에는 못 바꿉니다 (`BUNDLE_ID` 로 넘기고 `scripts/macos-bundle.sh` 기본값도 함께).
3. **프로필** — Profiles → `+` → Distribution → **Mac App Store Connect** → 위 앱 ID → Apple Distribution 인증서 → 내려받기 (`.provisionprofile`)

## 4. App Store Connect 에 앱 만들기

앱 → `+` → 새로운 앱 → macOS, 이름, 기본 언어 **한국어**, 번들 ID, SKU. 그다음 [`listing.md`](listing.md) 의 내용을 옮겨 적습니다 — 부제 · 설명 · 키워드 · 카테고리 · 연령 등급 · 개인정보(데이터 수집 안 함) · 심사 메모 · 가격. 스크린샷은 [`screenshots/`](screenshots/) (2880 × 1800).

개인정보처리방침 URL 이 필요합니다. 판매 사이트를 배포했다면 `https://<도메인>/privacy` (앱스토어판 문단이 들어 있음).

## 5. 샌드박스 미리 확인 (애플 계정 없이도)

```bash
scripts/macos-sandbox-test.sh
```

앱스토어판과 똑같이 샌드박스 안에서 열립니다. 확인할 것:

- [ ] 라이브러리 폴더를 고르고 → 앱 종료 → 다시 열면 묻지 않고 열린다
- [ ] 외장 하드의 라이브러리: 하드를 뺐다 꽂으면 다시 열린다
- [ ] SD 카드를 꽂으면 **‘사용 허락’** 창 → 카드를 고르면 가져오기 화면. 다시 꽂으면 바로 열린다
- [ ] 폴더에서 가져오기, 내보내기, CSV 내보내기
- [ ] 작품 폴더로 모으기 → *폴더 열기* 로 Finder 가 열린다
- [ ] 카드 추출
- [ ] 설정에 *라이선스* 칸이 없다

샌드박스판의 설정은 `~/Library/Containers/io.github.mj-best.photo-organize` 에 따로 저장됩니다 (라이브러리 폴더 안의 카탈로그 · 썸네일은 웹사이트판과 함께 씀).

## 6. 빌드 올리기

**Mac 에서 직접:**

```bash
TEAM_ID=ABCDE12345 \
APP_IDENTITY="Apple Distribution: 김경아 (ABCDE12345)" \
INSTALLER_IDENTITY="3rd Party Mac Developer Installer: 김경아 (ABCDE12345)" \
PROFILE=~/Downloads/Photo_Organize_App_Store.provisionprofile \
scripts/macos-appstore.sh
```

`dist-appstore/PhotoOrganize-<버전>-<빌드>.pkg` 가 나옵니다. **Transporter** 앱(Mac App Store, 무료)에 끌어다 놓고 *전송*. App Store Connect API 키가 있으면 `--upload` 로 바로 올릴 수도 있습니다 (`ASC_KEY_ID`, `ASC_ISSUER_ID`, 키 파일 `~/.appstoreconnect/private_keys/AuthKey_<ID>.p8`).

**GitHub Actions 로:** 저장소 Settings → Secrets 에 아래를 넣고 Actions → **app-store** → Run workflow.

| 시크릿 | 내용 |
|---|---|
| `MAS_APP_CERT_P12` | Apple Distribution 인증서를 키체인에서 .p12 로 내보낸 것의 base64 (`base64 -i cert.p12 \| pbcopy`) |
| `MAS_INSTALLER_CERT_P12` | Mac Installer Distribution 인증서 .p12 의 base64 |
| `MAS_CERT_PASSWORD` | 두 .p12 의 암호 |
| `MAS_PROVISIONING_PROFILE` | 프로필 파일의 base64 |
| `APPLE_TEAM_ID` | 팀 ID |
| `ASC_KEY_ID` · `ASC_ISSUER_ID` · `ASC_KEY_P8` | App Store Connect → 사용자 및 액세스 → 통합 → API 키 (`.p8` 파일 내용 그대로) |

빌드 번호는 실행 번호로 자동으로 올라갑니다.

## 7. 심사 제출

업로드 뒤 15–30분이면 App Store Connect 에 빌드가 나타납니다. 버전 화면에서 빌드를 고르고 **심사에 제출**. 보통 1–3일 걸립니다. 거절되면 사유가 오고, 고쳐서 같은 버전으로 다시 올리면 됩니다 (빌드 번호만 올림).

## 지켜야 할 것

- 앱스토어판 안에는 웹사이트 구매 · 가격 안내 · 라이선스 키를 넣지 않습니다 (지금 빌드에는 없음). 반대로 웹사이트에 *Mac App Store 에서 받기* 링크를 다는 것은 괜찮습니다.
- 업데이트는 App Store 로만 (2.4.5 vii). 웹사이트판의 터미널 업데이트 안내는 앱스토어판에 해당하지 않습니다.
- 새 버전마다 심사를 거칩니다. 웹사이트판과 버전 번호를 맞춰 내면 관리가 쉽습니다.
