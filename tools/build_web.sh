#!/usr/bin/env bash
# Compile the cafe engine to WebAssembly and drop it beside the page.
#
#   tools/build_web.sh          → static/pkg/causewaybay_panda_web.{js,wasm}
#
# After this, static/ is a complete cafe with no server: open index.html from
# a file or any static host and the shop runs in the tab.
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
cd "$here"

want="$(grep -oE 'wasm-bindgen = "=[0-9.]+"' Cargo.toml | grep -oE '[0-9.]+')"
have="$(wasm-bindgen --version 2>/dev/null | awk '{print $2}' || true)"
if [ "$want" != "$have" ]; then
  echo "wasm-bindgen CLI $have does not match the pinned crate $want" >&2
  echo "  cargo install wasm-bindgen-cli --version $want" >&2
  exit 1
fi
rustup target list --installed | grep -q '^wasm32-unknown-unknown$' \
  || rustup target add wasm32-unknown-unknown

cargo build --release --target wasm32-unknown-unknown -p causewaybay-panda-web
wasm-bindgen target/wasm32-unknown-unknown/release/causewaybay_panda_web.wasm \
  --out-dir static/pkg --target web --no-typescript --omit-default-module-path
# Optional: shrink further if wasm-opt is around.
if command -v wasm-opt >/dev/null; then
  wasm-opt -Os static/pkg/causewaybay_panda_web_bg.wasm -o static/pkg/causewaybay_panda_web_bg.wasm
fi
printf 'engine  %s\n' "$(du -h static/pkg/causewaybay_panda_web_bg.wasm | cut -f1)"
ls static/pkg
