# Contributing to Godot Zenoh GDExtension

Thank you for your interest in contributing to the Godot Zenoh GDExtension! This extension is part of the V-Sekai Rush Challenge, and we welcome contributions that help improve low-latency, finite-world multiplayer gaming.

## Development Setup

1. **Clone the repository:**
   ```bash
   git clone <repository-url>
   cd godot-zenoh
   ```

2. **Install dependencies:**
   - Rust 1.70+ with Cargo
   - Godot 4.3+
   - Zenoh router for testing

3. **Build the extension:**
   ```bash
   ./build.sh
   ```

## Guidelines

Contributions should:
- **Maintain finite world constraints:** Keep latency <30ms and memory usage <5MB
- **Include comprehensive tests:** Add unit tests for Rust code and integration tests for Godot
- **Follow Rust best practices:** Use idiomatic Rust, proper error handling, and documentation
- **Document GDScript integration:** Provide clear examples for Godot developers
- **Test multiplayer scenarios:** Ensure compatibility with Godot's multiplayer system

## Code Style

- Use `rustfmt` for formatting
- Follow the Rust API guidelines
- Use descriptive variable and function names
- Add comments for complex logic

## Testing

- Run unit tests: `cargo test`
- Test in Godot with a sample project
- Verify multiplayer functionality with Zenoh router

## Pull Request Process

1. Fork the repository
2. Create a feature branch
3. Make your changes with tests
4. Ensure all tests pass
5. Update documentation if needed
6. Submit a pull request with a clear description

## Reporting Issues

When reporting bugs, please include:
- Godot version
- Rust version
- Zenoh router version
- Steps to reproduce
- Expected vs actual behavior

## License

This extension follows the overall Forge project licensing (MIT).