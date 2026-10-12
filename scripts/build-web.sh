#!/usr/bin/env bash
# Build the WebAssembly module into web/, plus any imported original assets.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo build --release --target wasm32-unknown-unknown -p empire-web
cp target/wasm32-unknown-unknown/release/empire_web.wasm web/empire.wasm
if command -v wasm-opt >/dev/null 2>&1; then
  wasm-opt -O3 web/empire.wasm -o web/empire.wasm
fi

# Assets converted from an original disk (tools/interstel_import) are optional
# and never committed. Copy them next to the page and write a manifest.
rm -rf web/assets/original
if [ -d assets/original ]; then
  mkdir -p web/assets/original/maps
  cp assets/original/*.tiles assets/original/*.font web/assets/original/ 2>/dev/null || true
  cp assets/original/title2.img web/assets/original/ 2>/dev/null || true
  cp assets/original/maps/*.emap web/assets/original/maps/ 2>/dev/null || true
  (
    cd web/assets/original
    files=$(find . -type f ! -name manifest.json | sed 's|^\./||' | sort | sed 's/.*/"&"/' | paste -sd, -)
    printf '{"files":[%s]}\n' "$files" > manifest.json
  )
  echo "copied original assets: $(find web/assets/original -type f | wc -l | tr -d ' ') files"
fi

size=$(wc -c < web/empire.wasm | tr -d ' ')
echo "built web/empire.wasm ($((size / 1024)) KB)"
