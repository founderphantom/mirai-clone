#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PRODUCT_DIR="$ROOT_DIR/workers/product"
HOST_TARGET="$(rustc -vV | sed -n 's/^host: //p')"

cd "$PRODUCT_DIR"
cargo test --target "$HOST_TARGET"
