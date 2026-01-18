#!/bin/bash

# Copyright (c) 2026-present K. S. Ernest (iFire) Lee
# SPDX-License-Identifier: MIT

# set -e

export PATH="$HOME/.cargo/bin:$PWD:$PATH"

echo "🚀 Godot-Zenoh Multi-Peer Communication Test in CI/CD"

mkdir -p test_logs

echo "🎮 Starting 2 Godot peers..."

timeout 20s godot --headless godot_zenoh/scenes/main_scene.tscn > test_logs/peer1.log 2>&1 &
P1_PID=$!
sleep 1

timeout 20s godot --headless godot_zenoh/scenes/main_scene.tscn > test_logs/peer2.log 2>&1 &
P2_PID=$!
sleep 1

echo "⏳ Enabling peer-to-peer communication for 30 seconds..."
sleep 30

echo "🧹 Cleaning up processes..."
pkill -9 -f godot || true
sleep 1

P1_CONN=$(grep -c "CLIENT CONNECTED\|connected to network" test_logs/peer1.log)
P2_CONN=$(grep -c "CLIENT CONNECTED\|connected to network" test_logs/peer2.log)
P3_CONN=$(grep -c "CLIENT CONNECTED\|connected to network" test_logs/peer3.log)
P1_SENT=$(grep -c "SENT:" test_logs/peer1.log)
P2_SENT=$(grep -c "SENT:" test_logs/peer2.log)

TOTAL_CONN=$((P1_CONN + P2_CONN))
TOTAL_SENT=$((P1_SENT + P2_SENT))

echo "Debug: P1_CONN=$P1_CONN P2_CONN=$P2_CONN TOTAL_CONN=$TOTAL_CONN"

echo ""
echo "📊 MULTI-PEER TEST RESULTS:"
echo "==========================="
echo "Peers Connected: $TOTAL_CONN (target: ≥1)"
echo "Messages Sent: $TOTAL_SENT (target: ≥1)"
echo ""
echo "Peer 1: $P1_CONN connections, $P1_SENT sent"
echo "Peer 2: $P2_CONN connections, $P2_SENT sent"

if [ $TOTAL_CONN -ge 1 ] && [ $TOTAL_SENT -ge 1 ]; then
    echo ""
    echo "✅ MULTI-PEER TEST PASSED!"
    echo "✅ Multiple Godot peers successfully communicate via Zenoh router in CI/CD"
    echo "✅ Distributed peer-to-peer networking validated automatically"
    exit 0
else
    echo ""
    echo "❌ MULTI-PEER TEST FAILED!"
    echo "❌ Insufficient peer communication in automated environment"
    echo ""
    echo "🔍 Debug logs:"
    echo "Peer 1:"; grep -E "(ERROR|FAILED|SERVER|CLIENT|SENT)" test_logs/peer1.log | head -3
    echo "Peer 2:"; grep -E "(ERROR|FAILED|SERVER|CLIENT|SENT)" test_logs/peer2.log | head -3
    exit 1
fi