# Google Play 에 올리기 (안드로이드)

안드로이드판은 **태블릿 우선의 셀렉 앱**입니다. 촬영 현장에서 SD 카드나 USB 카드 리더로 사진을 가져와 B컷을 걸러 두고, 집에 와서 데스크톱 앱으로 이어 갑니다. 엔진(가져오기 · 카탈로그 · 셀렉 도우미 · 학습)은 데스크톱과 같은 Rust 코드이고, 화면만 안드로이드용(Kotlin · Jetpack Compose)입니다.

| | |
|---|---|
| 개발자 계정 ID | `7524051614722683224` (공개 개발자 페이지: https://play.google.com/store/apps/dev?id=7524051614722683224 — 앱이 공개된 뒤에 보임) |
| 패키지 이름 | **`io.github.mj_best.loupe`** (LOUPE 에 맞춤) — Play 주소 https://play.google.com/store/apps/details?id=io.github.mj_best.loupe. **첫 업로드 뒤에는 바꿀 수 없습니다.** (`android/app/build.gradle.kts` 의 `applicationId`. 소스 코드의 Kotlin 패키지 `io.github.mj_best.photoorganize` 는 밖에 보이지 않아 그대로) |
| 판매 방식 | 유료 앱 (Mac App Store 와 같음). 라이선스 키 · 로그인 · 구독 없음 |
| 최소 · 대상 버전 | Android 8.0 (API 26) 이상 · targetSdk 36 (2026년 Play 요건) |
| 기기 | 64비트 ARM (거의 모든 폰 · 태블릿), x86_64 (크롬북 · 에뮬레이터) |
| 권한 | **없음** — 카드는 시스템 폴더 선택기(SAF)로 읽고, 인터넷 권한도 없음 |
| 16 KB 페이지 | 모든 네이티브 라이브러리 16 KB 정렬 (Play 요건) — 확인함 |

## 1. 개발자 계정 (개인 계정 · 구글 검토 중)

- **개인 계정**이라 처음 정식 출시 전에 **테스터 12명 이상이 14일 연속 비공개 테스트**에 참여해야 합니다 (아래 5단계). 계정 검토가 끝나야 앱을 만들고 올릴 수 있습니다.
- **검토를 기다리는 동안 해 둘 것**
  1. 업로드 키 만들기 · GitHub 시크릿 넣기 (2단계) — 그래야 CI 가 서명된 AAB 를 만듭니다
  2. **테스터 12명 이상 모으기** — 안드로이드 기기와 Google 계정(Gmail)이 있는 사람. 이메일 주소 목록을 받아 두면 계정이 열리자마자 비공개 테스트를 시작할 수 있습니다. 넉넉히 15명 이상 권장 (중간에 빠지는 사람이 있으면 14일을 채우지 못함)
  3. 판매 사이트를 배포해 **개인정보처리방침** (`/privacy`) 을 공개하고, `site/site.config.json` 의 `business.email` 등을 채우기 — Play Console 이 방침 URL 과 연락 이메일을 요구합니다
- 조직 계정(D-U-N-S 번호로 인증)은 이 테스트 요건이 없는 것으로 알려져 있습니다. 나중에 사업자(은총) 명의로 옮길 수도 있지만, 지금 계정으로 진행하면 됩니다.
- **결제 프로필(판매자 계정)**: 유료 앱을 팔려면 Play Console → 설정 → 결제 프로필에서 만들고 정산 계좌를 등록합니다. 수수료는 연 매출 100만 달러까지 15% 입니다.
- 사업자 정보(상호 · 주소 · 연락처)는 스토어의 개발자 연락처에 공개됩니다. 대표자 생년월일 같은 개인 정보는 넣지 마세요.

## 2. 업로드 키 만들기 (한 번)

Play 앱 서명을 씁니다. 앱의 진짜 서명 키는 구글이 보관하고, 우리는 **업로드 키**로 서명해 올립니다. 업로드 키를 잃어버려도 Play Console 에서 재설정할 수 있습니다.

```bash
keytool -genkeypair -v -keystore upload.jks -alias upload -keyalg RSA -keysize 2048 -validity 10000
```

- `upload.jks` 와 비밀번호는 **저장소에 올리지 마세요** (`android/.gitignore` 가 막아 둠).
- GitHub → Settings → Secrets and variables → Actions 에 넣으면 CI 가 서명합니다:
  - `ANDROID_KEYSTORE_BASE64` — `base64 -w0 upload.jks` 의 결과
  - `ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS` (`upload`), `ANDROID_KEY_PASSWORD`
- 내 컴퓨터에서 빌드할 때는 `android/keystore.properties` (역시 저장소 밖):
  ```properties
  storeFile=/절대/경로/upload.jks
  storePassword=…
  keyAlias=upload
  keyPassword=…
  ```

## 3. 빌드

- **CI**: `android` 워크플로가 PR · main 에서 돌고, 아티팩트 `photo-organize-android` 에 **`app-release.aab`** (Play 업로드용)와 테스트용 APK 가 있습니다. 화면 스크린샷은 `android-screenshots`.
- **직접**: Rust · **JDK 21** · Android SDK · NDK 28 · `cargo install cargo-ndk` 가 있으면 (JDK 17 에서는 앱 테스트가 모두 실패합니다 — Robolectric 이 안드로이드 16 을 Java 21 에서만 돌림)
  ```bash
  cd android
  ./gradlew :app:testDebugUnitTest      # 엔진(JNA) + 화면 스크린샷
  ./gradlew :app:bundleRelease          # app/build/outputs/bundle/release/app-release.aab
  ```
- 버전은 `Cargo.toml` 의 버전을 그대로 씁니다 (0.6.0 → versionCode 6000). 같은 버전을 다시 올려야 하면 `-PversionCode=6001` (CI 는 저장소 변수 `ANDROID_VERSION_CODE`).

## 4. Play Console 에 앱 만들기

1. **앱 만들기** → 앱 이름 `LOUPE - 사진가의 서랍, 사진을 고르는 시간` · 기본 언어 **한국어** · **앱** · **유료** (한 번 무료로 낸 앱은 유료로 바꿀 수 없습니다)
2. **앱 콘텐츠** (모두 [listing.md](listing.md) 에 답이 있음)
   - 개인정보처리방침 URL — 판매 사이트의 `/privacy` (안드로이드판 문단 포함)
   - 광고: 없음 · 앱 액세스: 로그인 없이 모든 기능 사용 가능
   - 콘텐츠 등급 설문 → 유틸리티, 해당 없음 → 전체이용가
   - 타겟층: 18세 이상
   - **데이터 보안**: 수집 · 공유하는 데이터 없음 (인터넷 권한 자체가 없음)
3. **스토어 등록정보** — [listing.md](listing.md) 의 문구와 `docs/play-store/graphics/` 의 그림
   - 앱 아이콘 512×512 · 그래픽 이미지 1024×500
   - 휴대전화 스크린샷 (1080×1920), 7인치 태블릿 (1200×1920), 10인치 태블릿 (2560×1600)
4. **가격 · 국가**: 가격은 정해 주세요 (참고: 웹사이트판 89,000원). 처음엔 대한민국만 열고 넓혀도 됩니다.

## 5. 테스트 → 출시

1. **내부 테스트** (최대 100명, 심사 없이 바로): AAB 를 올리고 내 계정을 테스터로 넣어 내 폰 · 태블릿에서 확인. 아래 *기기에서 꼭 볼 것*.
2. **비공개 테스트** (개인 계정이라 필수): 테스터 12명 이상이 테스트 링크로 **참여(opt-in)한 채 14일 연속** 유지 → Play Console 대시보드에서 *프로덕션 액세스 신청* (테스트를 어떻게 했는지 · 받은 의견 등을 묻습니다). 중간에 새 버전을 올려도 되고, 의견을 받아 고치면 신청할 때 도움이 됩니다.
   - 유료 앱은 테스터도 결제해야 설치할 수 있습니다. 테스터 이메일을 Play Console → 설정 → **라이선스 테스트** 에도 넣으면 결제 없이 설치할 수 있습니다 (계정이 열리면 콘솔에서 확인).
3. **프로덕션** 출시 → 심사(보통 며칠).

### 기기에서 꼭 볼 것 (이 환경에는 실기기가 없어 아직 못 본 것)

- SD 카드(태블릿 슬롯)와 USB-C 카드 리더 양쪽에서 **카드의 DCIM 폴더 고르기 → 살펴보기 → 가져오기**
  - 안드로이드 11 이상은 카드의 맨 위 폴더를 고를 수 없게 막는 기기가 있어, DCIM 을 고르도록 안내합니다.
- 큰 카드(수천 장 · RAW)를 가져오는 동안 화면이 꺼지지 않는지, 앱을 잠깐 나갔다 와도 계속되는지
- 셀렉 도우미 분석 속도와 발열 (사진당 0.3–1초 예상), 끝나면 B컷 · A컷 후보가 채워지는지
- 사진 보기: 좌우로 넘기기, 두 번 눌러 확대 · 두 손가락 확대, **위로 밀면 픽 · 아래로 밀면 거절**
- 키보드를 붙인 태블릿: 1–5 별점, P · X · U, ← →, Delete
- 다크 모드(코코아), 화면 회전, 폴더블 · 분할 화면

## 알려진 제한 (1단계)

- 라이브러리는 **앱 전용 폴더**에 있어 **앱을 지우면 함께 지워집니다** (설정 화면에 안내). 다음 단계: 고른 사진 · 라이브러리를 카드나 USB 로 내보내 데스크톱 앱으로 옮기기.
- 지운 사진은 라이브러리 안의 휴지통으로 가고, 설정 → 휴지통 비우기로 완전히 지웁니다 (되살리기 화면은 아직 없음).
- 작품 · 테마, 내보내기, 검색 · 필터 바, 태그는 아직 데스크톱에만 있습니다.
- 가져오기는 앱이 열려 있는 동안 진행됩니다 (화면은 켜진 채 유지). 백그라운드 가져오기는 다음 단계.

## 구조

```
crates/android/          Rust: 앱이 부르는 엔진 인터페이스 (UniFFI) — 라이브러리 · 카드 살펴보기 · 가져오기 ·
                         셀렉 도우미 · 학습 · 지우기. 카드 파일은 앱이 연 파일 디스크립터로 받음
crates/core/src/source.rs  가져오기의 원본: 이 컴퓨터의 폴더, 또는 앱이 나열해 준 파일(안드로이드 SAF)
android/                 Gradle 프로젝트 (Kotlin · Compose)
  app/build.gradle.kts   cargo-ndk 로 엔진 빌드, Kotlin 바인딩 생성(uniffi-bindgen), 서명
  app/src/main/…/ui      화면: 라이브러리(태블릿은 사이드바, 폰은 칩) · 사진 보기 · 가져오기 · 설정
  app/src/main/…/data    카드 읽기(SAF), 썸네일 캐시
  app/src/test           엔진 테스트(JNA 로 이 컴퓨터용 엔진) · 화면 스크린샷(Robolectric + Roborazzi)
```
