#!/usr/bin/env sh
# Compila el motor a WebAssembly y deja la web lista en web/ (estática).
# Requiere: rustup target add wasm32-unknown-unknown
#           cargo install wasm-bindgen-cli --version <la del Cargo.lock>
set -eu
cd "$(dirname "$0")/.."
cargo build -p montajes-web --target wasm32-unknown-unknown --release
wasm-bindgen --target web --no-typescript --out-dir web/motor \
  target/wasm32-unknown-unknown/release/montajes_web.wasm
echo "Web lista en web/ ($(du -h web/motor/montajes_web_bg.wasm | cut -f1) de motor)"
