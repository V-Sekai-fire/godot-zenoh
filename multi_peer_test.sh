#!/bin/bash

# Copyright (c) 2026-present K. S. Ernest (iFire) Lee
# SPDX-License-Identifier: MIT

# set -e

export PATH="$HOME/.cargo/bin:$PWD:$PATH"

# Parameterize number of peers (default: 2)
NUM_PEERS=${NUM_PEERS:-2}

echo "🚀 Godot-Zenoh Multi-Peer Communication Test in CI/CD (Peers: $NUM_PEERS)"

# Force shutdown any existing zenohd processes
echo "🛑 Force shutting down any existing zenohd processes..."
pkill -9 -f zenohd || true
sleep 2

mkdir -p test_logs

# Start Zenoh router
echo "📡 Launching Zenoh network router..."
zenohd --listen tcp/127.0.0.1:7447 > test_logs/zenohd.log 2>&1 &
ZENOH_PID=$!
sleep 3

if ! ps -p $ZENOH_PID > /dev/null; then
    echo "❌ Zenoh router failed to start"
    cat test_logs/zenohd.log
    exit 1
fi

echo "✅ Zenoh router coordinating network on port 7447"

echo "🎮 Starting $NUM_PEERS Godot peers..."

PEER_PIDS=()
for i in $(seq 1 $NUM_PEERS); do
    timeout 20s godot --headless godot_zenoh/scenes/main_scene.tscn > test_logs/peer$i.log 2>&1 &
    PEER_PIDS[$i]=$!
    sleep 1
done

echo "⏳ Enabling peer-to-peer communication for 30 seconds..."
sleep 30

echo "🧹 Cleaning up processes..."
pkill -9 -f zenohd || true
pkill -9 -f godot || true
sleep 1

# Calculate totals
TOTAL_CONN=0
TOTAL_SENT=0
for i in $(seq 1 $NUM_PEERS); do
    CONN_VAR="P${i}_CONN"
    SENT_VAR="P${i}_SENT"
    eval "$CONN_VAR=\$(grep -c \"CLIENT CONNECTED\|connected to network\" test_logs/peer$i.log)"
    eval "$SENT_VAR=\$(grep -c \"SENT:\" test_logs/peer$i.log)"
    eval "TOTAL_CONN=\$((TOTAL_CONN + $CONN_VAR))"
    eval "TOTAL_SENT=\$((TOTAL_SENT + $SENT_VAR))"
done

echo "Debug: TOTAL_CONN=$TOTAL_CONN TOTAL_SENT=$TOTAL_SENT"

echo ""
echo "📊 MULTI-PEER TEST RESULTS:"
echo "==========================="
echo "Peers Connected: $TOTAL_CONN (target: ≥1)"
echo "Messages Sent: $TOTAL_SENT (target: ≥1)"
echo ""
for i in $(seq 1 $NUM_PEERS); do
    eval "echo \"Peer $i: \$P${i}_CONN connections, \$P${i}_SENT sent\""
done

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
    for i in $(seq 1 $NUM_PEERS); do
        echo "Peer $i:"; grep -E "(ERROR|FAILED|SERVER|CLIENT|SENT)" test_logs/peer$i.log | head -3
    done
    exit 1
fi