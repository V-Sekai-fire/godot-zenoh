#!/bin/bash

# Copyright (c) 2026-present K. S. Ernest (iFire) Lee
# SPDX-License-Identifier: MIT

# Build script for Godot Zenoh GDExtension
# This builds the Rust library for the target platform

set -e

echo "Building Godot Zenoh GDExtension..."

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

# Build the release version
cargo build --release

# Copy the built library to the godot-bin directory
LIB_NAME="libgodot_zenoh.$LIB_EXT"
TARGET_PATH="target/release/$LIB_NAME"

if [ -f "$TARGET_PATH" ]; then
    mkdir -p addons/godot-zenoh
    cp "$TARGET_PATH" "addons/godot-zenoh/"

    # On macOS, code signing is required for libraries
    if [ "$LIB_EXT" = "dylib" ]; then
        echo "Code signing GDExtension library for macOS..."
        codesign --force --sign - "addons/godot-zenoh/$LIB_NAME"
        echo "Library code signed."
    fi

    echo "Built library copied to addons/godot-zenoh/$LIB_NAME"
    echo "Ready to be used in Godot project!"
else
    echo "Error: Built library not found at $TARGET_PATH"
    exit 1
fi
