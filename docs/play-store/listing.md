# Play Console 에 넣을 내용

그림은 `graphics/` 에 있습니다 (스크린샷은 실제 앱 화면을 엔진 그대로 그린 것 — `android/app/src/test/…/ScreensTest.kt`, 데모 라이브러리로 `./gradlew :app:testDebugUnitTest -Pdemo=<라이브러리 폴더>`).

| 항목 | 파일 |
|---|---|
| 앱 아이콘 (512×512) | `graphics/icon-512.png` |
| 그래픽 이미지 (1024×500) | `graphics/feature-graphic.png` |
| 휴대전화 스크린샷 (1080×1920) | `graphics/phone-*.jpg` |
| 7인치 태블릿 (1200×1920) | `graphics/tablet7-*.jpg` |
| 10인치 태블릿 (2560×1600) | `graphics/tablet10-*.jpg` |

## 앱 정보

- 앱 이름: **LOUPE - 사진가의 서랍, 사진을 고르는 시간** (기기의 홈 화면 아이콘 이름은 `LOUPE`)
- 카테고리: **사진** (Photography)
- 태그: 사진 편집기 아님 — *사진 관리*, *카메라 도구* 계열
- 연락처 이메일: (사업자 이메일 — 판매 사이트 `site.config.json` 의 `business.email` 과 같게)
- 개인정보처리방침: 판매 사이트의 `/privacy`

## 한국어

**간단한 설명** (80자 이내)

> 사진가의 서랍, 사진을 고르는 시간 — 카드에서 바로 가져와 B컷부터 걸러 주는 셀렉 앱

**자세한 설명** (4,000자 이내)

```
사진가의 서랍, 사진을 고르는 시간.
촬영 현장에서 SD 카드를 꽂고, 집에 가기 전에 B컷부터 걸러 두세요.

LOUPE 는 사진가를 위한 셀렉 앱입니다. 카드의 사진을 날짜별로 정리해 가져오고, 셀렉 도우미가 흐리거나 흔들리거나 눈 감은 컷을 먼저 찾아 보여 줍니다. 마지막 결정은 언제나 사진가가 합니다.

■ 카드에서 바로
· 태블릿의 SD 카드 슬롯, 또는 USB-C 카드 리더로 연결한 카드에서 가져오기
· 촬영 날짜별 폴더로 정리, RAW 와 JPEG 은 한 장으로 묶어서
· 복사하면서 체크섬으로 확인하고, 이미 가져온 사진은 건너뜀
· 카드는 읽기만 하고 아무것도 지우지 않음

■ 셀렉
· 사진을 크게 보고 위로 밀면 픽, 아래로 밀면 거절
· 별점 1–5, 여러 장을 골라 한 번에
· 키보드를 붙인 태블릿에서는 데스크톱과 같은 단축키 (1–5, P, X, U)
· RAW 를 현상하지 않고 내장 미리보기로 바로 — 큰 카드도 가볍게

■ 셀렉 도우미
· B컷 후보: 초점 흐림 · 흔들림 · 얼굴 초점 빗나감 · 눈 감음 의심 · 노출 과다/부족 · 연사 중 덜 선명
· A컷 후보: 가장 잘 나온 사진, 연사마다 베스트 한 장
· 저절로 지우지 않습니다. 먼저 보여 줄 뿐
· 다 고른 뒤 거절한 사진은 한 번에 휴지통으로 — 학습 기록은 남겨 둡니다

■ 고를수록 배우는 기준
· 픽 · 거절 · 별점이 그대로 학습 데이터 — 따로 가르칠 필요가 없어요
· 인물 · 풍경 등 장면마다 다른 기준, 늘 거절하는 종류의 컷까지

■ 사진은 기기 밖으로 나가지 않아요
· 분석도 학습도 모두 이 기기에서. 인터넷 권한이 없습니다
· 계정 · 로그인 · 구독 없음
· 별점 · 거절은 XMP 사이드카에도 기록 — LOUPE 데스크톱 앱, 라이트룸에서 그대로

LOUPE 데스크톱 앱(맥 · 윈도우 · 리눅스)과 같은 엔진을 씁니다.
```

**새 소식** (출시 노트, 500자 이내)

```
안드로이드 첫 버전입니다.
· SD 카드 · USB 카드 리더에서 가져오기 (날짜별 정리, 체크섬 확인)
· 셀렉 도우미: B컷 · A컷 후보, 고를수록 배우는 기준
· 위로 밀면 픽, 아래로 밀면 거절
```

## English (en-US)

**Short description**

> Import straight from the card and see blurred, shaky and closed-eye shots first

**Full description**

```
Plug in the SD card on location and clear out the misses before you head home.

LOUPE is a culling app for photographers. It imports photos from the card into dated folders, and its culling assistant shows you the blurred, shaky and closed-eye shots first. The final call is always yours.

■ Straight from the card
· Import from a tablet's SD card slot or a USB-C card reader
· Dated folders; RAW and JPEG kept together as one photo
· Checksums verified while copying; photos already imported are skipped
· The card is only read, never changed

■ Culling
· Swipe up to pick, swipe down to reject
· 1–5 stars, on many photos at once
· On a tablet with a keyboard: the desktop shortcuts (1–5, P, X, U)
· Embedded previews instead of developing RAW files — big cards stay fast

■ Culling assistant
· B-cut candidates: missed focus, camera shake, face out of focus, closed eyes, over/under exposure, weaker burst frames
· A-cut candidates: your best photos, one per burst
· Nothing is deleted on its own — it only shows you first
· When you are done, move rejected photos to the trash in one go; what you decided stays for learning

■ Learns from your picks
· Picks, rejects and stars are the training data — nothing to label
· A standard per scene (people, landscapes, …), down to the kind of shots you always reject

■ Your photos stay on the device
· Analysis and learning run on the device; the app has no internet permission
· No account, no sign-in, no subscription
· Ratings and rejects also go to XMP sidecars, for the LOUPE desktop app and Lightroom

Same engine as the LOUPE desktop app (macOS, Windows, Linux).
```

## 데이터 보안 (Data safety) 답

- 앱이 사용자 데이터를 수집하거나 공유하나요? → **아니요**
  - 근거: 인터넷 권한(`android.permission.INTERNET`)이 없어 기기 밖으로 아무것도 보낼 수 없음. 광고 · 분석 SDK 없음.
- 데이터 암호화(전송 중) → 해당 없음 (전송 없음)
- 데이터 삭제 요청 방법 → 앱을 지우면 모든 데이터(라이브러리 포함)가 지워짐

## 콘텐츠 등급 설문

- 카테고리: 유틸리티 · 생산성 · 커뮤니케이션 · 기타 → **유틸리티**
- 폭력 · 성적 콘텐츠 · 욕설 · 약물 · 도박: 모두 아니요
- 사용자 간 상호작용 · 콘텐츠 공유 · 위치 공유 · 디지털 상품 구매: 모두 아니요
- 예상 등급: 전체이용가 (IARC 3+)
