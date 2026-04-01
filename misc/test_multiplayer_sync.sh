#!/bin/bash
# Copyright (c) 2026-present K. S. Ernest (iFire) Lee
# SPDX-License-Identifier: MIT
#
# Runs 1 server + 2 clients to test Godot's MultiplayerSynchronizer and @rpc
# through ZenohMultiplayerPeer.
#
# Exit 0 = all peers passed, 1 = at least one failed.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"
SAMPLE_PATH="$REPO_ROOT/sample"
LOG_DIR="$REPO_ROOT/test_logs/sync"
PORT=7449

mkdir -p "$LOG_DIR"

# ---- Locate godot binary ------------------------------------------------
if command -v godot &>/dev/null; then
    GODOT=godot
elif [ -x "/Applications/Godot.app/Contents/MacOS/Godot" ]; then
    GODOT="/Applications/Godot.app/Contents/MacOS/Godot"
elif [ -x "/usr/local/bin/godot" ]; then
    GODOT="/usr/local/bin/godot"
else
    echo "SKIP: godot not found"
    exit 0
fi
echo "Using Godot: $($GODOT --version 2>/dev/null | head -1)"

# ---- Build dylib if needed (Linux / macOS) ------------------------------
ADDONS_DIR="$SAMPLE_PATH/addons/godot-zenoh"
mkdir -p "$ADDONS_DIR"

if [ "$(uname)" = "Darwin" ]; then
    LIB="$REPO_ROOT/target/debug/libgodot_zenoh.dylib"
    DEST="$ADDONS_DIR/libgodot_zenoh.dylib"
else
    LIB="$REPO_ROOT/target/debug/libgodot_zenoh.so"
    DEST="$ADDONS_DIR/libgodot_zenoh.so"
fi

if [ ! -f "$LIB" ] || [ "$REPO_ROOT/src/peer.rs" -nt "$LIB" ] || \
   [ "$REPO_ROOT/src/networking.rs" -nt "$LIB" ]; then
    echo "Building extension..."
    (cd "$REPO_ROOT" && cargo build 2>&1)
fi
cp "$LIB" "$DEST"

# ---- Import project (first run only) ------------------------------------
echo "Importing project..."
"$GODOT" --headless --path "$SAMPLE_PATH" --import --quit 2>/dev/null || true
sleep 1

# ---- Override main scene to sync_test.gd --------------------------------
# We launch with a custom scene path rather than changing project.godot.
SYNC_SCENE="res://godot_zenoh/scenes/sync_test_scene.tscn"

# Write a minimal .tscn that loads sync_test.gd
cat > "$SAMPLE_PATH/godot_zenoh/scenes/sync_test_scene.tscn" << 'TSCN'
[gd_scene format=3]

[ext_resource type="Script" path="res://godot_zenoh/core/sync_test.gd" id="1"]

[node name="SyncTest" type="Node"]
script = ExtResource("1")
TSCN

GODOT_ARGS="--headless --path $SAMPLE_PATH godot_zenoh/scenes/sync_test_scene.tscn"

# ---- Launch server -------------------------------------------------------
echo "Starting SERVER on port $PORT..."
ZENOH_ROLE=server ZENOH_PORT=$PORT \
    timeout 20s "$GODOT" $GODOT_ARGS >"$LOG_DIR/server.log" 2>&1 &
SERVER_PID=$!
sleep 3   # let server bind and Zenoh session start

# ---- Launch 2 clients ----------------------------------------------------
echo "Starting CLIENT 1..."
ZENOH_ROLE=client ZENOH_PORT=$PORT \
    timeout 20s "$GODOT" $GODOT_ARGS >"$LOG_DIR/client1.log" 2>&1 &
CLIENT1_PID=$!
sleep 0.5

echo "Starting CLIENT 2..."
ZENOH_ROLE=client ZENOH_PORT=$PORT \
    timeout 20s "$GODOT" $GODOT_ARGS >"$LOG_DIR/client2.log" 2>&1 &
CLIENT2_PID=$!

# ---- Wait for all processes ---------------------------------------------
echo "Waiting for peers to finish (up to 15s)..."
wait $SERVER_PID  || SERVER_EXIT=$?
wait $CLIENT1_PID || CLIENT1_EXIT=$?
wait $CLIENT2_PID || CLIENT2_EXIT=$?

SERVER_EXIT=${SERVER_EXIT:-0}
CLIENT1_EXIT=${CLIENT1_EXIT:-0}
CLIENT2_EXIT=${CLIENT2_EXIT:-0}

# ---- Results ------------------------------------------------------------
echo ""
echo "=== SERVER log ==="
cat "$LOG_DIR/server.log"
echo ""
echo "=== CLIENT 1 log ==="
cat "$LOG_DIR/client1.log"
echo ""
echo "=== CLIENT 2 log ==="
cat "$LOG_DIR/client2.log"
echo ""

PASS=true
for role_exit in "SERVER:$SERVER_EXIT" "CLIENT1:$CLIENT1_EXIT" "CLIENT2:$CLIENT2_EXIT"; do
    name="${role_exit%%:*}"
    code="${role_exit##*:}"
    if [ "$code" -ne 0 ]; then
        echo "FAIL: $name exited with code $code"
        PASS=false
    fi
done

# Also grep logs for explicit PASS/FAIL lines
for log_file in server client1 client2; do
    if grep -q "FAIL" "$LOG_DIR/${log_file}.log" 2>/dev/null; then
        echo "FAIL: $log_file log contains FAIL"
        PASS=false
    fi
done

if $PASS; then
    echo "PASS: MultiplayerSynchronizer + RPC test passed for 1 server + 2 clients"
    exit 0
else
    echo "FAIL: one or more peers failed"
    exit 1
fi
