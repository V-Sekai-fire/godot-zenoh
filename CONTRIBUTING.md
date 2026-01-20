# Contributing to Godot Zenoh Multiplayer Extension

Thank you for your interest in contributing to the Godot Zenoh Multiplayer Extension!
This GDExtension provides a Zenoh-based networking backend for Godot's multiplayer
system, offering low-latency pub/sub communication with Head-of-Line (HOL) blocking
prevention.

## Project Overview

This extension implements fast "brutal flow" messaging using Zenoh as the transport
layer for Godot's multiplayer system, offering low-latency pub/sub communication with
Head-of-Line (HOL) blocking prevention.

### Architecture: "Brutal Flow" - Direct/Fast Path

**Purpose**: Real-time gaming communication

- **Speed**: Priority on latency via direct Zenoh pub/sub messaging
- **Reliability**: Best-effort delivery for performances
- **Use Cases**: Player movement, shooting, chat, physics sync
- **Implementation**: `ZenohMultiplayerPeer` with HOL blocking prevention
- **Quality**: Low-latency (\<10ms typical), high-throughput message passing

### Technical Features

- **Async networking** with Tokio runtime
- **HOL blocking prevention** using 256 virtual channels
- **Automatic synchronization** support
- **Thread-safe actor pattern** for Godot integration
- **Comprehensive debug logging** and performance monitoring

## Development Setup

### Prerequisites

- **Rust**: 1.70+ with Cargo
- **Godot**: 4.3+ (for testing the extension)
- **Zenoh Router**: For running multiplayer tests
- **Python 3**: For running test clients

### Clone and Setup

```bash
git clone <repository-url>
cd godot-zenoh
```

### Build the Extension

```bash
# Debug build (for development)
cargo build

# Release build (for production/testing)
cargo build --release
./build.sh

# Copy DLL to sample project for testing
cp target/release/godot_zenoh.dll sample/
```

This will compile the Rust code into a GDExtension library that Godot can load.

#### Testing Pub-Sub Behavior

To verify the pub-sub fanout works correctly:

```bash
# Run the comprehensive networking test
cd sample
godot --headless project.godot
```

The test creates 1 server + 10 clients and verifies messages are properly distributed
via Zenoh pub-sub. Success indicators:

- \[ok\] All clients connect successfully
- \[ok\] All clients send packets successfully
- \[success\] "Multi-client networking stack test PASSED!"

## Project Structure

```
godot-zenoh/
├── src/
│   ├── lib.rs                   # Main extension initialization
│   ├── peer.rs                  # ZenohMultiplayerPeer - Brutal flow implementation
│   └── networking.rs            # Zenoh session and packet routing
├── tests/                       # Integration tests
│   ├── integration.rs           # Full system integration tests
│   ├── networking_tests.rs      # Zenoh networking tests
│   ├── peer_tests.rs            # Peer implementation tests
│   └── peer_tests.proptest-regressions  # Property test regressions
├── sample/                       # Godot test project
│   ├── godot_zenoh/              # Test game code
│   │   ├── core/
│   │   │   ├── connection_genserver.gd
│   │   │   ├── election_genserver.gd
│   │   │   ├── game_genserver.gd
│   │   │   └── pong_test.gd
│   │   ├── scenes/
│   │   │   ├── main_scene.tscn
│   │   │   ├── test_scene.tscn     # Pub-sub networking tests
│   │   │   └── pong_test.tscn
│   │   ├── test_zenoh_networking.gd  # Comprehensive networking tests
│   │   └── scenes.tscn
│   ├── godot-zenoh.dll           # Compiled extension
│   ├── godot-zenoh.gdextension   # Extension configuration
│   └── project.godot             # Godot project file
├── build.sh                     # Build script
├── Cargo.toml                   # Rust dependencies
├── gdextension.json             # Godot extension configuration
├── project.godot                # Godot project file
└── CONTRIBUTING.md
```

## Networking Architecture

### Pub-Sub Message Flow

This extension implements a **proper pub-sub architecture** where:

1. **Publishers** send messages to topic-based channels
1. **Subscribers** receive messages from the same channels
1. **Fanout behavior** ensures messages sent by any peer are received by all other
   connected peers

#### Message Flow Example:

```rust
// Peer A sends message -> Zenoh pub/sub -> Peers B, C, D all receive the message
client_a.send_packet(data, channel: 0)  // -> zenoh/keyexpr("godot/game/{game_id}/channel000")
// Peers B, C, D all receive via their subscribers on the same channel
```

#### Recent Fixes

**Pub-Sub Fanout Fix**: Previously, the implementation had **isolated peer-to-peer
behavior** where peers could only send but never receive messages. This has been fixed
to restore **proper server-to-client fanout**:

- Added `Subscriber<FifoChannelHandler<Sample>>` to `ZenohSession`
- Implemented packet polling with `poll_packets()` method
- Added peer ID headers for message attribution
- Created comprehensive testing for pub-sub behavior

### Channel-Based Routing

Messages use topic-based routing: `godot/game/{game_id}/channel{channel_id:03d}`

- **Channel 0**: Default reliable channel
- **Channels 1-255**: HOL-blocking prevention virtual channels
- **Isolation**: Each game has its own topic namespace

## Development Guidelines

### Code Standards

- **Professional Logging**: Use clean, emoji-free error messages and logging statements
- **Rust Best Practices**: Follow idiomatic Rust patterns, proper error handling, and
  comprehensive documentation
- **Godot Integration**: Ensure compatibility with Godot's MultiplayerAPI and signal
  system
- **Async Safety**: Respect Zenoh's Send/Sync constraints using actor patterns
- **Memory Management**: Be mindful of Godot's garbage collection and reference counting

### Testing

#### Unit Tests

```bash
cargo test
```

#### Integration Tests

```bash
# Start Zenoh router
./bin/zenohd &

# Run Godot tests
godot --headless --script test_class_registration.gd

# Run Python client tests
python3 test_python_client.py --client-id 1 --game pong_test
python3 test_python_client.py --client-id 2 --game pong_test
```

#### Multiplayer Testing

```bash
# Terminal 1: Start server
godot --headless main_scene.tscn --server

# Terminal 2: Start client
godot --headless main_scene.tscn --client
```

### Git Commit Guidelines

This project does not use conventional commits. Use clear, descriptive commit messages
that explain:

- **What** was changed
- **Why** the change was necessary
- **How** the change addresses the issue

Example:

```
Fix race condition in message routing logic

Implement proper message ordering in Zenoh networking layer to prevent
concurrent access issues. Add mutex guards around shared packet queues
and validate ordering constraints in property-based tests.
```

### Pull Request Process

1. **Fork and Branch**: Create a feature branch from `main`
1. **Implement Changes**: Add tests for new functionality
1. **Code Quality**:
   - Run `cargo fmt` and `cargo clippy`
   - Ensure all tests pass
   - Update documentation
1. **Commit**: Use clear, descriptive commit messages as documented above
1. **PR Description**: Include what was changed and why

### Reporting Issues

Please include:

- Godot version and platform
- Rust version (`rustc --version`)
- Zenoh router version
- Steps to reproduce
- Full error logs (with debug logging enabled)
- Expected vs actual behavior

### Areas for Contribution

- Performance optimizations
- Additional networking features
- Better error handling
- Documentation improvements
- Cross-platform testing
- Godot editor integration

## License

This project is licensed under the MIT License.
