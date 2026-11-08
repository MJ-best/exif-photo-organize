# macOS 패키징

| 파일 | 쓰임 |
|---|---|
| `appstore.entitlements` | Mac App Store 빌드의 권한 (`scripts/macos-appstore.sh` 가 팀 ID · 번들 ID 를 채움) |
| `sandbox-test.entitlements` | 애플 계정 없이 이 맥에서 샌드박스 동작을 시험할 때 (`scripts/macos-sandbox-test.sh`) |

권한은 셋뿐입니다:

- `app-sandbox` — Mac App Store 필수 (App Review 2.4.5)
- `files.user-selected.read-write` — 사용자가 폴더 창에서 고른 라이브러리 · 카드 · 내보낼 폴더를 읽고 씀
- `files.bookmarks.app-scope` — 그 선택을 다음 실행에도 이어 씀 (보안 범위 북마크, `crates/macos`)

네트워크 권한은 없습니다. 앱스토어판에는 라이선스 기능(인터넷 연결)이 빠져 있기 때문입니다 (`--no-default-features`).
