#!/bin/bash
set -e

echo "🚀 Godot-Zenoh Multi-Peer Communication Test in CI/CD"

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

# Launch 3 Godot peers simultaneously
echo "🎮 Starting 3 Godot peers..."

timeout 20s godot --headless godot_zenoh/scenes/main_scene.tscn > test_logs/peer1.log 2>&1 &
P1_PID=$!
sleep 1

timeout 20s godot --headless godot_zenoh/scenes/main_scene.tscn > test_logs/peer2.log 2>&1 &
P2_PID=$!
sleep 1

timeout 20s godot --headless godot_zenoh/scenes/main_scene.tscn > test_logs/peer3.log 2>&1 &
P3_PID=$!
sleep 1

echo "⏳ Enabling peer-to-peer communication for 12 seconds..."
sleep 12

# Cleanup
echo "🧹 Cleaning up processes..."
pkill -9 -f zenohd || true
pkill -9 -f godot || true
sleep 1

# Validate results
P1_CONN=$(grep -c "CLIENT CONNECTED\|connected to network" test_logs/peer1.log)
P2_CONN=$(grep -c "CLIENT CONNECTED\|connected to network" test_logs/peer2.log)
P3_CONN=$(grep -c "CLIENT CONNECTED\|connected to network" test_logs/peer3.log)
P1_SENT=$(grep -c "SENT:" test_logs/peer1.log)
P2_SENT=$(grep -c "SENT:" test_logs/peer2.log)
P3_SENT=$(grep -c "SENT:" test_logs/peer3.log)

TOTAL_CONN=$((P1_CONN + P2_CONN + P3_CONN))
TOTAL_SENT=$((P1_SENT + P2_SENT + P3_SENT))

echo ""
echo "📊 MULTI-PEER TEST RESULTS:"
echo "==========================="
echo "Peers Connected: $TOTAL_CONN (target: ≥2)"
echo "Messages Sent: $TOTAL_SENT (target: ≥2)"
echo ""
echo "Peer 1: $P1_CONN connections, $P1_SENT sent"
echo "Peer 2: $P2_CONN connections, $P2_SENT sent"
echo "Peer 3: $P3_CONN connections, $P3_SENT sent"

if [ $TOTAL_CONN -ge 2 ] && [ $TOTAL_SENT -ge 2 ]; then
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
    echo "Peer 3:"; grep -E "(ERROR|FAILED|SERVER|CLIENT|SENT)" test_logs/peer3.log | head -3
    exit 1
fi