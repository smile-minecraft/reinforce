#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build -p reinforce-ffi

./scripts/generate-bindings.sh bindings

test -s bindings/swift/reinforce_ffi.swift
test -s bindings/swift/reinforce_ffiFFI.h
test -s bindings/swift/reinforce_ffiFFI.modulemap
test -s bindings/kotlin/uniffi/reinforce_ffi/reinforce_ffi.kt
