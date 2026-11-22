#!/usr/bin/env bash
# Builds the Rust `photoorg-ios` static libraries for iOS device and/or simulator, generates Swift
# UniFFI bindings, and packages them into `ios/Generated/LoupeCore.xcframework`.
#
# Usage (on macOS with Xcode and rustup):
#   ./ios/Scripts/build-rust-ios.sh               # release device + simulator XCFramework
#   ./ios/Scripts/build-rust-ios.sh --sim-only    # simulator-only build for CI / local testing
#   ./ios/Scripts/build-rust-ios.sh --device-only # device-only arm64 build for App Store archive
#   ./ios/Scripts/build-rust-ios.sh --sim-only --debug

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
IOS_DIR="$ROOT/ios"
OUT_DIR="$IOS_DIR/Generated"
HEADERS_DIR="$OUT_DIR/headers"
BUILD_SIM=true
BUILD_DEV=true
PROFILE="release"

# Match ios/project.yml deploymentTarget (17.0) so cc-rs (sqlite3.o, tract-linalg) and rustc
# emit iOS 17.0 LC_BUILD_VERSION load commands instead of the latest SDK version.
export IPHONEOS_DEPLOYMENT_TARGET="${IPHONEOS_DEPLOYMENT_TARGET:-17.0}"

for arg in "$@"; do
  case "$arg" in
    --sim-only) BUILD_DEV=false ;;
    --device-only) BUILD_SIM=false ;;
    --debug) PROFILE="debug" ;;
    *) echo "Unknown option: $arg" >&2; exit 2 ;;
  esac
done

if [[ "$BUILD_SIM" == "false" && "$BUILD_DEV" == "false" ]]; then
  echo "Cannot combine --sim-only and --device-only" >&2
  exit 2
fi

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

BINDGEN_LIB=""
XC_ARGS=()

if [[ "$BUILD_SIM" == "true" ]]; then
  echo "==> Building photoorg-ios for simulator ($SIM_TARGET, $PROFILE)…"
  cargo build -p photoorg-ios --target "$SIM_TARGET" ${CARGO_PROFILE_FLAG[@]+"${CARGO_PROFILE_FLAG[@]}"}
  SIM_LIB="$ROOT/target/$SIM_TARGET/$PROFILE/libphotoorg_ios.a"
  XC_ARGS+=(-library "$SIM_LIB" -headers "$HEADERS_DIR")
  BINDGEN_LIB="$SIM_LIB"
fi

if [[ "$BUILD_DEV" == "true" ]]; then
  echo "==> Building photoorg-ios for iOS device (aarch64-apple-ios, $PROFILE)…"
  cargo build -p photoorg-ios --target aarch64-apple-ios ${CARGO_PROFILE_FLAG[@]+"${CARGO_PROFILE_FLAG[@]}"}
  DEV_LIB="$ROOT/target/aarch64-apple-ios/$PROFILE/libphotoorg_ios.a"
  XC_ARGS+=(-library "$DEV_LIB" -headers "$HEADERS_DIR")
  if [[ -z "$BINDGEN_LIB" ]]; then
    BINDGEN_LIB="$DEV_LIB"
  fi
fi

echo "==> Generating Swift UniFFI bindings from $(basename "$BINDGEN_LIB")…"
cargo run -p photoorg-ios --features bindgen --bin uniffi-bindgen -- \
  generate --library "$BINDGEN_LIB" --language swift --out-dir "$OUT_DIR"

# Move C header & modulemap into dedicated headers directory for xcodebuild -create-xcframework.
mv "$OUT_DIR"/photoorg_iosFFI.h "$HEADERS_DIR/"
mv "$OUT_DIR"/photoorg_iosFFI.modulemap "$HEADERS_DIR/module.modulemap"

echo "==> Packaging LoupeCore.xcframework…"
xcodebuild -create-xcframework \
  "${XC_ARGS[@]}" \
  -output "$OUT_DIR/LoupeCore.xcframework"

echo "==> Done: $OUT_DIR/LoupeCore.xcframework + $OUT_DIR/photoorg_ios.swift"
