#!/usr/bin/env bash
#
# Builds the macOS-native DBM frontend.
#
#   ./build.sh            # debug bridge + app bundle
#   ./build.sh release    # release bridge + app bundle
#   ./build.sh check      # typecheck the Swift sources only
#
# The Rust bridge is a static library linked into the Swift executable; the
# result is an ad-hoc local test app, not a signed or notarized release.

set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")"

# Keep the Rust objects and the Swift objects on the same deployment target.
export MACOSX_DEPLOYMENT_TARGET=13.0

mode="${1:-debug}"
target="$(uname -m)-apple-macos13.0"
app="build/DBM Native.app"
profile_dir="debug"
cargo_flags=()

# The Rust static library does not carry its C dependencies' link flags, so the
# final link needs the system libraries its dependencies use: flate2's zlib
# backend, whoami's SystemConfiguration lookup, and the Security/CoreFoundation
# frameworks behind keyring and native-tls.
native_libs=(-lz -framework Security -framework CoreFoundation -framework SystemConfiguration)

case "$mode" in
  debug) ;;
  release)
    profile_dir="release"
    cargo_flags+=(--release)
    ;;
  check) ;;
  *)
    echo "usage: $0 [debug|release|check]" >&2
    exit 2
    ;;
esac

echo "==> Building the Rust bridge ($mode)"
if [[ "$mode" == "check" ]]; then
  cargo check --manifest-path bridge/Cargo.toml
else
  cargo build --manifest-path bridge/Cargo.toml "${cargo_flags[@]}"
fi

echo "==> Typechecking the Swift sources"
swiftc -typecheck \
  -target "$target" \
  -L "bridge/target/$profile_dir" -ldbm_native_bridge \
  DBMNative/*.swift

if [[ "$mode" == "check" ]]; then
  echo "==> Typecheck passed"
  exit 0
fi

echo "==> Compiling the app bundle"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
swiftc -parse-as-library -O \
  -target "$target" \
  -o "$app/Contents/MacOS/DBMNative" \
  -L "bridge/target/$profile_dir" -ldbm_native_bridge \
  "${native_libs[@]}" \
  DBMNative/*.swift
cp Info.plist "$app/Contents/Info.plist"

echo "==> Built $app"
echo "    Run it with: open \"$app\""
