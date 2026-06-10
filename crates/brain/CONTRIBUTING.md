# Contributing to BeeBotOS Brain

Thank you for your interest in contributing to the beebotos-brain module!

## Development Setup

1. **Install Rust** (latest stable)
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

2. **Clone the repository**
   ```bash
   git clone https://github.com/linc77/beebotos.git
   cd beebotos/crates/brain
   ```

3. **Build and test**
   ```bash
   cargo build
   cargo test
   ```

## Code Standards

### Required Checks

Before submitting a PR, ensure:

```bash
# 1. Code compiles without warnings
cargo build --all-features

# 2. All tests pass
cargo test --all-features

# 3. Clippy is satisfied
cargo clippy --all-targets --all-features -- -D warnings

# 4. Code is formatted
cargo fmt --check

# 5. Documentation builds
cargo doc --no-deps
```

### Coding Guidelines

1. **Documentation**: All public APIs must have doc comments
2. **Error Handling**: Use `BrainResult` and avoid `unwrap()`/`expect()`
3. **Testing**: Add tests for new functionality
4. **Naming**: Follow Rust naming conventions
5. **Safety**: Handle NaN and edge cases in float operations

### Commit Messages

Use conventional commits format:

```
type(scope): description

[optional body]

[optional footer]
```

Types:
- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation changes
- `test`: Test changes
- `refactor`: Code refactoring
- `perf`: Performance improvements

Example:
```
feat(memory): add memory index for fast retrieval

Implements inverted index for O(1) average lookup time.
```

## Submitting Changes

1. **Create a branch**: `feature/description` or `fix/description`
2. **Make changes**: Follow code standards
3. **Update CHANGELOG.md**: Add entry under `[Unreleased]`
4. **Submit PR**: Link related issues

## Module Structure

```
src/
├── api.rs           # Public API
├── lib.rs           # Module exports
├── error.rs         # Error types
├── utils.rs         # Utilities
├── metrics.rs       # Performance metrics
├── neat/            # NEAT evolution
├── pad/             # PAD emotion model
├── cognition/       # Cognitive system
├── memory/          # Memory systems
├── personality/     # OCEAN personality
├── metacognition/   # Self-reflection
└── ...              # Other modules
```

## Testing Guidelines

### Unit Tests

Place in the same file as the code:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feature() {
        assert_eq!(result, expected);
    }
}
```

### Integration Tests

Place in `tests/` directory:

```rust
// tests/feature_test.rs
use beebotos_brain::*;

#[test]
fn test_integration() {
    // Test code
}
```

### Documentation Tests

Include examples in doc comments:

```rust
/// # Example
/// ```
/// use beebotos_brain::SocialBrainApi;
///
/// let api = SocialBrainApi::new();
/// ```
```

## Questions?

- Open an issue for discussion
- Join our community chat
- Check existing documentation

## License

By contributing, you agree that your contributions will be licensed under the same license as the project.
