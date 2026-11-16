# App icon and character

`icon-source.webp` is the original artwork: a rounded square with a soft glow, on a white
background — a white character looking through a loupe, on an orange-to-pink gradient.
Everything else here (and the Android icons, and the Google Play icon) is generated from it with

```bash
python3 scripts/make-icons.py      # needs Pillow + NumPy
```

The script finds the outline of the rounded square, fits a symmetric shape to it, and grows the
picture a little past that outline so that it can be cut to any shape.

| File | Used by |
|---|---|
| `AppIcon.icns` | macOS `.app` bundle (`scripts/macos-bundle.sh`, `CFBundleIconFile`) — Big Sur grid: 824 px tile on a 1024 px canvas with a soft shadow |
| `icon-macos-1024.png` | the same as a PNG (preview, website) |
| `icon.ico` | Windows `.exe` resource (16–256 px), embedded by `crates/app/build.rs` |
| `icon-256.png` | window / taskbar icon at runtime (`src/icon.rs`), README, website |
| `icon-512.png` | the mascot inside the app (welcome screen, empty views), website |
| `icon-1024.png` | full-bleed master |
| `character.png` | the character on its own, transparent background — store pictures (`scripts/make-store-images.py`) |

Generated elsewhere by the same script:

| File | Used by |
|---|---|
| `android/app/src/main/res/mipmap-*/ic_launcher_foreground.png` | Android adaptive icon: the picture grown to 108dp, the tile on the middle 72dp |
| `android/app/src/main/res/mipmap-*/ic_launcher_monochrome.png` | Android themed icon: the character as one colour |
| `android/app/src/main/res/drawable-nodpi/mascot.png` | the mascot inside the Android app |
| `docs/play-store/graphics/icon-512.png` | Google Play icon (a full square; Play cuts its own shape) |

The 크림 / 코코아 themes (`src/theme.rs`) keep the colours of the first icon (paper `#FDF3E6`,
outline `#3D2B21`, accent `#F87E4A`, star `#FCB840`); the accent orange is also the orange of
this icon's gradient.
