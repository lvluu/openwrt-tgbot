#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

if [ "${1:-}" = "host" ]; then
	shift
	cargo build --release "$@"
	exit 0
fi

TOOLCHAIN_URL="https://github.com/cross-tools/musl-cross/releases/download/20261001/mipsel-unknown-linux-muslsf.tar.xz"
TOOLCHAIN_DIR="${TOOLCHAIN_DIR:-toolchain/mipsel-unknown-linux-muslsf}"
LINKER=$(ls "$TOOLCHAIN_DIR"/bin/mipsel-*muslsf-gcc 2>/dev/null | head -1)
if [ -z "$LINKER" ]; then
	echo "error: mipsel soft-float musl toolchain not found in $TOOLCHAIN_DIR" >&2
	echo "get it with: mkdir -p toolchain && curl -fsSL $TOOLCHAIN_URL | tar xJ -C toolchain" >&2
	exit 1
fi

export CARGO_TARGET_MIPSEL_UNKNOWN_LINUX_MUSL_LINKER="$(realpath "$LINKER")"
exec cargo +nightly build -Zbuild-std --release --target mipsel-unknown-linux-musl "$@"
