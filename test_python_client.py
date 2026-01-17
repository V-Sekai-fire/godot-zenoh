#!/usr/bin/env python3

"""
Zenoh Python client that mirrors Godot-Zenoh multiplayer peer behavior.
Implements the same sync protocol using channels 254/255 for late joiner synchronization.
"""

import argparse
import time
import json
try:
    import zenoh
except ImportError:
    import eclipse_zenoh as zenoh

class ZenohPythonClient:
    def __init__(self, server_addr, client_id, game_id):
        self.server_addr = server_addr
        self.client_id = client_id
        self.game_id = game_id
        self.session = None
        self.is_server = (client_id == 1)  # Client ID 1 is server
        self.sync_data = None

    def connect(self):
        """Connect to Zenoh network"""
        print(f"🔌 Connecting to {self.server_addr} as {'Server' if self.is_server else 'Client'} {self.client_id}")

        config = zenoh.Config()
        config.insert_json5("connect/endpoints", f"['tcp/{self.server_addr}']")

        try:
            self.session = zenoh.open(config)
            print(f"✅ Connected! ZID: {self.session.zid()}")
            return True
        except Exception as e:
            print(f"❌ Connection failed: {e}")
            return False

    def setup_channels(self):
        """Set up subscriptions for all channels (0-255)"""
        print("📡 Setting up channel subscriptions...")

        self.subscribers = []

        def create_handler(channel):
            def on_sample(sample):
                try:
                    # Handle different payload types
                    if hasattr(sample.payload, 'decode'):
                        payload = sample.payload.decode("utf-8")
                        self.handle_packet(channel, payload, sample.payload)
                    else:
                        # ZBytes or other object
                        payload_bytes = bytes(sample.payload)
                        try:
                            payload = payload_bytes.decode("utf-8")
                            self.handle_packet(channel, payload, payload_bytes)
                        except UnicodeDecodeError:
                            # Binary data
                            self.handle_packet(channel, None, payload_bytes)
                except Exception as e:
                    print(f"Error handling packet on channel {channel}: {e}")
            return on_sample

        # Subscribe to all channels individually (0-255)
        for channel in range(256):
            topic = self._channel_to_topic(channel)
            handler = create_handler(channel)
            subscriber = self.session.declare_subscriber(topic, handler)
            self.subscribers.append(subscriber)

        print(f"🎧 Subscribed to all 256 channels (0-255)")

    def handle_packet(self, channel, text_payload, binary_payload):
        """Handle incoming packets on different channels"""
        if channel == 254:
            # Sync request from client
            if self.is_server:
                print(f"📨 Received sync request on channel 254 from client: '{text_payload}'")
                self.send_sync_response()
            return  # Don't print sync requests

        elif channel == 255:
            # Sync data from server - store it and handle
            if self.is_server:
                # Server receiving its own broadcast - just store it
                try:
                    if text_payload:
                        self.sync_data = json.loads(text_payload)
                    print(f"📥 Server stored sync data: {self.sync_data}")
                except:
                    print("⚠️ Server failed to parse sync data")
            else:
                # Client receiving sync data from server
                print("📨 Received sync data on channel 255")
                try:
                    if text_payload:
                        sync_data = json.loads(text_payload)
                    else:
                        # Handle binary sync data
                        sync_data = {"binary_size": len(binary_payload)}
                    print(f"✅ Sync data: {sync_data}")
                    self.sync_data = sync_data
                except json.JSONDecodeError:
                    print("⚠️ Failed to decode sync data")
            return

        # Regular channels
        if text_payload:
            print(f"📥 Channel {channel}: '{text_payload}'")
        else:
            print(f"📥 Channel {channel}: {len(binary_payload)} bytes binary data")

    def send_packet(self, channel, data):
        """Send packet on specific channel"""
        topic = self._channel_to_topic(channel)
        if isinstance(data, str):
            self.session.put(topic, data.encode())
            print(f"📤 Channel {channel}: '{data}'")
        else:
            # Binary data
            self.session.put(topic, data)
            print(f"📤 Channel {channel}: {len(data)} bytes binary data")

    def send_sync_response(self):
        """Server: respond to sync request with current sync data"""
        if self.sync_data:
            sync_json = json.dumps(self.sync_data)
            self.send_packet(255, sync_json)
            print("📤 Sent sync response on channel 255")
        else:
            print("⚠️ No sync data available to send")

    def request_sync(self):
        """Client: request sync from server"""
        self.send_packet(254, "SYNC_REQUEST")
        print("📡 Requested sync from server on channel 254")

    def broadcast_sync_data(self, sync_data):
        """Server: broadcast sync data to all clients"""
        self.sync_data = sync_data
        sync_json = json.dumps(sync_data)
        self.send_packet(255, sync_json)
        print("📢 Broadcasted sync data on channel 255")

    def _channel_to_topic(self, channel):
        """Convert channel number to Zenoh topic"""
        return f"{self.game_id}/channel{channel:03d}"

    def _topic_to_channel(self, topic):
        """Convert Zenoh topic to channel number"""
        # Extract channel number from topic like "game/channel042"
        parts = topic.split('/')
        if len(parts) >= 2 and parts[1].startswith('channel'):
            return int(parts[1][7:])  # Remove 'channel' prefix
        return 0

    def run_test(self):
        """Run the test scenario"""
        print(f"\n🎮 Starting {'Server' if self.is_server else 'Client'} test...")

        if self.is_server:
            # Server test: broadcast sync data periodically
            test_data = {
                "score": 100,
                "players": 2,
                "timestamp": time.time(),
                "server_id": self.client_id
            }
            self.broadcast_sync_data(test_data)

            # Wait for client requests
            print("⏳ Server waiting for client sync requests...")
            time.sleep(10)

        else:
            # Client test: request sync after a delay
            time.sleep(2)
            self.request_sync()

            # Wait for response
            print("⏳ Client waiting for sync response...")
            time.sleep(8)

        print("🏁 Test completed")

def main():
    parser = argparse.ArgumentParser(description="Zenoh Python client mirroring Godot behavior")
    parser.add_argument("--server", default="localhost:7447", help="Server peer address")
    parser.add_argument("--client-id", type=int, default=100, help="Client ID (1 = server)")
    parser.add_argument("--game", default="sync_test", help="Game/topic prefix")
    args = parser.parse_args()

    client = ZenohPythonClient(args.server, args.client_id, args.game)

    if not client.connect():
        return 1

    client.setup_channels()
    client.run_test()

    client.session.close()
    print("🔚 Session closed")
    return 0

if __name__ == "__main__":
    exit(main())
