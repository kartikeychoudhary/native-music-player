#!/usr/bin/env bash
# Usage: check-engine-deps.sh <engine binary> <windows|macos|linux>
# Fails if the engine links a library a user's machine may not have. Needs llvm-objdump from the
# Rust toolchain: `rustup component add llvm-tools`.
set -euo pipefail
bin="$1"; os="$2"
objdump="$(find "$(rustc --print sysroot)" -name 'llvm-objdump*' -type f | head -n 1)"
[ -x "$objdump" ] || { echo "llvm-objdump not found: rustup component add llvm-tools"; exit 2; }
case "$os" in
  windows) libs=$("$objdump" -p "$bin" | sed -n 's/^ *DLL Name: //p')
           bad=$(echo "$libs" | grep -iE '^(vcruntime|msvcp|api-ms-win-crt-)' || true) ;;
  macos)   libs=$("$objdump" --macho --dylibs-used "$bin" | tail -n +2 | awk '{print $1}')
           bad=$(echo "$libs" | grep -vE '^(/usr/lib/|/System/Library/)' || true) ;;
  linux)   libs=$("$objdump" -p "$bin" | sed -n 's/^ *NEEDED *//p')
           bad=$(echo "$libs" | grep -vE '^(libc|libm|libdl|librt|libpthread|libgcc_s|libstdc\+\+|ld-linux[-.a-z0-9_]*|libasound|libpipewire-0\.3)\.so' || true) ;;
  *) echo "unknown os $os"; exit 2 ;;
esac
# Every real engine links something; an empty list means a wrong os argument or an unreadable binary.
[ -n "$libs" ] || { echo "no linked libraries found in $bin as a $os binary (wrong os argument?)"; exit 1; }
echo "$bin links:"; echo "$libs" | sed 's/^/  /'
if [ -n "$bad" ]; then echo "not allowed:"; echo "$bad" | sed 's/^/  /'; exit 1; fi
