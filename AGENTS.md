# 작업 규칙 (에이전트 · 개발자 · 리뷰어)

이 저장소에서 일할 때 지키는 규칙입니다. 사람이든 AI 에이전트든 같습니다.

**사용자** 는 이 앱을 직접 쓰는 사진가이자 제품 책임자입니다. 사용자와는 **한국어** 로 이야기하고, 사용자가 읽는 글(릴리스 노트 · README · 앱 문구)도 한국어로 씁니다.

## 1. 먼저 읽을 것

1. **[docs/DEVLOG.md](docs/DEVLOG.md)** — 지금까지의 요청 · 피드백 · 결정과 이유 · 열린 과제.
   - 2절 *제품 원칙* 과 3절 *반복해서 나온 피드백* 은 새 작업이 어기면 안 되는 기준입니다.
2. [README.md](README.md) — 기능과 단축키 (사용자용 설명서).
3. [docs/cull-engine/ROADMAP.md](docs/cull-engine/ROADMAP.md) — 셀렉 엔진의 계획과 "재고 나서 고친다" 원칙.
4. [LICENSING.md](LICENSING.md) — MIT(앱 · 코어) + BUSL 1.1(셀렉 엔진).

## 2. 구조

| 경로 | 내용 | 라이선스 |
|---|---|---|
| `crates/core` | 가져오기 · 카탈로그(SQLite) · EXIF · XMP · 내보내기 · 크롭(`framing.rs`) | MIT |
| `crates/cull` | 셀렉 엔진: 분석 · 얼굴 · 장면 · 인물 분할 · 구도 · 학습 · 평가, 내장 모델 | BUSL 1.1 |
| `crates/app` | egui 데스크톱 앱 (`--no-default-features` = Mac App Store 판) | MIT |
| `crates/cli` | `photoorg` 명령줄 도구 (`cull-eval` 포함) | MIT |
| `crates/macos` | 샌드박스 북마크 · Finder · 추출 | MIT |
| `crates/android` · `android/` | UniFFI 다리 · Kotlin/Compose 앱 | MIT |
| `crates/ios` · `ios/` | UniFFI 다리(`LoupeCore.xcframework`) · SwiftUI 아이폰/아이패드 앱 | MIT |
| `site/` | 판매 사이트 (Firebase · 토스페이먼츠) | MIT |

## 3. 바꾼 뒤 확인할 것

```bash
cargo fmt --all
cargo clippy --workspace --all-targets        # 경고 0
cargo test --workspace
cargo test -p photoorg-app --no-default-features   # Mac App Store 판 (CI 도 돌림)
```

- **안드로이드**: `cd android && ./gradlew :app:testDebugUnitTest` 로 확인합니다. JDK 21 이 필요합니다 (Robolectric).
- **iOS / iPadOS**: macOS(Xcode 16+ · XcodeGen)에서 `./ios/Scripts/build-rust-ios.sh --sim-only --debug && (cd ios && xcodegen generate)` 후 `LoupeTests` 를 돌립니다 (CI `ios.yml` 도 돌림).
- **화면을 바꿨으면** 앱을 실제로 띄워 스크린샷으로 봅니다. 리눅스에서는 Xvfb + xdotool 로 띄웁니다.
  - 창 폭 **960(최소) · 1200 · 1600** 에서 봅니다.
  - **정보 패널 · 사이드바를 켜고 끈** 상태를 모두 봅니다.
  - 글자 겹침이 없는지, **사진을 가리는 표시** 가 없는지 확인합니다.
- **셀렉 엔진을 바꿨으면**
  - 합성 테스트(`crates/cull`)가 통과하는지 봅니다.
  - 가능하면 `photoorg cull-eval` 숫자를 바꾸기 전과 후로 비교합니다. 나빠진 숫자도 그대로 보고합니다.
  - 분석 결과의 형식이 바뀌면 `ANALYSIS_VERSION` 을 올립니다. 올리면 업데이트 뒤 사진을 다시 분석합니다.
- **카탈로그 스키마를 바꿨으면** 마이그레이션을 넣습니다. 이전 버전으로 만든 라이브러리를 새 버전으로 열어 별점 · 판정이 남는지 봅니다.
- 이 환경(리눅스)에서 못 본 것은 못 봤다고 적습니다. 맥 샌드박스 · 실제 SD 카드 · 안드로이드 실기기가 그렇습니다.

## 4. 브랜치 · 머지 · 릴리스

- **작업 브랜치** 는 세션에서 지정된 브랜치입니다. 지금은 `claude/rust-photo-organizer-ewmqk7` 입니다.
  - force push 하지 않습니다.
  - 다른 브랜치에는 허락 없이 푸시하지 않습니다.
- **PR 은 사용자가 요청할 때만** 엽니다.
  - "머지하고 릴리즈" 는 다음 순서를 뜻합니다: PR 열기 → CI 확인 → **squash 머지** → 릴리스 파일 확인 → 보고.
  - PR 본문은 무엇을 · 왜 · 어떻게 확인했는지를 씁니다.
- **squash 머지 뒤** 작업 브랜치를 main 에 다시 맞춥니다. 예전 기록을 합쳐 두어 푸시가 fast-forward 가 되게 합니다:
  ```bash
  git fetch origin main
  git checkout -B claude/rust-photo-organizer-ewmqk7 origin/main
  git merge -s ours --no-edit -m "이전 브랜치 기록 합치기 (#N squash 머지 이후)" <머지 전 브랜치 HEAD>
  git push -u origin claude/rust-photo-organizer-ewmqk7
  ```
- **릴리스 만들기** (데스크톱)
  1. `Cargo.toml` 의 `[workspace.package] version` 을 올립니다. 안드로이드 versionCode 도 이 값에서 나옵니다.
  2. `docs/releases/v<버전>.md` 를 씁니다. 앞 버전의 형식을 따릅니다: 첫 줄 굵은 요약 → 내려받기 표 → 맥 터미널 설치 · 업데이트 명령 → 바뀐 것.
  3. **셀렉 엔진이 바뀐 릴리스면** `crates/cull/LICENSE` 의 Change Date 를 *릴리스 날 + 4년* 으로 올리고, `LICENSING.md` 의 날짜 목록에 더합니다. 이유는 LICENSING.md 에 있습니다.
  4. [docs/DEVLOG.md](docs/DEVLOG.md) 에 항목을 더하고, 1절 *지금 상태* 를 고칩니다.
  5. main 에 들어가면 `build.yml` 이 릴리스를 만듭니다. **파일 4개**(맥 dmg · zip, Windows zip, Linux tar.gz)가 다 올라간 것을 확인한 뒤 사용자에게 링크와 맥 터미널 명령을 알립니다.
     - 빠진 파일은 다음 main 푸시에서 자동으로 채워집니다.
- **Mac App Store**
  - `app-store.yml` 은 수동 실행입니다. 자동화 권한으로는 실행 요청이 403 으로 막히므로 **사용자가 직접 실행** 합니다 (Actions → app-store → Run workflow).
  - 빌드 번호는 실행 번호입니다. App Store Connect 의 버전 번호는 올린 빌드와 맞춰야 합니다.
- **Google Play**: 절차는 [docs/play-store/README.md](docs/play-store/README.md) 에 있습니다. 패키지 이름 `io.github.mj_best.loupe` 은 바꾸지 않습니다.

## 5. 보안 · 개인 정보

- **인증서(.p12) · API 키(.p8) · 암호 · 결제 키는 GitHub 시크릿에만** 넣습니다.
  - 채팅 · 이슈 · 저장소에 붙여 넣게 하지 않습니다.
  - 사용자가 보내려 하면 멈추게 합니다.
- **공개 문서에 개인 정보를 쓰지 않습니다.** 개인 이메일, 대표자 생년월일, 인증서에 찍힌 실명 등이 그렇습니다. 사업자 정보는 사이트 설정 파일 한 곳에만 둡니다.
- 커밋 · PR · 코드에 AI 모델 이름을 쓰지 않습니다.
- **사진은 기기 밖으로 나가지 않습니다.** 새 네트워크 코드를 넣지 않습니다. 라이선스 확인만 예외이고, 그것도 App Store · 안드로이드 판에는 없습니다.
- 모델 · 데이터셋은 **상업 사용이 되는 라이선스** 만 씁니다 (ROADMAP 4절). 새 모델은 라이선스 파일을 `crates/cull/models/` 에 함께 넣습니다.

## 6. 기록 규칙

사용자의 요청 · 피드백 하나마다 **같은 PR 에서** [docs/DEVLOG.md](docs/DEVLOG.md) 를 고칩니다.

- **요청 · 피드백**: 요약하되, 판단에 중요한 표현은 원문 그대로 따옴표로 옮깁니다. 스크린샷으로 받은 피드백은 무엇이 보였는지 적습니다.
- **반영**: 무엇이 바뀌었는지. 코드 위치는 파일 이름 정도만 적습니다.
- **결정과 이유**: 다른 방법을 두고 왜 이렇게 했는지 적습니다. 해 보고 버린 방법(예: 너무 느린 모델)도 적습니다.
- **확인**: 어떻게 확인했는지, 무엇은 확인하지 못했는지 적습니다.
- **남은 점**: 그리고 4절 *열린 과제* 를 고칩니다.
- 같은 피드백이 두 번 나오면 3절 *반복해서 나온 피드백* 에 올리고, 다시 생기지 않게 할 방법을 적습니다.
- 실패 · 나빠진 숫자 · 실수도 그대로 적습니다. 예: 0.3.0 의 정확도 2%p 하락, #16 의 Java 버전 실수.
