# 모바일 스토어 스크린샷

아이폰 · 아이패드(iOS 시뮬레이터)와 안드로이드(Robolectric) 화면을 **실제 사진** 으로 찍는 방법입니다.

## 견본 카드 (`sample-card/`)

사용자가 찍은 사진 5장과 그 XMP 를 SD 카드처럼 둔 폴더입니다 (`DCIM/100NIKON/DSC_xxxx.JPG` + `.xmp`).

| 파일 | 장면 | 촬영 일시 |
|---|---|---|
| `DSC_0606` | 카페의 크리스마스트리 (세로) | 2025-11-30 12:41 |
| `DSC_1926` | 한옥 골목 · 역광 (세로) | 2025-12-14 15:16 |
| `DSC_1929` | 한옥 골목 · 역광 (가로) | 2025-12-14 15:16 |
| `DSC_1930` | 처마와 소나무 (세로) | 2025-12-14 15:16 |
| `DSC_2039` | 해 질 녘 한옥 지붕 (가로) | 2025-12-14 16:15 |

- 사진은 채팅으로 받으면서 **EXIF 가 지워지고 긴 변 2000px 로 줄어든** 상태였습니다.
- 그래서 EXIF 를 XMP 의 자동 태그와 맞게 다시 넣었습니다: Nikon Z f · NIKKOR Z 40mm f/2 · 40mm, 촬영 일시(파일 이름), 오프셋 +09:00.
- **ISO · 조리개 · 셔터 · 노출 보정 값은 XMP 의 구간에 맞춘 대표값** 입니다 (원래 값이 아님).
  - 감도 *저감도* → ISO 100, 조리개 *중간* → ƒ4 (카페) · ƒ5.6 (한옥)
  - 셔터 *일반* → 1/250 · 1/500 · 1/320, *고속* → 1/1600 · 1/2000, 노출 보정 *+* → +0.7
- GPS 는 넣지 않았습니다.

## 아이폰 · 아이패드 (iOS 시뮬레이터)

맥(Xcode 16+ · XcodeGen)에서:

```bash
./ios/Scripts/build-rust-ios.sh --sim-only --debug
(cd ios && xcodegen generate)
./ios/Scripts/screenshots.sh
```

- 견본 카드를 `photoorg` CLI 로 가져오고 분석한 라이브러리를, 시뮬레이터 앱의 `Documents/LOUPE` 에 넣고 찍습니다.
- 화면마다 앱을 `-LoupeScene <장면>` 으로 띄웁니다 (`ios/Loupe/App/ScreenshotScene.swift`, Debug 빌드에만 있음).
- 결과는 `docs/ios-app-store/screenshots/` 입니다: 아이폰 6.9" (Pro Max) · 아이패드 13" (iPad Pro 13).
- 맥이 없으면 GitHub Actions 의 **ios-screenshots** 워크플로가 macOS 러너에서 같은 스크립트를 돌립니다.
  - 작업 브랜치에 `ios/Loupe/**` · 이 폴더 등이 바뀌어 푸시되면 돌고, 찍은 그림을 그 브랜치에 커밋합니다.
  - main 이나 수동 실행(Actions → ios-screenshots → Run workflow)에서는 아티팩트로만 올립니다.

## 안드로이드 (Robolectric · Roborazzi)

```bash
cd android
./gradlew :app:testDebugUnitTest --tests '*ScreensTest*' -Pcard=docs/screenshots/sample-card -Pphoto=DSC_2039
```

- 견본 카드를 안드로이드 엔진으로 가져오고 분석한 뒤 화면을 그립니다. 결과는 `android/app/build/outputs/roborazzi/` 입니다.
- 가져오기 화면의 *준비* 단계는 카드를 실제로 살펴본 값입니다. *복사 중* · *완료* 단계의 숫자는 그 값에서 만든 것입니다.
- 사진 보기는 패널을 접은 상태(`viewer-phone` · `viewer-tablet7`)와 **자세히** 를 누른 상태(`*-details`)를 찍습니다. 두 상태 모두 사진 영역이 패널 · 위쪽 막대와 겹치지 않는지 테스트가 검사합니다.
- 스토어 그림 `docs/play-store/graphics/` 는 여기서 JPEG(화질 90)로 옮긴 것입니다.
  - `phone-3-viewer.jpg` ← `viewer-phone.png` (접은 상태: 사진이 크게)
  - `tablet7-2-viewer.jpg` ← `viewer-tablet7-details.png` (펼친 상태: 7인치는 펼쳐도 가로 사진이 다 보이므로 셀렉 도우미의 이유 · 구도까지 보여 줌)
  - `tablet10-3-viewer.jpg` ← `viewer-tablet.png`
