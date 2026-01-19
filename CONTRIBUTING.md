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

# Install pre-commit hooks for code quality (recommended)
pip install pre-commit
pre-commit install
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
│   ├── bin/
│   │   └── zenoh_cli_test.rs     # CLI testing tool for Mars HLC Counter test
│   ├── lib.rs                   # Main extension initialization
│   ├── peer.rs                  # ZenohMultiplayerPeer - Brutal flow implementation
│   └── networking.rs            # Zenoh session and packet routing
├── tests/                       # Integration tests
│   ├── integration.rs           # Full system integration tests
│   ├── networking_tests.rs      # Zenoh networking tests
│   ├── peer_tests.rs            # Peer implementation tests
│   └── peer_tests.proptest-regressions  # Property test regressions
├── sample/                      # Godot sample project
│   ├── project.godot
│   ├── gdextension.json
│   ├── godot-zenoh.gdextension
│   └── addons/godot-zenoh/
├── misc/                        # Development utilities and scripts
├── godot_zenoh/                 # Primary Godot project for multiplayer testing (godot --headless)
├── build.sh                     # Build script
├── Cargo.toml                   # Rust dependencies
├── LICENSE
├── CONTRIBUTING.md
└── README.md
```

## Networking Architecture

### Message Flow Routing

Messages are sent through the brutal flow for low-latency communication:

```rust
// Quick player movement - goes through brutal flow
player.move_to(position)  // -> Zenoh pub/sub direct messaging
```

### Network Isolation

Messages use the brutal flow key expression: `game/{game_id}/brutal/{channel_id}` -
Direct messaging

## Development Guidelines

### Code Standards

- **Professional Logging**: Use clean, emoji-free error messages and logging statements
- **Clean Compiler Output**: Maintain zero warning/code compilation with `cargo clippy`
  and cleaner tools
- **Rust Best Practices**: Follow idiomatic Rust patterns, proper error handling, and
  comprehensive documentation
- **Resource Management**: Properly manage struct fields, avoid unused
  imports/variables, and clean up dead code
- **Godot Integration**: Ensure compatibility with Godot's MultiplayerAPI and signal
  system
- **Async Safety**: Respect Zenoh's Send/Sync constraints using actor patterns
- **Memory Management**: Be mindful of Godot's garbage collection and reference counting

### Testing

#### CLI Testing Tool

The project includes a comprehensive CLI testing tool for benchmarking and validation:

```bash
# Build the CLI test tool
cargo build --bin zenoh_cli_test

# Run Mars HLC Counter Test (uses 3 clients internally)
cargo run --bin zenoh_cli_test -- mars 3 30

# Real Zenoh network testing
cargo run --bin zenoh_cli_test -- network publisher "Hello World"

# Multi-peer scaling benchmark
cargo run --bin zenoh_cli_test -- scale 5 10

# Start Zenoh router
cargo run --bin zenoh_cli_test -- start-router
```

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
   - Ensure all tests pass (`cargo test`)
   - Run pre-commit checks (`pre-commit run --all-files`)
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

### Mars HLC Counter Test

The project includes the Mars HLC Counter Test - a distributed consistency test that
validates Hybrid Logical Clock-based total ordering for concurrent operations across
multiple clients. This test demonstrates how Zenoh + HLC enables strong consistency in
distributed systems, ensuring all concurrent increments are applied in a globally
consistent order.

**Features:**

- Hybrid Logical Clock (HLC) implementation for distributed timestamping
- Concurrent client simulation with configurable duration
- Global ordering verification using HLC total order
- Distributed counter invariant checking (final value = sum of increments)
- Jepsen-style consistency validation

**Run the test:**

```bash
cargo run --bin zenoh_cli_test -- mars <any_number> <seconds>
# The test uses 3 clients internally, ignores the first argument
# Example: cargo run --bin zenoh_cli_test -- mars 3 30
```

**Test Validation:**

- 3 concurrent clients each perform 5 increment operations
- Backend collects all operations and sorts by HLC timestamp
- Final counter must equal 15 (3 * 5) for test success
- Demonstrates causal consistency through timestamp ordering

### Areas for Contribution

- Performance optimizations
- Additional networking features
- Better error handling
- Documentation improvements
- Cross-platform testing
- Godot editor integration
- Mars Extreme test enhancements
- Advanced scaling algorithms

## License

This project is licensed under the MIT License.
