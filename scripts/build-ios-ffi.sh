#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

profile="debug"
cargo_profile_args=()
out_dir="apps/ios/Generated"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --release)
      profile="release"
      cargo_profile_args+=(--release)
      shift
      ;;
    --out-dir)
      out_dir="$2"
      shift 2
      ;;
    *)
      out_dir="$1"
      shift
      ;;
  esac
done

export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}"

ios_targets=(aarch64-apple-ios aarch64-apple-ios-sim)
if command -v rustup >/dev/null 2>&1; then
  rustup_bin_dir="$(dirname "$(rustup which cargo)")"
  export PATH="$rustup_bin_dir:$PATH"
  installed_targets="$(rustup target list --installed)"
  for target in "${ios_targets[@]}"; do
    if ! grep -qx "$target" <<<"$installed_targets"; then
      echo "Adding missing Rust target: $target"
      rustup target add "$target"
    fi
  done
fi

echo "Building reinforce-ffi ($profile, CARGO_BUILD_JOBS=$CARGO_BUILD_JOBS)..."
cargo build -p reinforce-ffi ${cargo_profile_args[@]+"${cargo_profile_args[@]}"}
for target in "${ios_targets[@]}"; do
  cargo build -p reinforce-ffi --lib --target "$target" ${cargo_profile_args[@]+"${cargo_profile_args[@]}"}
done

case "$(uname -s)" in
  Darwin) host_library="target/$profile/libreinforce_ffi.dylib" ;;
  *)
    echo "iOS XCFramework packaging requires macOS (Darwin)." >&2
    exit 1
    ;;
esac

swift_dir="$out_dir/swift"
headers_dir="$out_dir/headers"
xcframework_dir="$out_dir/ReinforceFFI.xcframework"

rm -rf "$swift_dir" "$headers_dir" "$xcframework_dir"
mkdir -p "$swift_dir" "$headers_dir"

cargo run -p reinforce-ffi --bin uniffi-bindgen -- \
  generate "$host_library" --language swift --out-dir "$swift_dir"

cp "$swift_dir/reinforce_ffiFFI.h" "$headers_dir/reinforce_ffiFFI.h"
cp "$swift_dir/reinforce_ffiFFI.modulemap" "$headers_dir/module.modulemap"

xcodebuild -create-xcframework \
  -library "target/aarch64-apple-ios/$profile/libreinforce_ffi.a" \
  -headers "$headers_dir" \
  -library "target/aarch64-apple-ios-sim/$profile/libreinforce_ffi.a" \
  -headers "$headers_dir" \
  -output "$xcframework_dir"

echo "Generated Swift bindings: $swift_dir/reinforce_ffi.swift"
echo "Generated XCFramework:    $xcframework_dir"
