#!/usr/bin/env python3

"""
Simple Zenoh Python client to test connectivity with Godot-Zenoh server.
This will help diagnose if the issue is with Godot or with zenoh P2P networking.
"""

import argparse
import time
import zenoh

def main():
    parser = argparse.ArgumentParser(description="Zenoh Python client for testing Godot-Zenoh connectivity")
    parser.add_argument("--server", default="localhost:7447", help="Server peer address (default: localhost:7447)")
    parser.add_argument("--client-id", type=int, default=100, help="Client ID number (default: 100)")
    parser.add_argument("--game", default="pong_test", help="Game/topic prefix (default: pong_test)")
    args = parser.parse_args()

    print(f"🔌 Zenoh Python Client - Connecting to {args.server} as Player {args.client_id}")

    # Configure Zenoh connection to server peer
    config = zenoh.Config()
    config.insert_json5("connect/endpoints", f"['tcp/{args.server}']")

    try:
        # Open zenoh session
        print("🔧 Opening zenoh session...")
        session = zenoh.open(config)
        print(f"✅ Connected! ZID: {session.zid()}")

        # Define topic for our game/channel
        topic = f"{args.game}/channel000"  # Same as Godot client channel 0
        print(f"📡 Subscribing to topic: {topic}")

        # Subscribe to receive messages
        def on_sample(sample):
            payload = sample.payload.decode("utf-8")
            print(f"📥 Received: '{payload}' on topic '{sample.key_expr}'")

        session.declare_subscriber(topic, on_sample)
        print("🎧 Listening for messages...")

        # Send a test message
        test_msg = f"HELLO_FROM_PYTHON:{args.client_id}"
        session.put(topic, test_msg.encode())
        print(f"📤 Sent: '{test_msg}'")

        # Wait a bit to receive responses
        print("⏳ Waiting 5 seconds for responses...")
        time.sleep(5)

        # Clean up
        session.close()
        print("🔚 Session closed")

    except Exception as e:
        print(f"❌ Error: {e}")
        return 1

    return 0

if __name__ == "__main__":
    exit(main())
