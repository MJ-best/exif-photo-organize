#!/usr/bin/env bash
# Builds the Rust `photoorg-ios` static libraries for iOS device + simulator, generates Swift
# UniFFI bindings, and packages them into `ios/Generated/LoupeCore.xcframework`.
#
# Usage (on macOS with Xcode and rustup):
#   ./ios/Scripts/build-rust-ios.sh            # release device + simulator XCFramework
#   ./ios/Scripts/build-rust-ios.sh --sim-only # fast simulator-only build for CI / local testing

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
IOS_DIR="$ROOT/ios"
OUT_DIR="$IOS_DIR/Generated"
HEADERS_DIR="$OUT_DIR/headers"
SIM_ONLY=false
PROFILE="release"

for arg in "$@"; do
  case "$arg" in
    --sim-only) SIM_ONLY=true ;;
    --debug) PROFILE="debug" ;;
    *) echo "Unknown option: $arg" >&2; exit 2 ;;
  esac
done

CARGO_PROFILE_FLAG=()
if [[ "$PROFILE" == "release" ]]; then
  CARGO_PROFILE_FLAG=(--release)
fi

rm -rf "$OUT_DIR"
mkdir -p "$HEADERS_DIR"

HOST_ARCH="$(uname -m)"
if [[ "$HOST_ARCH" == "arm64" ]]; then
  SIM_TARGET="aarch64-apple-ios-sim"
else
  SIM_TARGET="x86_64-apple-ios"
fi

echo "==> Building photoorg-ios for simulator ($SIM_TARGET, $PROFILE)…"
cargo build -p photoorg-ios --target "$SIM_TARGET" ${CARGO_PROFILE_FLAG[@]+"${CARGO_PROFILE_FLAG[@]}"}

SIM_LIB="$ROOT/target/$SIM_TARGET/$PROFILE/libphotoorg_ios.a"

echo "==> Generating Swift UniFFI bindings…"
cargo run -p photoorg-ios --features bindgen --bin uniffi-bindgen -- \
  generate --library "$SIM_LIB" --language swift --out-dir "$OUT_DIR"

# Move C header & modulemap into dedicated headers directory for xcodebuild -create-xcframework.
mv "$OUT_DIR"/photoorg_iosFFI.h "$HEADERS_DIR/"
mv "$OUT_DIR"/photoorg_iosFFI.modulemap "$HEADERS_DIR/module.modulemap"

XC_ARGS=(-library "$SIM_LIB" -headers "$HEADERS_DIR")

if [[ "$SIM_ONLY" == "false" ]]; then
  echo "==> Building photoorg-ios for iOS device (aarch64-apple-ios, $PROFILE)…"
  cargo build -p photoorg-ios --target aarch64-apple-ios ${CARGO_PROFILE_FLAG[@]+"${CARGO_PROFILE_FLAG[@]}"}
  DEV_LIB="$ROOT/target/aarch64-apple-ios/$PROFILE/libphotoorg_ios.a"
  XC_ARGS+=(-library "$DEV_LIB" -headers "$HEADERS_DIR")
fi

echo "==> Packaging LoupeCore.xcframework…"
xcodebuild -create-xcframework \
  "${XC_ARGS[@]}" \
  -output "$OUT_DIR/LoupeCore.xcframework"

echo "==> Done: $OUT_DIR/LoupeCore.xcframework + $OUT_DIR/photoorg_ios.swift"
