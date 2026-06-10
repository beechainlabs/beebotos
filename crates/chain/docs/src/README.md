# BeeBotOS Chain

[![Version](https://img.shields.io/badge/version-1.0.0-blue.svg)](https://crates.io/crates/beebotos-chain)
[![License](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)

BeeBotOS Chain is a production-ready blockchain integration module for the BeeBotOS platform. It provides comprehensive support for Ethereum-compatible chains, including Monad, with a focus on DAO governance, identity management, and AI agent operations.

## Features

### Core Capabilities

- **Multi-Chain Support**: Ethereum, Monad, and EVM-compatible chains
- **Wallet Management**: HD wallets with BIP39/BIP32 support
- **Contract Interactions**: Type-safe contract calls with Alloy
- **DAO Governance**: Proposal creation, voting, and execution
- **Identity Management**: On-chain agent identity and reputation

### Advanced Features

- **Event Streaming**: WebSocket and polling subscription support
- **Metrics & Monitoring**: Prometheus-compatible metrics
- **Security**: Reentrancy protection, rate limiting, audit logging
- **Caching**: LRU cache with disk persistence
- **Retry Logic**: Exponential backoff with circuit breaker

## Quick Start

```rust
use beebotos_chain::config::ChainConfig;
use beebotos_chain::compat::AlloyClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Configure connection
    let config = ChainConfig::monad_testnet()?;
    
    // Create client
    let client = AlloyClient::new(&config.rpc_url).await?;
    
    // Get block number
    let block = client.get_block_number().await?;
    println!("Current block: {}", block);
    
    Ok(())
}
```

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
beebotos-chain = "1.0.0"
```

### Features

- `prometheus` (default): Enable Prometheus metrics export
- `testing`: Include test utilities

```toml
[dependencies]
beebotos-chain = { version = "1.0.0", features = ["prometheus"] }
```

## Architecture

```
beebotos-chain/
├── config/      # Chain configuration
├── wallet/      # Wallet and key management
├── dao/         # DAO governance
├── identity/    # Agent identity
├── contracts/   # Contract bindings
├── monad/       # Monad-specific types
├── metrics/     # Performance metrics
├── security/    # Security utilities
├── cache/       # Persistent caching
└── deployment/  # Contract deployment
```

## Performance

- **Operations/sec**: 100k+ cache operations
- **Latency**: <1ms for cached reads
- **Memory**: Configurable LRU cache limits
- **Metrics**: Sub-millisecond overhead

Run benchmarks:

```bash
cargo bench
```

## Security

- Reentrancy protection with `ReentrancyGuard`
- Rate limiting for contract calls
- Address validation and sanitization
- Security audit logging
- Input validation and bounds checking

## Documentation

- [API Documentation](https://docs.rs/beebotos-chain)
- [User Guide](https://beebotos.github.io/beebotos-chain)

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Contributing

Contributions are welcome! Please read our [Contributing Guide](CONTRIBUTING.md) for details.

## Support

- GitHub Issues: [Report bugs or request features](https://github.com/linc77/beebotos/issues)
- Discussions: [Ask questions or share ideas](https://github.com/linc77/beebotos/discussions)
