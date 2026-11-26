#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

out_dir="${1:-bindings}"
mkdir -p "$out_dir/swift" "$out_dir/kotlin"

cargo build -p reinforce-ffi --release

case "$(uname -s)" in
  Darwin) library="target/release/libreinforce_ffi.dylib" ;;
  Linux) library="target/release/libreinforce_ffi.so" ;;
  *)
    echo "unsupported platform: $(uname -s)" >&2
    exit 1
    ;;
esac

cargo run -p reinforce-ffi --bin uniffi-bindgen -- \
  generate "$library" --language swift --out-dir "$out_dir/swift"
cargo run -p reinforce-ffi --bin uniffi-bindgen -- \
  generate "$library" --language kotlin --out-dir "$out_dir/kotlin"

if command -v swiftc >/dev/null 2>&1; then
  swiftc -typecheck \
    -Xcc "-fmodule-map-file=$out_dir/swift/reinforce_ffiFFI.modulemap" \
    -I "$out_dir/swift" \
    "$out_dir/swift/reinforce_ffi.swift"
  echo "Swift bindings type-check passed."
else
  echo "swiftc not found; skipped Swift type-check."
fi
