#!/usr/bin/env bash
# Downloads the CEF binary distribution matching the pinned vendor/cef-rs
# submodule into vendor/cef, using cef-rs's own exporter. The CEF version is
# pinned by the submodule commit, not by this script.
#
# After running, set CEF_PATH=<repo>/vendor/cef (and PATH / LD_LIBRARY_PATH /
# DYLD_FALLBACK_LIBRARY_PATH per vendor/cef-rs/README.md) before `cargo build`.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root/vendor/cef-rs"
cargo run --release -p export-cef-dir -- --force "$root/vendor/cef"
echo "CEF exported to $root/vendor/cef"
