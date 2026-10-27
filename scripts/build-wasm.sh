#!/bin/sh
# Builds games/raffle/wasm for wasm32-unknown-unknown on the STABLE toolchain
# (single-threaded) and writes the wasm-bindgen output for the web page to
# games/raffle/web/src/wasm/.
#
# Needs: the wasm32-unknown-unknown target, wasm-bindgen-cli matching the
# locked wasm-bindgen (0.2.128), clang with the WebAssembly backend (Apple
# clang has it) for secp256k1-sys, and an llvm-ar (Apple's ar cannot index
# wasm objects). LLVM_AR defaults to the one shipped in a rustup toolchain.
set -eu
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
if [ -z "${LLVM_AR:-}" ]; then
  LLVM_AR="$(find "$HOME/.rustup/toolchains" -path '*/bin/llvm-ar' -type f 2>/dev/null | head -1)"
fi
[ -x "$LLVM_AR" ] || { echo "build-wasm.sh: no llvm-ar found; set LLVM_AR (rustup component add llvm-tools)" >&2; exit 1; }
export AR_wasm32_unknown_unknown="$LLVM_AR"
export CC_wasm32_unknown_unknown="${CC_wasm32_unknown_unknown:-clang}"
env -u RUSTFLAGS cargo build -p zecraft-raffle-wasm --lib --release --target wasm32-unknown-unknown
out="$root/games/raffle/web/src/wasm"
rm -rf "$out"
wasm-bindgen target/wasm32-unknown-unknown/release/zecraft_raffle_wasm.wasm --target web --out-dir "$out"
# Optional: binaryen's wasm-opt (the web package's devDependency, or WASM_OPT).
# -O3, not -Oz: the in-browser Halo2 check is the slow part, keep it fast.
opt="${WASM_OPT:-$root/games/raffle/web/node_modules/.bin/wasm-opt}"
if [ -x "$opt" ]; then
  "$opt" -O3 --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext \
    --enable-mutable-globals --enable-reference-types --enable-multivalue \
    "$out/zecraft_raffle_wasm_bg.wasm" -o "$out/zecraft_raffle_wasm_bg.wasm"
else
  echo "build-wasm.sh: no wasm-opt (npm install in games/raffle/web adds it); the wasm is not optimised"
fi
ls -l "$out/zecraft_raffle_wasm_bg.wasm"
