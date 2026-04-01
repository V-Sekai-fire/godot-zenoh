#!/bin/bash

# Copyright (c) 2026-present K. S. Ernest (iFire) Lee
# SPDX-License-Identifier: MIT

# Build script for Godot Zenoh GDExtension
# This builds the Rust library for the target platform.
#
# Single and double precision libraries are placed in the same addon directory
# with platform/precision in the filename, matching godot-zenoh.gdextension:
#   single → libgodot_zenoh.{platform}.template_release.{arch}.{ext}
#   double → libgodot_zenoh.{platform}.template_release.double.{arch}.{ext}
#
# Usage:
#   ./misc/build.sh                   # single precision (default)
#   PRECISION=double ./misc/build.sh  # double precision
#   PRECISION=both   ./misc/build.sh  # both precisions

set -e

PRECISION="${PRECISION:-single}"

# ── Detect platform / arch ────────────────────────────────────────────────────
case "$(uname -s)" in
    Linux)
        PLATFORM="linux"
        LIB_EXT="so"
        ;;
    Darwin)
        PLATFORM="macos"
        LIB_EXT="dylib"
        ;;
    CYGWIN*|MINGW32*|MSYS*|MINGW*)
        PLATFORM="windows"
        LIB_EXT="dll"
        ;;
    *)
        echo "Unsupported platform"
        exit 1
        ;;
esac

case "$(uname -m)" in
    arm64|aarch64) ARCH="arm64" ;;
    x86_64)        ARCH="x86_64" ;;
    *)             echo "Unsupported arch: $(uname -m)"; exit 1 ;;
esac

# macOS builds target universal; Linux/Windows use explicit arch
if [ "$PLATFORM" = "macos" ]; then
    ARCH_LABEL="universal"
else
    ARCH_LABEL="$ARCH"
fi

echo "Building Godot Zenoh GDExtension (precision=$PRECISION, platform=$PLATFORM, arch=$ARCH_LABEL)..."

mkdir -p sample/addons/godot-zenoh

# ── Helper: build one precision ───────────────────────────────────────────────
build_precision() {
    local prec="$1"  # "single" or "double"
    local cargo_features=""
    local prec_label=""

    if [ "$prec" = "double" ]; then
        cargo_features="--features double-precision"
        prec_label=".double"
        # double-precision feature uses api-custom and needs the Godot binary
        # to extract the extension API JSON at compile time.
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
    # Separate target dirs so both precisions can coexist without a full rebuild
    CARGO_TARGET_DIR="target/${prec}" cargo build --release $cargo_features

    local src_lib="target/${prec}/release/libgodot_zenoh.$LIB_EXT"
    # Filename matches gdextension [libraries] keys, e.g.:
    #   libgodot_zenoh.linux.template_release.x86_64.so
    #   libgodot_zenoh.linux.template_release.double.x86_64.so
    local dst_name="libgodot_zenoh.${PLATFORM}.template_release${prec_label}.${ARCH_LABEL}.${LIB_EXT}"
    local dst_lib="sample/addons/godot-zenoh/$dst_name"

    if [ ! -f "$src_lib" ]; then
        echo "Error: Built library not found at $src_lib"
        exit 1
    fi

    cp "$src_lib" "$dst_lib"

    # On macOS, code signing is required for libraries
    if [ "$LIB_EXT" = "dylib" ]; then
        echo "Code signing for macOS..."
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
echo "Done. Libraries in sample/addons/godot-zenoh/:"
ls -lh sample/addons/godot-zenoh/libgodot_zenoh.* 2>/dev/null | awk '{print "  " $NF}' || true
