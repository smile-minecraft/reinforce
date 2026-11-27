#!/usr/bin/env bash
# Checks for the GPUI desktop client.
#
# GPUI only builds on macOS with a GUI toolkit and a full Metal toolchain, so it
# is excluded from `scripts/verify.sh` and from Linux CI. Run this script on a
# Mac before touching `crates/reinforce-desktop`; it formats, lints, tests and
# builds the crate with a bounded number of codegen jobs because GPUI is heavy.
set -euo pipefail

cd "$(dirname "$0")/.."

export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}"

cargo fmt -p reinforce-desktop --check
cargo clippy -p reinforce-desktop --all-targets -- -D warnings
cargo test -p reinforce-desktop
cargo build -p reinforce-desktop
