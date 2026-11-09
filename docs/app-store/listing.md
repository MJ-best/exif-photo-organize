# App Store Connect 에 넣을 내용

App Store Connect → 앱 → (버전) 화면에 그대로 옮겨 적으면 됩니다. 기본 언어는 **한국어**, 영어는 *현지화 추가 → English (U.S.)*.

## 앱 정보

| 항목 | 값 |
|---|---|
| 이름 (30자) | `LOUPE - 사진가의 서랍, 사진을 고르는 시간` (또는 이름 `LOUPE`, 부제 `사진가의 서랍, 사진을 고르는 시간`) |
| 번들 ID | `io.github.mj-best.photo-organize` (첫 업로드 뒤에는 바꿀 수 없음) |
| SKU | `LOUPE-MAC` |
| 카테고리 | 기본 **사진 (Photography)**, 보조 **생산성 (Productivity)** |
| 연령 등급 | 4+ (설문에서 모두 ‘없음’) |
| 저작권 | © 2026 은총 |
| 가격 | 유료 — 가격 등급에서 고름 (예: 89,000원 근처) |
| 개인정보 보호 (앱이 수집하는 데이터) | **데이터를 수집하지 않음** — 앱스토어판은 인터넷에 연결하지 않습니다 |
| 수출 규정 (암호화) | 암호화를 쓰지 않음 (Info.plist `ITSAppUsesNonExemptEncryption = NO` 로 자동) |
| 개인정보처리방침 URL | `https://<사이트 도메인>/privacy` (판매 사이트 배포 뒤) |
| 지원 URL | `https://<사이트 도메인>/` — 사이트 배포 전에는 `https://github.com/MJ-best/exif-photo-organize` |

스크린샷(2880 × 1800): `docs/app-store/screenshots/` 의 5장, 이 순서로 — 라이브러리, B컷 후보, A컷 후보, 작품, 크게 보기. 지금 것은 견본 사진(그림)으로 찍었으니, 실제 작품 사진으로 다시 찍으면 훨씬 좋습니다.

## 한국어

**부제 (30자)**
사진가의 서랍, 사진을 고르는 시간

**프로모션 텍스트 (170자)**
사진가의 서랍, 사진을 고르는 시간 — 고를수록 내 눈을 닮아 가는 셀렉. 흔들리고 핀 나간 B컷은 먼저 걸러 두고, 잘 나온 A컷은 앞에 모아 둡니다. 사진은 기기 밖으로 나가지 않아요.

**설명**

```
사진가의 서랍, 사진을 고르는 시간.
SD 카드를 꽂고, 고르고, 작품 폴더로. 사진가를 위한 정리 · 셀렉 앱입니다.

LOUPE 는 촬영한 사진을 날짜별로 정리하고, 수천 장 가운데 흔들리고 핀 나간 B컷은 먼저 걸러 두고, 잘 나온 A컷은 앞에 모아 둡니다. 평소처럼 고르기만 하면 당신의 기준과 취향을 배워요. 편집 기능은 일부러 넣지 않았습니다 — 고르는 일에만 집중한, 가볍고 빠른 앱입니다.

■ 가져오기 · 정리
· SD 카드나 폴더의 사진을 복사하면서 날짜별 폴더로 정리
· 이미 가져온 사진은 건너뛰고, 복사한 뒤 체크섬으로 확인
· RAW 와 JPEG 은 한 장으로 묶어서

■ 셀렉
· 1–5 별점, P 픽, X 거절 — 키 하나로
· RAW 를 현상하지 않고 내장 미리보기로 바로 — 10만 장 라이브러리도 가볍게
· 카메라 · 렌즈 · ISO · 초점거리 · 별점 · 날짜로 즉시 걸러 보기

■ 셀렉 도우미
· B컷 후보: 초점 흐림 · 흔들림 · 얼굴 초점 빗나감 · 눈 감음 의심 · 노출 과다/부족 · 연사 중 덜 선명
· A컷 후보: 가장 잘 나온 사진, 연사마다 베스트 한 장
· 저절로 지우지 않습니다. 먼저 보여 줄 뿐, 마지막 결정은 언제나 사진가가 합니다
· 다 고른 뒤 거절한 사진은 한 번에 휴지통으로 — RAW · JPEG · XMP 함께, 학습 기록은 남겨 둡니다

■ 고를수록 배우는 기준
· 픽 · 거절 · 별점이 그대로 학습 데이터 — 따로 가르칠 필요가 없어요
· 인물 · 풍경 등 장면마다 다른 기준
· 무엇을 찍었는지까지: 늘 지우는 종류의 컷을 알아봅니다
· ‘내 셀렉 정확도’ 로 학습에 쓰지 않은 사진으로 잰 정확도를 확인

■ 작품 · 라이트룸
· 사진집 · 전시처럼 작품과 테마로 나누고, 작품 폴더로 모으기 한 번
· 별점 · 거절 · 테마는 XMP 로 기록 — Lightroom · Bridge 에서 그대로

■ 기기 안에서만
· 얼굴 · 장면 인식까지 모든 분석이 이 Mac 안에서. 인터넷 연결도, 계정도 필요 없습니다
· 웨딩 · 인물처럼 고객 사진을 다루는 작업에도 안심

지원 형식: Canon CR3 · CR2, Nikon NEF, Sony ARW, Fujifilm RAF, Panasonic RW2, OM/Olympus ORF, Pentax PEF, DNG 등 대부분의 RAW 와 JPEG · HEIF

※ macOS 보안 규칙에 따라 라이브러리 폴더와 SD 카드는 처음 한 번 폴더 창에서 골라 허락해 주세요. 같은 폴더 · 카드는 다음부터 바로 열립니다.
```

**키워드 (100자, 쉼표로)**
사진,셀렉,정리,RAW,SD카드,라이트룸,별점,B컷,A컷,웨딩,스냅,EXIF,가져오기,사진작가,컬링

**이번 버전의 새로운 기능**
Mac App Store 에 처음 나왔습니다.

## English (U.S.)

**Subtitle**
Import, sort and cull photos

**Promotional text**
Culling that learns your eye. Blurry and missed-focus shots are set aside first, your best frames come first — and your photos never leave your Mac.

**Description**

```
Plug in the card, pick, and gather your work. LOUPE is an importer and culling tool for photographers.

It sorts what you shot into date folders, sets aside the blurry, shaken and missed-focus frames among thousands, and brings your best shots to the front. Just pick as you always do — it learns your standards and your taste. There is deliberately no editing: a light, fast app for choosing.

IMPORT AND SORT
• Copy from SD cards or folders into date folders, skipping what you already have, verified by checksum
• RAW and JPEG pairs are one photo

PICK
• 1–5 stars, P to pick, X to reject — one key each
• RAW files open instantly from their embedded previews — light even with 100,000 photos
• Filter by camera, lens, ISO, focal length, rating and date

CULLING ASSISTANT
• Likely B-cuts: out of focus, camera shake, missed focus on faces, closed eyes, over/under exposure, softer frames in a burst
• Likely A-cuts: your best shots, the best frame of each burst
• Nothing is deleted — it shows you first; you always decide

LEARNS AS YOU PICK
• Your picks, rejects and ratings are the training data — nothing to set up
• Separate standards per scene (portrait, landscape, …)
• Even what you shoot: it recognises the kind of frame you always throw away
• "My accuracy" shows how well it predicts photos it did not learn from

PROJECTS AND LIGHTROOM
• Sort picks into projects and themes, then gather them into project folders in one step
• Ratings, rejects and themes are written to XMP — they show up in Lightroom and Bridge

ON YOUR MAC ONLY
• All analysis, including face and scene recognition, runs on this Mac. No internet connection, no account
• Safe for client work such as weddings and portraits

Formats: most RAW files (Canon CR3/CR2, Nikon NEF, Sony ARW, Fujifilm RAF, Panasonic RW2, OM/Olympus ORF, Pentax PEF, DNG, …), JPEG and HEIF.

Note: as macOS requires, choose your library folder and each SD card once in the folder panel; after that they open directly.
```

**Keywords**
photo,culling,raw,sd card,import,lightroom,rating,exif,organizer,photographer,select,wedding

**What's new**
First release on the Mac App Store.

## 심사 메모 (App Review Information → Notes, 영어)

```
LOUPE is a photo importer and culling tool for photographers. No account or sign-in.

To try it:
1. On first launch click "라이브러리 폴더 선택…" (Choose library folder…) and choose or create any folder,
   e.g. ~/Pictures/LOUPE.
2. In the sidebar click "폴더에서 가져오기…" (Import from folder…) and choose any folder with JPEG or RAW
   photos, then "새 항목 모두 가져오기" (Import all new).
3. Rate with keys 1–5, pick with P, reject with X. The sidebar's "B컷 후보" (likely rejects) and
   "A컷 후보" (likely keepers) appear once the photos are analysed (a few seconds).

App Sandbox: the app only uses folders and SD cards the user chooses in the standard folder panel,
and keeps that permission with security-scoped bookmarks. When an SD card is inserted, the app asks
the user to choose it once.

All analysis runs on the Mac with neural networks bundled in the app (YuNet face detection, MIT;
MobileNetV2, Apache-2.0). The app makes no network connections and collects no data.
```
