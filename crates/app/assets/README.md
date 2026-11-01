# App icon

`icon-source.webp` is the original artwork (a rounded square on a black background).
Everything else here is generated from it with

```bash
python3 scripts/make-icons.py crates/app/assets/icon-source.webp crates/app/assets   # needs Pillow + NumPy
```

| File | Used by |
|---|---|
| `AppIcon.icns` | macOS `.app` bundle (`scripts/macos-bundle.sh`, `CFBundleIconFile`) — Big Sur grid: 824 px artwork on a 1024 px canvas with a soft shadow |
| `icon-macos-1024.png` | the same as a PNG (preview) |
| `icon.ico` | Windows `.exe` resource (16–256 px), embedded by `crates/app/build.rs` |
| `icon-256.png` | window / taskbar icon at runtime (`src/icon.rs`), README |
| `icon-512.png` | mascot inside the app (welcome screen, empty views) |
| `icon-1024.png` | full-bleed master |

The colours of the 크림 / 코코아 themes (`src/theme.rs`) are sampled from this artwork:
paper `#FDF3E6`, outline `#3D2B21`, check badge `#F87E4A`, sparkle `#FCB840`.
