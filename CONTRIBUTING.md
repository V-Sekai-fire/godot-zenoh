# Contributing to godot-zenoh

## Development setup

### Prerequisites

| Tool | Version |
|------|---------|
| Rust | stable (1.70+) |
| Godot | 4.6+ (for running sample/tests) |

### Build

```bash
cargo build
```

Copy the output library into the sample project before testing with Godot:

```bash
# macOS
cp target/debug/libgodot_zenoh.dylib sample/addons/godot-zenoh/

# Linux
cp target/debug/libgodot_zenoh.so sample/addons/godot-zenoh/
```

Run `godot --headless --path sample --import --quit` once after a fresh clone to
let Godot register the extension.

## Running tests

### Rust tests

```bash
cargo test                     # unit + integration
cargo test --test integration  # 3-peer packet-delivery test only
```

### End-to-end GDScript test

Requires Godot 4.6+ on `PATH` (or at `/Applications/Godot.app` on macOS):

```bash
bash misc/test_multiplayer_sync.sh
```

Launches 1 headless server and 2 headless clients, verifies `@rpc` delivery
and `MultiplayerSynchronizer` replication, exits 0 on success.

## Code standards

- Run `cargo fmt` before committing.
- Run `cargo clippy` and resolve all warnings.
- Follow idiomatic Rust error handling — no `unwrap()` in production paths.
- Use `godot_print!` / `godot_error!` for Godot-visible log output.
- Do not use the `uhlc` crate (see `Cargo.toml` comment).

## Project structure

See [README.md](README.md) for the full layout and architecture diagram.

Key files:

| File | Purpose |
|------|---------|
| `src/peer.rs` | `ZenohMultiplayerPeer` — `IMultiplayerPeerExtension` impl, signal emission, packet queue |
| `src/networking.rs` | `ZenohSession` — async Zenoh pub/sub, discovery beacon, peer-ID derivation |
| `tests/integration.rs` | 3-peer Rust integration test |
| `sample/godot_zenoh/core/sync_test.gd` | GDScript RPC + MultiplayerSynchronizer test |
| `misc/test_multiplayer_sync.sh` | Shell script that runs the GDScript test headlessly |

## Networking architecture

Packets travel on Zenoh topics:

```
godot/game/{game_id}/channel{NNN}   # game data (channels 0–255)
godot/game/{game_id}/discovery       # peer-announce beacons (no payload beyond peer_id header)
```

Every packet is prefixed with an 8-byte little-endian `peer_id` so receivers
can attribute and filter messages. Self-echoed messages are dropped in
`ZenohSession::drain_packets`.

When a client connects it publishes a zero-payload beacon on the discovery
topic so the server emits `peer_connected` before sending its first RPC.

## Pull request process

1. Branch from `main`.
2. Make changes with tests.
3. `cargo fmt && cargo clippy` — no warnings.
4. `cargo test` — all pass.
5. `bash misc/test_multiplayer_sync.sh` — exits 0.
6. Open PR with a description covering what changed and why.

## Commit messages

No conventional-commit prefix required. Write what changed and why:

```
Fix RPC routing when server has no known peers yet

Previously rpc_ping.rpc() sent to nobody because the server hadn't
received any packets from clients at send time. Discovery beacons now
let the server emit peer_connected before sending its first RPC.
```

## Reporting issues

Please include:

- Platform and OS version
- Godot version (`godot --version`)
- Rust version (`rustc --version`)
- Steps to reproduce
- Full log output

## License

MIT — see [LICENSE](LICENSE).
