#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

# The GPUI desktop crate only builds on macOS and pulls in a full GUI stack, so
# it is excluded here to keep this script portable and fast; run
# `scripts/check-desktop.sh` on a Mac to check it.
cargo fmt --all --check
cargo clippy --workspace --all-targets --exclude reinforce-desktop -- -D warnings
cargo test --workspace --exclude reinforce-desktop
cargo build -p reinforce-ffi

./scripts/generate-bindings.sh bindings

test -s bindings/swift/reinforce_ffi.swift
test -s bindings/swift/reinforce_ffiFFI.h
test -s bindings/swift/reinforce_ffiFFI.modulemap
test -s bindings/kotlin/uniffi/reinforce_ffi/reinforce_ffi.kt
