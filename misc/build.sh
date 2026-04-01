#!/bin/bash

# Copyright (c) 2026-present K. S. Ernest (iFire) Lee
# SPDX-License-Identifier: MIT

# Build script for Godot Zenoh GDExtension
# This builds the Rust library for the target platform.
#
# Usage:
#   ./misc/build.sh                   # single precision (default)
#   PRECISION=double ./misc/build.sh  # double precision
#   PRECISION=both   ./misc/build.sh  # both precisions

set -e

PRECISION="${PRECISION:-single}"

echo "Building Godot Zenoh GDExtension (precision=$PRECISION)..."

# Get the target triple from uname
case "$(uname -s)" in
    Linux)
        TARGET_TRIPLE="x86_64-unknown-linux-gnu"
        LIB_EXT="so"
        ;;
    Darwin)
        case "$(uname -m)" in
            arm64)
                TARGET_TRIPLE="aarch64-apple-darwin"
                ;;
            x86_64)
                TARGET_TRIPLE="x86_64-apple-darwin"
                ;;
        esac
        LIB_EXT="dylib"
        ;;
    CYGWIN*|MINGW32*|MSYS*|MINGW*)
        TARGET_TRIPLE="x86_64-pc-windows-gnu"
        LIB_EXT="dll"
        ;;
    *)
        echo "Unsupported platform"
        exit 1
        ;;
esac

echo "Target: $TARGET_TRIPLE"
echo "Library extension: $LIB_EXT"

mkdir -p sample/addons/godot-zenoh

# ── Helper: build one precision ───────────────────────────────────────────────
build_precision() {
    local prec="$1"  # "single" or "double"
    local cargo_features=""
    local lib_suffix=""

    if [ "$prec" = "double" ]; then
        cargo_features="--features double-precision"
        lib_suffix=".double"
        # double-precision requires api-custom which needs the Godot binary.
        # Default: look for the binary built by misc/build_godot.sh.
        if [ -z "$GODOT4_BIN" ]; then
            local default_bin
            default_bin="$(pwd)/misc/godot-bin/godot.double"
            if [ -f "$default_bin" ]; then
                export GODOT4_BIN="$default_bin"
                echo "Using GODOT4_BIN=$GODOT4_BIN"
            else
                echo "ERROR: double-precision build requires GODOT4_BIN to point to"
                echo "       a double-precision Godot editor binary."
                echo "       Build one with: PRECISION=double ./misc/build_godot.sh"
                exit 1
            fi
        fi
    fi

    echo ""
    echo "=== Building extension ($prec precision) ==="
    # Build into a separate target dir so both precisions can coexist
    CARGO_TARGET_DIR="target/${prec}" cargo build --release $cargo_features

    local src_lib="target/${prec}/release/libgodot_zenoh.$LIB_EXT"
    local dst_lib="sample/addons/godot-zenoh/libgodot_zenoh${lib_suffix}.$LIB_EXT"

    if [ ! -f "$src_lib" ]; then
        echo "Error: Built library not found at $src_lib"
        exit 1
    fi

    cp "$src_lib" "$dst_lib"

    # On macOS, code signing is required for libraries
    if [ "$LIB_EXT" = "dylib" ]; then
        echo "Code signing GDExtension library for macOS..."
        codesign --force --sign - "$dst_lib"
    fi

    echo "Output: $dst_lib"
}

# ── Build ─────────────────────────────────────────────────────────────────────
case "$PRECISION" in
    single) build_precision single ;;
    double) build_precision double ;;
    both)
        build_precision single
        build_precision double
        ;;
    *)
        echo "Unknown PRECISION=$PRECISION (use: single | double | both)"
        exit 1
        ;;
esac

echo ""
echo "Done. Extension libraries in sample/addons/godot-zenoh/:"
ls -lh sample/addons/godot-zenoh/libgodot_zenoh* 2>/dev/null || true
