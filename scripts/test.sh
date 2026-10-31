#!/bin/bash
. scripts/setup.sh
guard() {
  local limit=$1
  shift
  timeout --kill-after=30s "$limit" "$@" || {
    local code=$?
    [ "$code" -ge 124 ] && echo "::error::$* did not finish within $limit (stalled test?)"
    return "$code"
  }
}
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
export CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true CARGO_PROFILE_RELEASE_OVERFLOW_CHECKS=true
guard 30m cargo test --release --workspace
guard 30m cargo test --release -p "$APP" -- --ignored
