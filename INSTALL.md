# Installation Guide

This guide covers installing BeeBotOS and its dependencies.

## System Requirements

### Minimum Requirements
- **OS**: Linux, macOS, or Windows (WSL2)
- **CPU**: 4 cores
- **RAM**: 8 GB
- **Storage**: 50 GB available
- **Network**: Broadband connection

### Recommended Requirements
- **CPU**: 8+ cores
- **RAM**: 16+ GB
- **Storage**: 100+ GB SSD

## Prerequisites

### 1. Rust Toolchain

```bash
# Install Rust via rustup
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env

# Verify installation
rustc --version
cargo --version
```

### 2. Node.js (for Web UI)

```bash
# Using nvm (recommended)
curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.39.0/install.sh | bash
nvm install 18
nvm use 18

# Verify
node --version
npm --version
```

### 3. Foundry (for Smart Contracts)

```bash
curl -L https://foundry.paradigm.xyz | bash
foundryup

# Verify
forge --version
cast --version
```

### 4. Docker (optional, for containerized deployment)

```bash
# Install Docker Desktop or Docker Engine
# https://docs.docker.com/get-docker/

# Verify
docker --version
docker-compose --version
```

### 5. Additional Tools

```bash
# Install just (command runner)
cargo install just

# Install protobuf compiler
# macOS
brew install protobuf

# Ubuntu/Debian
sudo apt-get install -y protobuf-compiler

# Verify
protoc --version
```

## Building from Source

### 1. Clone Repository

```bash
git clone https://github.com/linc77/beebotos.git
cd beebotos
```

### 2. Build Rust Components

```bash
# Build all crates
cargo build --release

# Or build specific components
cargo build --release -p beebotos-gateway
cargo build --release -p beebotos-cli
```

### 3. Build Solidity Contracts

```bash
cd contracts/solidity
forge install
forge build
cd ../..
```

### 4. Build Web UI

```bash
cd apps/web
npm install
npm run build
cd ../..
```

## Installation Methods

### Method 1: Using Cargo (Recommended for Developers)

```bash
# Install CLI tool
cargo install --path apps/cli

# Install to specific location
cargo install --path apps/cli --root /usr/local
```

### Method 2: Using Pre-built Binaries

Download from [GitHub Releases](https://github.com/linc77/beebotos/releases):

```bash
# Linux/macOS
curl -L https://github.com/linc77/beebotos/releases/download/v1.0.0/beebotos-linux-amd64.tar.gz | tar xz
sudo mv beebotos /usr/local/bin/

# Windows
# Download .zip from releases page and extract to PATH
```

### Method 3: Using Docker

```bash
# Pull images
docker pull beebotos/gateway:latest
docker pull beebotos/agent:latest
docker pull beebotos/brain:latest

# Or build locally
docker-compose up --build
```

### Method 4: Using Installation Script

```bash
curl -sSL https://install.beebotos.dev | bash
```

## Configuration

### 1. Environment Setup

```bash
# Copy example environment file
cp .env.example .env

# Edit configuration
nano .env
```

Required variables:
```env
# API Keys
OPENAI_API_KEY=your_key_here
ANTHROPIC_API_KEY=your_key_here

# Blockchain
PRIVATE_KEY=your_private_key
RPC_URL=https://rpc.monad.xyz

# Database
DATABASE_URL=postgres://user:pass@localhost/beebotos
REDIS_URL=redis://localhost:6379
```

### 2. Initialize Database

```bash
# Run migrations
cargo run --bin migrate

# Or using sqlx
sqlx migrate run
```

### 3. Setup TEE (Optional)

For production deployments with TEE:

```bash
# Install SGX drivers (Intel)
./scripts/setup/setup-tee.sh

# Verify installation
./scripts/setup/verify-tee.sh
```

## Verification

### Test Installation

```bash
# Test CLI
beebotos --version

# Test gateway
curl http://localhost:8080/health

# Test agent runtime
beebotos agent list
```

### Run Test Suite

```bash
# Unit tests
cargo test --workspace

# Integration tests
cargo test --test integration

# Contract tests
cd contracts/solidity && forge test
```

## Troubleshooting

### Common Issues

#### "linker 'cc' not found"
```bash
# Ubuntu/Debian
sudo apt-get install build-essential

# macOS
xcode-select --install
```

#### "protoc not found"
```bash
# Install protobuf compiler
# See Prerequisites section above
```

#### "Out of memory during build"
```bash
# Reduce parallel jobs
cargo build --release -j 2

# Or increase swap space
sudo fallocate -l 8G /swapfile
sudo chmod 600 /swapfile
sudo mkswap /swapfile
sudo swapon /swapfile
```

#### "SSL certificate verify failed"
```bash
# Update certificates
# Ubuntu/Debian
sudo apt-get update && sudo apt-get install ca-certificates

# macOS
brew install ca-certificates
```

### Getting Help

- [Documentation](https://docs.beebotos.dev)
- [Discord Community](https://discord.gg/beebotos)
- [GitHub Issues](https://github.com/linc77/beebotos/issues)

## Next Steps

- [Quick Start Guide](docs/tutorials/01-quick-start.md)
- [Create Your First Agent](docs/tutorials/02-create-first-agent.md)
- [Configuration Reference](docs/guides/configuration.md)
