#!/bin/bash

# Copyright (c) 2026-present K. S. Ernest (iFire) Lee
# SPDX-License-Identifier: MIT

# Build Godot 4.6-stable from source (both single and double precision).
# Usage:
#   ./misc/build_godot.sh               # build both precisions
#   PRECISION=single ./misc/build_godot.sh
#   PRECISION=double ./misc/build_godot.sh
#   GODOT_VERSION=4.6-stable ./misc/build_godot.sh

set -e

GODOT_VERSION="${GODOT_VERSION:-4.6-stable}"
GODOT_SRC="${GODOT_SRC:-/tmp/godot-src-${GODOT_VERSION}}"
OUTPUT_DIR="${OUTPUT_DIR:-$(pwd)/misc/godot-bin}"
PRECISION="${PRECISION:-both}"  # single | double | both
JOBS="${JOBS:-$(nproc 2>/dev/null || sysctl -n hw.logicalcpu 2>/dev/null || echo 4)}"

echo "Godot version : $GODOT_VERSION"
echo "Source dir    : $GODOT_SRC"
echo "Output dir    : $OUTPUT_DIR"
echo "Precision     : $PRECISION"
echo "Parallel jobs : $JOBS"

# ── Detect platform ──────────────────────────────────────────────────────────
case "$(uname -s)" in
    Linux)  PLATFORM="linuxbsd" ;;
    Darwin) PLATFORM="macos" ;;
    *)      echo "Unsupported platform: $(uname -s)"; exit 1 ;;
esac

case "$(uname -m)" in
    arm64|aarch64) ARCH="arm64" ;;
    x86_64)        ARCH="x86_64" ;;
    *)             echo "Unsupported arch: $(uname -m)"; exit 1 ;;
esac

echo "Platform: $PLATFORM / $ARCH"

# ── Install build dependencies (Linux only) ──────────────────────────────────
if [ "$PLATFORM" = "linuxbsd" ]; then
    if command -v apt-get >/dev/null 2>&1; then
        sudo apt-get update -qq
        sudo apt-get install -y --no-install-recommends \
            build-essential scons python3 pkg-config \
            libx11-dev libxcursor-dev libxrandr-dev libxinerama-dev \
            libxi-dev libxext-dev libgl-dev libglu-dev \
            libasound2-dev libpulse-dev libfreetype6-dev libssl-dev \
            libx11-xcb-dev libxkbcommon-dev
    fi
fi

# macOS deps are assumed installed (brew install scons)
if [ "$PLATFORM" = "macos" ] && ! command -v scons >/dev/null 2>&1; then
    echo "scons not found. Install with: brew install scons"
    exit 1
fi

# ── Clone / update Godot source ───────────────────────────────────────────────
if [ ! -d "$GODOT_SRC/.git" ]; then
    echo "Cloning Godot $GODOT_VERSION..."
    git clone --depth 1 --branch "$GODOT_VERSION" \
        https://github.com/godotengine/godot.git "$GODOT_SRC"
else
    echo "Updating Godot source..."
    git -C "$GODOT_SRC" fetch --depth 1 origin "refs/tags/$GODOT_VERSION:refs/tags/$GODOT_VERSION" 2>/dev/null \
        || git -C "$GODOT_SRC" fetch --depth 1 origin "$GODOT_VERSION"
    git -C "$GODOT_SRC" checkout "$GODOT_VERSION"
fi

mkdir -p "$OUTPUT_DIR"

# ── Helper: build one precision ───────────────────────────────────────────────
build_precision() {
    local prec="$1"  # "single" or "double"
    local extra_flags=""
    local suffix=""

    if [ "$prec" = "double" ]; then
        extra_flags="precision=double"
        suffix=".double"
    fi

    echo ""
    echo "=== Building Godot $GODOT_VERSION ($prec precision) ==="
    scons -C "$GODOT_SRC" \
        platform="$PLATFORM" \
        target=editor \
        arch="$ARCH" \
        vulkan=no \
        $extra_flags \
        -j"$JOBS"

    # Locate the built binary (name varies by platform/arch/precision)
    local bin_name
    if [ "$prec" = "double" ]; then
        bin_name="$(ls "$GODOT_SRC/bin/" | grep "editor.double.$ARCH" | head -1 || true)"
        # macOS may use different naming
        [ -z "$bin_name" ] && bin_name="$(ls "$GODOT_SRC/bin/" | grep "editor" | grep "double" | head -1 || true)"
    else
        bin_name="$(ls "$GODOT_SRC/bin/" | grep "editor.$ARCH" | grep -v "double" | head -1 || true)"
        [ -z "$bin_name" ] && bin_name="$(ls "$GODOT_SRC/bin/" | grep "editor" | grep -v "double" | head -1 || true)"
    fi

    if [ -z "$bin_name" ]; then
        echo "ERROR: Could not find built binary in $GODOT_SRC/bin/"
        ls "$GODOT_SRC/bin/" || true
        exit 1
    fi

    local out_name="godot${suffix}"
    cp "$GODOT_SRC/bin/$bin_name" "$OUTPUT_DIR/$out_name"
    chmod +x "$OUTPUT_DIR/$out_name"
    echo "Output: $OUTPUT_DIR/$out_name  (from $bin_name)"
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
echo "Done. Godot binaries in $OUTPUT_DIR:"
ls -lh "$OUTPUT_DIR"/godot* 2>/dev/null || true
