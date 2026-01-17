#!/bin/bash

# Simple Godot-Zenoh Networking Test Script
echo "🎮 GODOT-ZENOH NETWORKING TEST SCRIPT"
echo "===================================="
echo

# Check if zenohd is running
ZENOHD_PID=$(ps aux | grep zenohd | grep -v grep | awk '{print $2}')
if [ -z "$ZENOHD_PID" ]; then
    echo "⚠️  zenohd not running"
    echo "Start router with: /usr/local/bin/zenohd --rest-http-port 8000"
    echo
    exit 1
else
    echo "✅ zenohd running (PID: $ZENOHD_PID)"
fi

# Check if Godot project exists
if [ ! -f "project.godot" ]; then
    echo "❌ Godot project files not found"
    exit 1
else
    echo "✅ Godot project found"
fi

# Check if GDExtension is built
if [ ! -f "addons/godot-zenoh/libgodot_zenoh.dylib" ]; then
    echo "❌ GDExtension not built"
    echo "Build with: ./build.sh"
    exit 1
else
    echo "✅ GDExtension built"
fi

echo
echo "🚀 READY TO TEST GODOT-ZENOH NETWORKING"
echo "========================================"
echo
echo "Run Godot with:"
echo "godot project.godot"
echo
echo "This will test:"
echo "• ❤️ GDExtension loading"
echo "• 🔧 Virtual channel configuration (0-255)"
echo "• 📦 Packet sending/reception"
echo "• 🛡️ HOL blocking prevention"
echo "• 🌐 Real zenoh network communication"
echo
echo "Expected output includes:"
echo "• 'ZenohMultiplayerPeer initialized'"
echo "• 'Priority channels: 0→255 packet ordering'"
echo "• '✅ Server created successfully'"
echo "• HOL blocking prevention demonstration"
