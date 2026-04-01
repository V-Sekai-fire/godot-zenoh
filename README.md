# godot-zenoh

A [Zenoh](https://zenoh.io)-based `MultiplayerPeer` GDExtension for Godot 4.6+.
Drop it in and Godot's built-in `@rpc` calls and `MultiplayerSynchronizer` nodes
route their packets through Zenoh pub/sub instead of ENet or WebRTC.

## Features

- Full `MultiplayerPeerExtension` integration — `@rpc`, `MultiplayerSynchronizer`,
  and `MultiplayerSpawner` all work out of the box
- Async Zenoh session on a dedicated Tokio thread; never blocks the main thread
- 256 virtual channels for Head-of-Line blocking prevention
- Automatic peer discovery via a dedicated beacon channel
- Self-echo filtering (Zenoh reflects published messages to local subscribers)

## Quick start

### Prerequisites

| Tool | Version |
|------|---------|
| Rust | stable (1.70+) |
| Godot | 4.6+ |

### Build

```bash
cargo build                    # debug
cargo build --release          # release
```

Copy the output library into the sample project's addons directory:

```bash
# macOS
cp target/debug/libgodot_zenoh.dylib sample/addons/godot-zenoh/

# Linux
cp target/debug/libgodot_zenoh.so sample/addons/godot-zenoh/
```

On first run, let Godot import the project (needed once to register the extension):

```bash
godot --headless --path sample --import --quit
```

### Run the sample

```bash
godot --headless --path sample --quit
```

Expected output includes `CLIENT: connected` and `CLIENT CONNECTED`.

## Usage in GDScript

```gdscript
var peer := ZenohMultiplayerPeer.new()
peer.game_id = "my_game"

# --- Server ---
peer.create_server(7447, 32)
multiplayer.multiplayer_peer = peer
multiplayer.peer_connected.connect(func(id): print("client joined: ", id))

# --- Client ---
peer.create_client("127.0.0.1", 7447)
multiplayer.multiplayer_peer = peer
multiplayer.connected_to_server.connect(func(): print("connected"))

# Poll every frame (ZenohMultiplayerPeer is also polled internally by
# Godot's SceneMultiplayer, but an explicit call ensures timely delivery)
func _process(_delta):
    peer.poll()
```

### RPC example

```gdscript
@rpc("authority", "call_remote")
func sync_state(value: int) -> void:
    print("received: ", value)

# server-side
sync_state.rpc(42)              # broadcast to all peers
rpc_id(client_id, "sync_state", 42)  # targeted
```

### MultiplayerSynchronizer example

```gdscript
@export var position: Vector2 = Vector2.ZERO   # @export required

func _ready():
    var cfg := SceneReplicationConfig.new()
    cfg.add_property(NodePath(".:position"))

    var sync := MultiplayerSynchronizer.new()
    sync.root_path = NodePath("..")
    sync.replication_config = cfg
    add_child(sync)
```

## Testing

### Rust unit + integration tests

```bash
cargo test
cargo test --test integration   # 3-peer packet-delivery test
```

### End-to-end GDScript test (1 server + 2 clients)

```bash
bash misc/test_multiplayer_sync.sh
```

This headlessly runs one server and two clients, verifies that `@rpc` values
and `MultiplayerSynchronizer` property updates arrive at both clients, and
exits 0 on success.

## Project structure

```
godot-zenoh/
├── src/
│   ├── lib.rs            GDExtension entry point
│   ├── peer.rs           ZenohMultiplayerPeer — MultiplayerPeerExtension impl
│   └── networking.rs     ZenohSession — async Zenoh pub/sub layer
│   └── bin/
│       └── server_test.rs  standalone server smoke-test binary
├── tests/
│   ├── integration.rs    3-peer Rust integration tests
│   └── peer_tests.rs     peer unit tests
├── sample/               Godot 4.6 sample project
│   ├── godot-zenoh.gdextension
│   ├── project.godot
│   └── godot_zenoh/
│       ├── core/
│       │   ├── pong_test.gd      basic pub/sub demo
│       │   └── sync_test.gd      RPC + MultiplayerSynchronizer test
│       └── scenes/
│           ├── main_scene.tscn
│           └── sync_test_scene.tscn
├── misc/
│   ├── build.sh                  cross-platform build helper
│   └── test_multiplayer_sync.sh  end-to-end headless test
├── Cargo.toml
├── Cargo.lock
└── gdextension.json
```

## Architecture

```
GDScript / Godot MultiplayerAPI
        │  put_packet_script / get_packet_script
        ▼
ZenohMultiplayerPeer  (Godot main thread)
  – packet_queue (Vec)
  – known_peers  (HashSet)
  – emits: connection_succeeded, peer_connected
        │  mpsc channels
        ▼
ZenohAsyncBridge  (worker thread + Tokio runtime)
        │
        ▼
ZenohSession
  – publishers:  godot/game/{id}/channel{N}
  – subscribers: godot/game/{id}/channel{N}  (callback → mpsc)
  – discovery:   godot/game/{id}/discovery   (beacon on connect)
```

Packets are framed with an 8-byte little-endian `peer_id` header.
Self-echoed messages (Zenoh delivers to local subscribers) are filtered
by comparing the header with `self.peer_id`.

## Known limitations

- **Broadcast topology**: Zenoh pub/sub delivers every packet to every
  subscriber. Godot's MultiplayerAPI handles per-peer filtering via the
  destination encoded in the packet payload.
- **`!connected_peers.has(sender)` warnings**: Godot logs this when a
  packet arrives from a peer whose `peer_connected` signal hasn't been
  processed yet. The packet is still handled correctly; it is a cosmetic
  warning from the timing of signal dispatch.
- **No `MultiplayerSpawner` tested yet**: `@rpc` and `MultiplayerSynchronizer`
  are verified; spawner support is untested.

## License

MIT — see [LICENSE](LICENSE).
