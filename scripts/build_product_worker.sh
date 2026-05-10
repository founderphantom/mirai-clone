#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PRODUCT_DIR="$ROOT_DIR/workers/product"
TOOL_ROOT="$PRODUCT_DIR/.cargo-bin"
WORKER_BUILD_VERSION="0.1.14"
WASM_BINDGEN_VERSION="0.2.121"

ensure_cargo_tool() {
  local crate="$1"
  local version="$2"

  if ! cargo install --list --root "$TOOL_ROOT" | grep -q "^${crate} v${version}:"; then
    cargo install --locked --version "$version" --root "$TOOL_ROOT" "$crate"
  fi
}

cd "$PRODUCT_DIR"

ensure_cargo_tool "worker-build" "$WORKER_BUILD_VERSION"
ensure_cargo_tool "wasm-bindgen-cli" "$WASM_BINDGEN_VERSION"

WASM_BINDGEN_BIN="$TOOL_ROOT/bin/wasm-bindgen" "$TOOL_ROOT/bin/worker-build" --release
