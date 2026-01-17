# Contributing to Godot Zenoh Multiplayer Extension

Thank you for your interest in contributing to the Godot Zenoh Multiplayer Extension! This GDExtension provides a Zenoh-based networking backend for Godot's multiplayer system, offering low-latency pub/sub communication with Head-of-Line (HOL) blocking prevention.

## Project Overview

This extension implements dual networking flows for Godot multiplayer: **fast "brutal flow" messaging** and **consensus-based Raft transactions**, both using Zenoh as the transport layer.

### Architecture: Dual Networking Flows

#### Flow 1: Standard "Brutal Flow" - Direct/Fast Path
**Purpose**: Real-time gaming communication
- **Speed**: Priority on latency via direct Zenoh pub/sub messaging
- **Reliability**: Best-effort delivery for performances
- **Use Cases**: Player movement, shooting, chat, physics sync
- **Implementation**: `ZenohMultiplayerPeer` with HOL blocking prevention
- **Quality**: Low-latency (<10ms typical), high-throughput message passing

#### Flow 2: "Transaction with Raft Flow" - Consensus Path
**Purpose**: Critical state changes requiring distributed consensus
- **Consistency**: Raft protocol guarantees via majority quorum voting
- **Reliability**: ACID-like consistency with distributed log replication
- **Use Cases**: Leader election, item transactions, match state, game rules
- **Implementation**: `ZenohRaftConsensus` with async-raft library v0.6.1
- **Quality**: High consistency, fault-tolerant, distributed consensus

### Technical Features

- **Async networking** with Tokio runtime
- **HOL blocking prevention** using 256 virtual channels
- **Automatic synchronization** support
- **Thread-safe actor pattern** for Godot integration
- **Raft consensus protocol** implementation
- **Real distributed consensus** using async-raft library
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
./build.sh
```

This will compile the Rust code into a GDExtension library that Godot can load.

## Project Structure

```
godot-zenoh/
├── src/
│   ├── lib.rs                   # Main extension initialization
│   ├── peer.rs                  # ZenohMultiplayerPeer - Brutal flow implementation
│   ├── networking.rs            # Zenoh session and packet routing
│   └── raft_consensus.rs        # ZenohRaftConsensus - Raft flow implementation
├── tests/                       # Integration tests
│   ├── networking_tests.rs      # Zenoh networking tests
│   ├── raft_consensus_tests.rs  # Raft consensus algorithm tests
│   └── integration.rs           # Full system integration tests
├── bin/                         # Zenoh router binary
├── addons/godot-zenoh/          # Godot addon files
├── demo_*.gd                    # Godot demo scripts
├── test_*.py                    # Python test clients
├── pong_test.gd                 # Dual flow demonstration scene
└── *.tscn                       # Godot scenes for testing
```

## Networking Architecture

### Message Flow Routing

The extension automatically routes messages based on requirements:

```rust
// Quick player movement - goes through brutal flow
player.move_to(position)  // -> Zenoh pub/sub direct messaging

// Critical transaction - requires consensus
inventory.transfer(item)  // -> Raft consensus + log replication

// Game state change - uses Raft for consistency
game.change_phase(new_phase)  // -> Distributed consensus protocol
```

### Network Isolation

Both flows run over Zenoh but use separate key expressions:
- **Brutal Flow**: `game/{game_id}/brutal/{channel_id}` - Direct messaging
- **Raft Flow**: `game/{game_id}/raft/{node_id}` - Consensus messaging

This allows independent scaling of real-time vs consensus traffic.

## Development Guidelines

### Code Standards

- **Professional Logging**: Use clean, emoji-free error messages and logging statements
- **Rust Best Practices**: Follow idiomatic Rust patterns, proper error handling, and comprehensive documentation
- **Godot Integration**: Ensure compatibility with Godot's MultiplayerAPI and signal system
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

This project does not use conventional commits. Use clear, descriptive commit messages that explain:

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
2. **Implement Changes**: Add tests for new functionality
3. **Code Quality**:
   - Run `cargo fmt` and `cargo clippy`
   - Ensure all tests pass
   - Update documentation
4. **Commit**: Use clear, descriptive commit messages as documented above
5. **PR Description**: Include what was changed and why

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
